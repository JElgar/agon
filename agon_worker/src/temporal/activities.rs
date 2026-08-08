//! Temporal activities — the thin, idempotent, retryable units that touch the
//! outside world. Each reuses the shared `agon_core` DAO / search client.
//!
//! Activities take a single (de)serializable argument, so multi-field inputs are
//! passed as structs.

use agon_core::dao::Dao;
use agon_core::search::{Index, SearchClient};
use serde::{Deserialize, Serialize};
use temporalio_macros::activities;
use temporalio_sdk::activities::{ActivityContext, ActivityError};

/// Shared dependencies available to every activity. Registered once with the
/// worker; activities read `dao` / `search` off it.
pub struct AgonActivities {
    pub dao: Dao,
    pub search: SearchClient,
}

/// One audience member: the viewer plus their capped "people you follow in
/// this match" list and their own side (if they're a participant) — see
/// `agon_core::dao::audience::AudienceMember`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedViewer {
    pub viewer_id: String,
    pub known_player_ids: Vec<String>,
    pub known_player_count: u32,
    pub viewer_side_id: Option<String>,
}

/// The result of resolving a match's fan-out: who should see it (with their
/// known-players list) and the match's start time (the feed sort key material).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutAudience {
    pub viewers: Vec<FeedViewer>,
    pub starts_at: String,
    /// False if the match no longer exists (workflow should stop).
    pub match_exists: bool,
}

/// One chunk of feed writes: the viewers in this batch for a given match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFeedChunk {
    pub viewers: Vec<FeedViewer>,
    pub match_id: String,
    pub starts_at: String,
    /// Processing timestamp, stamped by the workflow (deterministic per run).
    pub now: String,
}

/// Inputs for linking an accepted invitation to its roster entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkAccepted {
    pub invitation_id: String,
    pub accepting_user_id: String,
    pub responded_at: String,
}

#[activities]
impl AgonActivities {
    /// Resolve the deduplicated fan-out audience for a match and its start time.
    /// Returns `match_exists = false` if the match is gone (workflow no-ops).
    #[activity]
    pub async fn resolve_fanout_audience(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<FanoutAudience, ActivityError> {
        let starts_at = match self.dao.get_match(&match_id).await.map_err(activity_err)? {
            Some(agg) => agg.match_.starts_at,
            None => {
                return Ok(FanoutAudience {
                    viewers: Vec::new(),
                    starts_at: String::new(),
                    match_exists: false,
                });
            }
        };
        let audience = self
            .dao
            .resolve_fanout_audience(&match_id)
            .await
            .map_err(activity_err)?;
        let viewers = audience
            .into_iter()
            .map(|(viewer_id, member)| FeedViewer {
                viewer_id,
                known_player_ids: member.known_player_ids,
                known_player_count: member.known_player_count,
                viewer_side_id: member.viewer_side_id,
            })
            .collect();
        Ok(FanoutAudience {
            viewers,
            starts_at,
            match_exists: true,
        })
    }

    /// Write one chunk of feed entries. Idempotent on `<starts_at>#<matchId>`, so
    /// a retried chunk is harmless.
    #[activity]
    pub async fn write_feed_chunk(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        chunk: WriteFeedChunk,
    ) -> Result<(), ActivityError> {
        let viewers: Vec<agon_core::dao::feed::FeedViewer> = chunk
            .viewers
            .into_iter()
            .map(|v| agon_core::dao::feed::FeedViewer {
                viewer_id: v.viewer_id,
                audience: agon_core::dao::audience::AudienceMember {
                    known_player_ids: v.known_player_ids,
                    known_player_count: v.known_player_count,
                    viewer_side_id: v.viewer_side_id,
                },
            })
            .collect();
        self.dao
            .write_feed_items(&viewers, &chunk.match_id, &chunk.starts_at, &chunk.now)
            .await
            .map_err(activity_err)
    }

    /// Ensure a match is present in the search index (idempotent upsert). Used at
    /// the end of fan-out so a newly-created match is discoverable even before
    /// the inline indexing stream event lands.
    #[activity]
    pub async fn index_match(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<(), ActivityError> {
        match self.dao.get_match(&match_id).await.map_err(activity_err)? {
            Some(agg) => {
                let doc = crate::handlers::index::match_search_doc(&agg);
                self.search
                    .upsert(Index::Matches, &doc)
                    .await
                    .map_err(search_err)
            }
            None => Ok(()),
        }
    }

    /// Reconcile every participant's stat contribution for a match. Idempotent.
    /// Used by the accept saga so accepting an invite into an already-completed
    /// match credits the newly-linked player — a `PLAYER#` link alone doesn't
    /// touch `#META`, so the stream-driven stats handler won't fire for it.
    #[activity]
    pub async fn reconcile_match_stats(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<(), ActivityError> {
        crate::handlers::stats::reconcile_match_stats(&self.dao, &match_id)
            .await
            .map_err(worker_err)
    }

    /// Link an accepted invitation's roster entry (match player / team member) to
    /// the accepting user. Idempotent.
    #[activity]
    pub async fn link_accepted_invitation(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        input: LinkAccepted,
    ) -> Result<(), ActivityError> {
        self.dao
            .link_accepted_invitation(
                &input.invitation_id,
                &input.accepting_user_id,
                &input.responded_at,
            )
            .await
            .map_err(activity_err)
    }

    /// Refresh every side's cached roster preview from the match's current
    /// player collection. Idempotent. Used by the accept saga — linking an
    /// invitee can flip a cached preview entry from external to a real user
    /// (see `Dao::refresh_side_roster_previews`).
    #[activity]
    pub async fn refresh_side_roster_previews(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<(), ActivityError> {
        self.dao
            .refresh_side_roster_previews(&match_id)
            .await
            .map_err(activity_err)
    }
}

/// Map a DAO error into a Temporal `ActivityError` (an Application error, so the
/// activity retries per its retry policy). `DaoError` is a `std::error::Error`
/// (thiserror), which `ActivityError` accepts via its blanket `From`.
fn activity_err(err: agon_core::dao::error::DaoError) -> ActivityError {
    ActivityError::from(err)
}

/// Same, for a Meilisearch error.
fn search_err(err: agon_core::error::SearchError) -> ActivityError {
    ActivityError::from(err)
}

/// Same, for a worker-handler error (used by activities that reuse an inline
/// handler's logic, e.g. stats reconciliation).
fn worker_err(err: crate::error::WorkerError) -> ActivityError {
    ActivityError::from(err)
}
