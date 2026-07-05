//! Fan-out feed operations: write feed entries for a set of viewers, and read a
//! viewer's feed newest-first.
//!
//! Feed entries are thin pointers (`ref_type`/`ref_id` + `starts_at`), one row
//! per viewer, living under `UFEED#<viewer>` / `FEED#<starts_at>#<matchId>`.
//! Writes are idempotent on the sort key: re-fanning-out the same match to the
//! same viewer overwrites the identical row, so the at-least-once fan-out
//! workflow can replay a chunk harmlessly.
//!
//! Writes use `BatchWriteItem` (25 items/request max) so a large audience is
//! written efficiently; the caller (the fan-out workflow) chunks the audience so
//! a mid-way failure resumes rather than restarts.

use aws_sdk_dynamodb::types::{PutRequest, WriteRequest};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ItemBuilder, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::FeedItemRecord;

pub const TYPE_FEED_ITEM: &str = "feed_item";

/// DynamoDB's hard cap on items per `BatchWriteItem` request.
const BATCH_WRITE_MAX: usize = 25;

impl Dao {
    /// Write one feed entry per viewer for a match, in batches. Idempotent on
    /// `<starts_at>#<matchId>` (an overwrite is a no-op in effect). Retries any
    /// `UnprocessedItems` DynamoDB returns (throttling) until the batch drains.
    ///
    /// `viewer_ids` should already be deduplicated by the caller.
    pub async fn write_feed_items(
        &self,
        viewer_ids: &[String],
        match_id: &str,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<()> {
        for chunk in viewer_ids.chunks(BATCH_WRITE_MAX) {
            let mut requests = Vec::with_capacity(chunk.len());
            for viewer_id in chunk {
                let record = FeedItemRecord {
                    viewer_id: viewer_id.clone(),
                    ref_type: "match".into(),
                    ref_id: match_id.into(),
                    starts_at: starts_at.into(),
                    created_at: now.into(),
                };
                let item = ItemBuilder::new(to_item(
                    &Pk::UserFeed(viewer_id.clone()),
                    &Sk::Feed {
                        starts_at: starts_at.into(),
                        match_id: match_id.into(),
                    },
                    TYPE_FEED_ITEM,
                    &record,
                )?)
                .build();

                let put = PutRequest::builder()
                    .set_item(Some(item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                requests.push(WriteRequest::builder().put_request(put).build());
            }

            self.flush_batch_write(requests).await?;
        }
        Ok(())
    }

    /// List a viewer's feed newest-first (by match `starts_at`), paginated.
    pub async fn list_feed(
        &self,
        viewer_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<FeedItemRecord>> {
        use super::item::{ATTR_PK, s};
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::UserFeed(viewer_id.into()).to_string()))
                .expression_attribute_values(
                    ":sk",
                    s(Sk::Feed {
                        starts_at: String::new(),
                        match_id: String::new(),
                    }
                    .prefix()),
                )
                .scan_index_forward(false), // newest (latest starts_at) first
            cursor,
            limit,
        )
        .await
    }

    /// Submit a batch of write requests, retrying `UnprocessedItems` (which
    /// DynamoDB returns under throttling) until the batch fully drains.
    async fn flush_batch_write(&self, requests: Vec<WriteRequest>) -> DaoResult<()> {
        let mut pending = requests;
        // Bounded retry loop; DynamoDB drains unprocessed items across attempts.
        for _ in 0..10 {
            if pending.is_empty() {
                return Ok(());
            }
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
