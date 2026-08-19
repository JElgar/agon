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

use super::audience::AudienceMember;
use super::batch::BATCH_WRITE_MAX;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ItemBuilder, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::FeedItemRecord;

pub const TYPE_FEED_ITEM: &str = "feed_item";

/// One viewer's feed entry to write for a match — the viewer id plus their
/// [`AudienceMember`] data (known-players list / own side, see
/// `Dao::resolve_fanout_audience`).
#[derive(Debug, Clone)]
pub struct FeedViewer {
    pub viewer_id: String,
    pub audience: AudienceMember,
}

impl Dao {
    /// Write one feed entry per viewer for a match, in batches. Idempotent on
    /// `<starts_at>#<matchId>` (an overwrite is a no-op in effect, and fully
    /// replaces the item — including `known_player_ids`/`viewer_side_id` — so
    /// a re-fan-out refreshes it to the audience computation's current
    /// state). Retries any `UnprocessedItems` DynamoDB returns (throttling)
    /// until the batch drains.
    ///
    /// `viewers` should already be deduplicated by the caller.
    #[tracing::instrument(skip(self))]
    pub async fn write_feed_items(
        &self,
        viewers: &[FeedViewer],
        match_id: &str,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<()> {
        for chunk in viewers.chunks(BATCH_WRITE_MAX) {
            let mut requests = Vec::with_capacity(chunk.len());
            for viewer in chunk {
                let item = self.feed_item(
                    &viewer.viewer_id,
                    match_id,
                    starts_at,
                    now,
                    &viewer.audience,
                )?;
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

    /// Build one viewer's feed-entry item for a match. Idempotent on the sort key
    /// `<starts_at>#<matchId>`, so writing it again (via fan-out) overwrites the
    /// identical row. Shared with the accept / create transactions, which write a
    /// participant's *own* row synchronously (with a default `AudienceMember` —
    /// empty `known_player_ids`, zero `known_player_count` — it's their own
    /// match, not someone they follow playing in it — plus their own `side_id`,
    /// if assigned).
    pub(super) fn feed_item(
        &self,
        viewer_id: &str,
        match_id: &str,
        starts_at: &str,
        now: &str,
        audience: &AudienceMember,
    ) -> DaoResult<super::item::Item> {
        let record = FeedItemRecord {
            viewer_id: viewer_id.into(),
            ref_type: "match".into(),
            ref_id: match_id.into(),
            starts_at: starts_at.into(),
            created_at: now.into(),
            known_player_ids: audience.known_player_ids.clone(),
            known_player_count: audience.known_player_count,
            viewer_side_id: audience.viewer_side_id.clone(),
        };
        Ok(ItemBuilder::new(to_item(
            &Pk::UserFeed(viewer_id.into()),
            &Sk::Feed {
                starts_at: starts_at.into(),
                match_id: match_id.into(),
            },
            TYPE_FEED_ITEM,
            &record,
        )?)
        .build())
    }

    /// List a viewer's feed newest-first (by match `starts_at`), paginated.
    #[tracing::instrument(skip(self))]
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
                .expression_attribute_values(":sk", s(Sk::feed_prefix()))
                .scan_index_forward(false), // newest (latest starts_at) first
            cursor,
            limit,
        )
        .await
    }
}
