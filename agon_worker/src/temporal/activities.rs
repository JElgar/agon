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
    pub viewer_is_going: bool,
    pub viewer_invitation_pending: bool,
    pub viewer_can_join_side_ids: Vec<String>,
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

/// One chunk of stale feed-row deletes: a reschedule's old `starts_at` plus
/// the viewers (by id only — no audience data needed to delete) who may still
/// have a row under it. See `Dao::delete_feed_items`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFeedChunk {
    pub viewer_ids: Vec<String>,
    pub match_id: String,
    pub old_starts_at: String,
}

/// Inputs for linking an accepted invitation to its roster entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkAccepted {
    pub invitation_id: String,
    pub accepting_user_id: String,
    pub responded_at: String,
}

/// Inputs for generating one occurrence of a series, passed in full each
/// call (mirrors `WriteFeedChunk`'s own shape) rather than making the
/// activity re-fetch anything the workflow already has in hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateSeriesOccurrence {
    pub series_id: String,
    /// 0-based occurrence index — also the exact `occurrences_generated`
    /// value this occurrence's generation is conditioned on (see
    /// `Dao::bump_series_occurrences_generated`): generating occurrence
    /// `index` requires the series' counter to currently read `index`.
    pub index: u32,
    /// This occurrence's full start time (already advanced from the
    /// series' first occurrence by the workflow's pure date math — see
    /// `agon_core::series::occurrence_date`), RFC-3339.
    pub starts_at: String,
    pub created_by_user_id: String,
    pub match_type: String,
    pub name: String,
    pub description: String,
    pub location: Option<agon_core::dao::records::LocationRecord>,
    pub sides: Vec<agon_core::dao::records::SeriesSideRecord>,
}

#[activities]
impl AgonActivities {
    /// Fetch a series by id. `None` if it's gone (the workflow no-ops).
    #[activity]
    pub async fn get_series(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        series_id: String,
    ) -> Result<Option<agon_core::dao::records::SeriesRecord>, ActivityError> {
        self.dao.get_series(&series_id).await.map_err(activity_err)
    }

    /// Generate one occurrence of a series: claim its generation slot, create
    /// its match (team-linked sides only, with team self-join turned on and
    /// no invited players — a series only configures a team per side and has
    /// no invite-list support yet, so roster join happens per-occurrence via
    /// each side's own team; the organiser gets an unassigned, non-invited
    /// `Owner` row of their own, since they never play in the template),
    /// then bump the series' `occurrences_generated` counter. Every
    /// step is individually idempotent, so a retry of this whole activity —
    /// for an occurrence this activity (or an earlier attempt) already fully
    /// or partially generated — is always safe:
    /// - `claim_series_occurrence` is a conditional put keyed on
    ///   `(series_id, starts_at)`: if this attempt loses the race, it gets
    ///   back whichever match id the winning attempt offered, and uses that
    ///   one instead of the fresh id it generated for itself (see
    ///   `agon_core::dao::series_ops`'s doc comment — this is what replaced
    ///   the old hash-derived match id).
    /// - `create_match` on the claimed match id hits `Conflict` if that
    ///   occurrence's match already exists (the common case on a retry —
    ///   either this attempt won the claim and created it before, or it lost
    ///   the claim to an attempt that already created it), treated as
    ///   already-done rather than an error. If the claim was written but the
    ///   match never got created (a crash in between), this call still
    ///   creates it now, under the same claimed id.
    /// - `bump_series_occurrences_generated` is itself conditioned on the
    ///   counter still reading `index` (see
    ///   `Dao::bump_series_occurrences_generated`'s doc comment), so a
    ///   retry that already bumped it is a no-op rather than a double count.
    #[activity]
    pub async fn generate_series_occurrence(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        input: GenerateSeriesOccurrence,
    ) -> Result<(), ActivityError> {
        let claim = agon_core::dao::records::SeriesOccurrenceRecord {
            series_id: input.series_id.clone(),
            index: input.index,
            starts_at: input.starts_at.clone(),
            match_id: agon_core::id::new_id(),
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };
        let claimed = self
            .dao
            .claim_series_occurrence(&claim)
            .await
            .map_err(activity_err)?;
        let match_id = claimed.match_id;

        let mut sides: std::collections::HashMap<String, agon_core::dao::records::MatchSideRecord> =
            Default::default();
        for side in &input.sides {
            let side_id = agon_core::id::new_id();
            sides.insert(
                side_id.clone(),
                agon_core::dao::records::MatchSideRecord {
                    side_id,
                    team_id: Some(side.team_id.clone()),
                    name: side.name.clone(),
                    colour: side.colour.clone(),
                    max_players: side.max_players,
                    // Roster join happens per-occurrence via each side's own
                    // team (no invite-list support for a series yet) — this
                    // has to be on or nobody can ever get onto a generated
                    // occurrence.
                    team_join_enabled: true,
                    player_count: 0,
                    roster_preview: Vec::new(),
                },
            );
        }
        // Mirrors `create_match_series`'s own organiser row: the series
        // organiser never plays in the template, so without this they'd
        // have no roster row to carry `MatchPlayerRole::Owner` on any
        // occurrence (see that type's doc comment).
        let organiser_player = agon_core::dao::records::MatchPlayerRecord {
            player_id: agon_core::id::new_id(),
            user_id: Some(input.created_by_user_id.clone()),
            display_name: None,
            side_id: None,
            is_member_of_team: None,
            invitation: None,
            role: agon_core::dao::records::MatchPlayerRole::Owner,
            joined_via: None,
        };

        let match_record = agon_core::dao::records::MatchRecord {
            id: match_id.clone(),
            created_by_user_id: input.created_by_user_id,
            name: input.name,
            description: input.description,
            match_type: input.match_type,
            status: "scheduled".to_string(),
            starts_at: input.starts_at,
            allow_unassigned: true,
            total_player_count: 0,
            location: input.location,
            series_id: Some(input.series_id.clone()),
            header_photos: Vec::new(),
            sides,
            confirmed_score: None,
            pending_score: None,
            like_count: 0,
            comment_count: 0,
            live_seq: 0,
            live_tip_seq: None,
            format: None,
            // A series occurrence is always friendly. `SeriesRecord` has no
            // `ranked` of its own, and generation puts *other* people's
            // players into spots without asking them — which is the one thing
            // a rating needs (see `rating_consent`). An organiser can switch a
            // single occurrence to ranked afterwards, which asks everyone in
            // it. A ranked *series* needs its own consent design first.
            ranked: false,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };

        match self
            .dao
            .create_match(&match_record, &[organiser_player])
            .await
        {
            Ok(()) => {}
            Err(agon_core::dao::error::DaoError::Conflict(_)) => {
                // Already generated — by this attempt's own earlier try, or
                // by whichever attempt won the claim above. Fall through to
                // (re-)bump the counter, which is itself a no-op if that
                // earlier attempt already did it too.
            }
            Err(e) => return Err(activity_err(e)),
        }

        self.dao
            .bump_series_occurrences_generated(&input.series_id, input.index)
            .await
            .map_err(activity_err)
    }

    /// Flip a series' status once generation finishes. Idempotent: guarded
    /// on the series still reading `"generating"` (see
    /// `Dao::update_series_status`), so a replay/retry of this final step
    /// after it already landed is a no-op.
    #[activity]
    pub async fn activate_series(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        series_id: String,
    ) -> Result<(), ActivityError> {
        self.dao
            .update_series_status(&series_id, "generating", "active")
            .await
            .map_err(activity_err)
    }

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
                viewer_is_going: member.viewer_is_going,
                viewer_invitation_pending: member.viewer_invitation_pending,
                viewer_can_join_side_ids: member.viewer_can_join_side_ids,
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
                    viewer_is_going: v.viewer_is_going,
                    viewer_invitation_pending: v.viewer_invitation_pending,
                    viewer_can_join_side_ids: v.viewer_can_join_side_ids,
                },
            })
            .collect();
        self.dao
            .write_feed_items(&viewers, &chunk.match_id, &chunk.starts_at, &chunk.now)
            .await
            .map_err(activity_err)
    }

    /// Delete one chunk of stale feed rows left behind under a match's
    /// previous `starts_at` after a reschedule. Idempotent — deleting an
    /// already-gone row is a no-op.
    #[activity]
    pub async fn delete_stale_feed_chunk(
        self: std::sync::Arc<Self>,
        _ctx: ActivityContext,
        chunk: DeleteFeedChunk,
    ) -> Result<(), ActivityError> {
        self.dao
            .delete_feed_items(&chunk.viewer_ids, &chunk.match_id, &chunk.old_starts_at)
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
