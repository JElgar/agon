//! Shared batch-operation plumbing: `BatchGetItem` with unprocessed-key retry,
//! and the backoff used between attempts by both the batch get here and the
//! `BatchWriteItem` fan-out in `feed.rs`.
//!
//! `BatchGetItem` / `BatchWriteItem` can partially complete on a *successful*
//! (200) response — under throttling or the 16 MB response cap DynamoDB returns
//! the keys/items it didn't process and expects the caller to re-request them.
//! That's distinct from the SDK's transport-level retries (which cover a failed
//! call), so we drive it ourselves.

use std::time::Duration;

use aws_sdk_dynamodb::types::KeysAndAttributes;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::Item;

/// DynamoDB's hard cap on keys per `BatchGetItem` request. Callers batch a page
/// at a time, which the service caps (`MAX_PAGE_LIMIT`) well below this, so no
/// request ever needs splitting.
pub(super) const BATCH_GET_MAX: usize = 100;

/// How many times to re-request unprocessed keys/items before giving up. A
/// couple of retries clears transient throttling; past that we fail the whole
/// call loudly (dropping whatever *did* come back) rather than spin — a partial
/// page is worse than a clean, retryable error. Shared by batch get (here) and
/// batch write (`feed.rs`).
pub(super) const MAX_BATCH_ATTEMPTS: u32 = 3;

/// Sleep before a retry attempt: capped exponential backoff, no wait on the
/// first attempt. Gives DynamoDB room to recover between partial completions
/// instead of hammering it back-to-back. Attempts 0,1,2,3,… → 0, 25, 50, 100,
/// 200 ms … capped at 1s.
pub(super) async fn backoff(attempt: u32) {
    if attempt == 0 {
        return;
    }
    let ms = (25u64 << (attempt - 1)).min(1_000);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

impl Dao {
    /// Fetch every base-table item for `keys` in one `BatchGetItem`, re-requesting
    /// any `UnprocessedKeys` with [`backoff`] between attempts. Returns the items
    /// that came back, in no particular order; keys with no item are simply
    /// absent (so the caller keys the result by id and treats a miss as `None`).
    ///
    /// `projection` optionally restricts the attributes fetched (e.g. just the
    /// `PK` for an existence check). `keys` must hold at most [`BATCH_GET_MAX`]
    /// entries and must be free of duplicates — DynamoDB rejects a request that
    /// repeats a key; callers de-dup while building the key list.
    pub(super) async fn batch_get_all(
        &self,
        keys: Vec<Item>,
        projection: Option<&str>,
    ) -> DaoResult<Vec<Item>> {
        debug_assert!(
            keys.len() <= BATCH_GET_MAX,
            "batch_get_all called with more than the BatchGetItem key limit"
        );
        let mut pending = keys;
        let mut out = Vec::new();

        for attempt in 0..MAX_BATCH_ATTEMPTS {
            if pending.is_empty() {
                break;
            }
            backoff(attempt).await;

            let mut builder =
                KeysAndAttributes::builder().set_keys(Some(std::mem::take(&mut pending)));
            if let Some(projection) = projection {
                builder = builder.projection_expression(projection);
            }
            let keys_and_attrs = builder
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            let resp = self
                .client
                .batch_get_item()
                .request_items(self.table(), keys_and_attrs)
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            if let Some(items) = resp.responses.and_then(|mut r| r.remove(self.table())) {
                out.extend(items);
            }
            if let Some(unprocessed) = resp
                .unprocessed_keys
                .and_then(|mut u| u.remove(self.table()))
            {
                pending = unprocessed.keys;
            }
        }

        if !pending.is_empty() {
            return Err(DaoError::Dynamo(
                "batch get did not drain unprocessed keys".into(),
            ));
        }

        Ok(out)
    }
}
