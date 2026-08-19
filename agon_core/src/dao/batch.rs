//! Shared batch-operation plumbing: `BatchGetItem` with unprocessed-key retry,
//! `BatchWriteItem` with unprocessed-item retry, and the backoff used between
//! attempts by both.
//!
//! `BatchGetItem` / `BatchWriteItem` can partially complete on a *successful*
//! (200) response — under throttling or the 16 MB response cap DynamoDB returns
//! the keys/items it didn't process and expects the caller to re-request them.
//! That's distinct from the SDK's transport-level retries (which cover a failed
//! call), so we drive it ourselves.

use std::time::Duration;

use aws_sdk_dynamodb::types::{KeysAndAttributes, WriteRequest};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::Item;

/// DynamoDB's hard cap on keys per `BatchGetItem` request. Callers batch a page
/// at a time, which the service caps (`MAX_PAGE_LIMIT`) well below this, so no
/// request ever needs splitting.
pub(super) const BATCH_GET_MAX: usize = 100;

/// DynamoDB's hard cap on items per `BatchWriteItem` request (puts and deletes
/// alike). Callers chunk their requests into pages of this size.
pub(super) const BATCH_WRITE_MAX: usize = 25;

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
    /// Fetch every base-table item for `keys`, in as few `BatchGetItem`
    /// requests as `BATCH_GET_MAX` allows. Returns the items that came back,
    /// in no particular order; keys with no item are simply absent (so the
    /// caller keys the result by id and treats a miss as `None`).
    ///
    /// `projection` optionally restricts the attributes fetched (e.g. just the
    /// `PK` for an existence check). `keys` must be free of duplicates —
    /// DynamoDB rejects a request that repeats a key; callers de-dup while
    /// building the key list. Most callers' input is bounded by a
    /// service-capped page and fits in one request; a caller unioning ids
    /// across several sources per page (e.g. a feed page's known-participant
    /// and roster-preview ids together) may occasionally need a couple more —
    /// still a small, bounded number of requests, not one per source item.
    #[tracing::instrument(skip(self))]
    pub(super) async fn batch_get_all(
        &self,
        keys: Vec<Item>,
        projection: Option<&str>,
    ) -> DaoResult<Vec<Item>> {
        let mut out = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(BATCH_GET_MAX) {
            out.extend(self.batch_get_chunk(chunk.to_vec(), projection).await?);
        }
        Ok(out)
    }

    /// One `BatchGetItem` request's worth of work (at most [`BATCH_GET_MAX`]
    /// keys): send, and re-request any `UnprocessedKeys` with [`backoff`]
    /// between attempts until the chunk fully drains.
    async fn batch_get_chunk(
        &self,
        keys: Vec<Item>,
        projection: Option<&str>,
    ) -> DaoResult<Vec<Item>> {
        debug_assert!(
            keys.len() <= BATCH_GET_MAX,
            "batch_get_chunk called with more than the BatchGetItem key limit"
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

    /// Submit one `BatchWriteItem` request's worth of work (at most
    /// [`BATCH_WRITE_MAX`] requests — puts and/or deletes), retrying any
    /// `UnprocessedItems` DynamoDB returns (throttling) with [`backoff`]
    /// between attempts until the batch fully drains. Shared by every
    /// batch-write caller (feed fan-out, roster removal, …) so they don't
    /// each re-implement the retry loop.
    pub(super) async fn flush_batch_write(&self, requests: Vec<WriteRequest>) -> DaoResult<()> {
        debug_assert!(
            requests.len() <= BATCH_WRITE_MAX,
            "flush_batch_write called with more than the BatchWriteItem request limit"
        );
        let mut pending = requests;
        for attempt in 0..MAX_BATCH_ATTEMPTS {
            if pending.is_empty() {
                return Ok(());
            }
            backoff(attempt).await;

            let out = self
                .client
                .batch_write_item()
                .request_items(self.table(), std::mem::take(&mut pending))
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            if let Some(unprocessed) = out.unprocessed_items
                && let Some(items) = unprocessed.get(self.table())
                && !items.is_empty()
            {
                pending = items.clone();
            }
        }
        if pending.is_empty() {
            Ok(())
        } else {
            Err(DaoError::Dynamo(
                "batch write did not drain unprocessed items".into(),
            ))
        }
    }
}
