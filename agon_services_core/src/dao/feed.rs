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

use std::collections::{HashMap, HashSet};

use aws_sdk_dynamodb::types::{DeleteRequest, PutRequest, WriteRequest};

use super::audience::AudienceMember;
use super::batch::BATCH_WRITE_MAX;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, ItemBuilder, from_item, item_pk, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{FeedItemRecord, TeamRecord};

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
            viewer_is_going: audience.viewer_is_going,
            viewer_invitation_pending: audience.viewer_invitation_pending,
            viewer_can_join_side_ids: audience.viewer_can_join_side_ids.clone(),
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

    /// Delete one stale feed entry per viewer for a match, at a given
    /// `starts_at` — used when a reschedule changes the match's `starts_at`,
    /// since that's part of the sort key (see the module doc comment): the
    /// re-fan-out this triggers writes a *new* row under the new `starts_at`
    /// rather than updating the old one in place, so the row left behind
    /// under the previous `starts_at` must be deleted explicitly or the match
    /// shows twice in affected viewers' feeds. A no-op `DeleteItem` for a
    /// viewer who never had a row at that key (e.g. a follower added after
    /// the reschedule) is harmless.
    #[tracing::instrument(skip(self))]
    pub async fn delete_feed_items(
        &self,
        viewer_ids: &[String],
        match_id: &str,
        old_starts_at: &str,
    ) -> DaoResult<()> {
        for chunk in viewer_ids.chunks(BATCH_WRITE_MAX) {
            let mut requests = Vec::with_capacity(chunk.len());
            for viewer_id in chunk {
                let key = [
                    (
                        ATTR_PK.to_string(),
                        s(Pk::UserFeed(viewer_id.clone()).to_string()),
                    ),
                    (
                        ATTR_SK.to_string(),
                        s(Sk::Feed {
                            starts_at: old_starts_at.into(),
                            match_id: match_id.into(),
                        }
                        .to_string()),
                    ),
                ]
                .into_iter()
                .collect();
                let delete = DeleteRequest::builder()
                    .set_key(Some(key))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                requests.push(WriteRequest::builder().delete_request(delete).build());
            }
            self.flush_batch_write(requests).await?;
        }
        Ok(())
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

    /// One `BatchGetItem` for what a team's "next match" card needs from
    /// DynamoDB beyond the stats store: the viewer's own feed entries for
    /// some matches, and some teams' metas (opponent logos).
    ///
    /// The feed entry is where the feed reads `viewer_is_going` from
    /// (`FeedItemRecord::viewer_is_going`, set by the fan-out audience and,
    /// synchronously, by the create/join/accept transactions), so a card
    /// built from this agrees with the viewer's feed. It's addressed by
    /// `(match_id, starts_at)`; a viewer with no entry for a match (not a
    /// participant, not following anyone involved) simply has none in the
    /// result.
    ///
    /// Returns the feed entries keyed by match id and the teams keyed by
    /// team id. No request at all when both lists are empty.
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_feed_entries_and_team_metas(
        &self,
        viewer_id: &str,
        matches: &[(String, String)],
        team_ids: &[String],
    ) -> DaoResult<(HashMap<String, FeedItemRecord>, HashMap<String, TeamRecord>)> {
        let key = |pk: Pk, sk: Sk| {
            HashMap::from([
                (ATTR_PK.to_string(), s(pk.to_string())),
                (ATTR_SK.to_string(), s(sk.to_string())),
            ])
        };
        // De-dup — BatchGetItem rejects duplicate keys in one request.
        let mut seen = HashSet::new();
        let mut keys = Vec::with_capacity(matches.len() + team_ids.len());
        for (match_id, starts_at) in matches {
            if seen.insert(("m", match_id.as_str(), starts_at.as_str())) {
                keys.push(key(
                    Pk::UserFeed(viewer_id.into()),
                    Sk::Feed {
                        starts_at: starts_at.clone(),
                        match_id: match_id.clone(),
                    },
                ));
            }
        }
        for team_id in team_ids {
            if seen.insert(("t", team_id.as_str(), "")) {
                keys.push(key(Pk::Team(team_id.clone()), Sk::Meta));
            }
        }

        let mut feed = HashMap::new();
        let mut teams = HashMap::new();
        for item in self.batch_get_all(keys, None).await? {
            match item_pk(&item)? {
                Pk::UserFeed(_) => {
                    let record: FeedItemRecord = from_item(item)?;
                    feed.insert(record.ref_id.clone(), record);
                }
                Pk::Team(_) => {
                    let record: TeamRecord = from_item(item)?;
                    teams.insert(record.id.clone(), record);
                }
                other => {
                    return Err(DaoError::Malformed(format!(
                        "batch_get_feed_entries_and_team_metas returned an item under {other}"
                    )));
                }
            }
        }
        Ok((feed, teams))
    }
}
