//! Temporal activities — the thin, idempotent, retryable units that touch the
//! outside world. Each reuses the shared `agon_core` DAO / search client.
//!
//! ⚠️ UNVERIFIED: written against the Temporal Rust SDK **Public Preview** API
//! (`temporalio-sdk` README as of 2026-07). That SDK is a churning git
//! dependency with no crates.io release and cannot be compiled in this
//! environment, so signatures here may need adjustment against the exact SDK
//! revision you pin. Gated behind the `temporal` cargo feature.
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

/// The result of resolving a match's fan-out: who should see it and the match's
/// start time (the feed sort key material).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutAudience {
    pub viewer_ids: Vec<String>,
    pub starts_at: String,
    /// False if the match no longer exists (workflow should stop).
    pub match_exists: bool,
}

/// One chunk of feed writes: the viewers in this batch for a given match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFeedChunk {
    pub viewer_ids: Vec<String>,
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
        &self,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<FanoutAudience, ActivityError> {
        let starts_at = match self.dao.get_match(&match_id).await.map_err(activity_err)? {
            Some(agg) => agg.match_.starts_at,
            None => {
                return Ok(FanoutAudience {
                    viewer_ids: Vec::new(),
                    starts_at: String::new(),
                    match_exists: false,
                });
            }
        };
        let viewer_ids = self
            .dao
            .resolve_fanout_audience(&match_id)
            .await
            .map_err(activity_err)?;
        Ok(FanoutAudience {
            viewer_ids,
            starts_at,
            match_exists: true,
        })
    }

    /// Write one chunk of feed entries. Idempotent on `<starts_at>#<matchId>`, so
    /// a retried chunk is harmless.
    #[activity]
    pub async fn write_feed_chunk(
        &self,
        _ctx: ActivityContext,
        chunk: WriteFeedChunk,
    ) -> Result<(), ActivityError> {
        self.dao
            .write_feed_items(
                &chunk.viewer_ids,
                &chunk.match_id,
                &chunk.starts_at,
                &chunk.now,
            )
            .await
            .map_err(activity_err)
    }

    /// Ensure a match is present in the search index (idempotent upsert). Used at
    /// the end of fan-out so a newly-created match is discoverable even before
    /// the inline indexing stream event lands.
    #[activity]
    pub async fn index_match(
        &self,
        _ctx: ActivityContext,
        match_id: String,
    ) -> Result<(), ActivityError> {
        match self.dao.get_match(&match_id).await.map_err(activity_err)? {
            Some(agg) => {
                let doc = crate::handlers::index::match_search_doc(&agg);
                self.search
                    .upsert(Index::Matches, &doc)
                    .await
                    .map_err(activity_err)
            }
            None => Ok(()),
        }
    }

    /// Link an accepted invitation's roster entry (match player / team member) to
    /// the accepting user. Idempotent.
    #[activity]
    pub async fn link_accepted_invitation(
        &self,
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
}

/// Map a DAO error into a Temporal `ActivityError` so the activity retries.
fn activity_err(err: agon_core::dao::error::DaoError) -> ActivityError {
    ActivityError::from(anyhow::anyhow!(err.to_string()))
}
