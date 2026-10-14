//! Temporal workflows — deterministic orchestration of the multi-step async
//! work (feed fan-out, the accept-invitation saga). Workflows call activities;
//! they never touch DynamoDB / the network directly.
//!
//! Built against the Temporal Rust SDK (crates.io 0.5) — the workflow/activity
//! macros, `WorkflowContext::start_activity` and `workflow_time`, and the
//! unit-struct + `#[run(ctx, input)]` shape all match the SDK's own examples.
//!
//! Idempotency / determinism:
//! - Workflow ids are deterministic (`fanout-<match_id>`, `accept-<inv_id>`) and
//!   started with `UseExisting`, so a duplicate start attaches to the running
//!   run (see docs/async-design.md §3).
//! - Every activity's effects are idempotent (feed writes keyed by match id,
//!   link is a fixed-point update), so activity retries are safe.
//! - Timestamps come from `ctx.workflow_time()` (deterministic on replay), never
//!   the wall clock.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ActivityOptions, WorkflowContext, WorkflowResult};

use super::activities::{
    AgonActivities, DeleteFeedChunk, GenerateSeriesOccurrence, LinkAccepted, WriteFeedChunk,
};

/// How many feed rows to write per activity invocation. Each chunk is a
/// separately-retryable, checkpointed step — the whole point of running fan-out
/// on Temporal (a mid-way failure resumes, not restarts).
const FEED_CHUNK: usize = 500;

/// Default activity timeout. Fan-out chunks and single DAO ops are all quick.
fn activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(30))
}

/// An RFC3339 timestamp from the workflow's deterministic clock. Falls back to
/// empty if the context has no time yet (shouldn't happen inside `run`).
fn workflow_now(ctx: &WorkflowContext<impl Sized>) -> String {
    ctx.workflow_time()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        .unwrap_or_default()
}

// ===========================================================================
// FanOutMatch — fan a match into its audience's feeds.
// ===========================================================================

/// Inputs to the fan-out workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanOutMatchInput {
    pub match_id: String,
    /// The match's `starts_at` before this write, when this run was triggered
    /// by a change that moved it (a reschedule). `starts_at` is part of the
    /// feed row's sort key (see `agon_core::dao::feed`), so a re-fan-out after
    /// a reschedule writes a *new* row rather than updating the old one in
    /// place — `None` for every other trigger (creation, an invitation
    /// acceptance, any other meta edit) skips the cleanup step below.
    pub old_starts_at: Option<String>,
}

/// Fan a match into the feeds of everyone who should see it. Started when a
/// match is created / completed. Workflow id: `fanout-<match_id>`.
#[workflow]
#[derive(Default)]
pub struct FanOutMatch;

#[workflow_methods]
impl FanOutMatch {
    #[run]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: FanOutMatchInput,
    ) -> WorkflowResult<()> {
        let FanOutMatchInput {
            match_id,
            old_starts_at,
        } = input;

        // 1. Resolve the audience + the match start time.
        let audience = ctx
            .start_activity(
                AgonActivities::resolve_fanout_audience,
                match_id.clone(),
                activity_opts(),
            )
            .await?;

        if !audience.match_exists {
            return Ok(());
        }

        // 2. Write feed entries in checkpointed chunks. Each chunk is its own
        //    activity, so a failure resumes at the failed chunk on replay.
        let now = workflow_now(ctx);
        for chunk in audience.viewers.chunks(FEED_CHUNK) {
            ctx.start_activity(
                AgonActivities::write_feed_chunk,
                WriteFeedChunk {
                    viewers: chunk.to_vec(),
                    match_id: match_id.clone(),
                    starts_at: audience.starts_at.clone(),
                    now: now.clone(),
                },
                activity_opts(),
            )
            .await?;
        }

        // 2b. A reschedule moved `starts_at`: the rows just written above live
        //     under the new sort key, so delete the stale rows left behind
        //     under the old one (otherwise the match shows twice in the
        //     affected viewers' feeds).
        if let Some(old_starts_at) = old_starts_at.filter(|old| *old != audience.starts_at) {
            let viewer_ids: Vec<String> = audience
                .viewers
                .iter()
                .map(|v| v.viewer_id.clone())
                .collect();
            for chunk in viewer_ids.chunks(FEED_CHUNK) {
                ctx.start_activity(
                    AgonActivities::delete_stale_feed_chunk,
                    DeleteFeedChunk {
                        viewer_ids: chunk.to_vec(),
                        match_id: match_id.clone(),
                        old_starts_at: old_starts_at.clone(),
                    },
                    activity_opts(),
                )
                .await?;
            }
        }

        // 3. Ensure the match is searchable (idempotent; complements the inline
        //    indexing handler).
        ctx.start_activity(AgonActivities::index_match, match_id, activity_opts())
            .await?;

        Ok(())
    }
}

// ===========================================================================
// AcceptInvitation — the acceptance saga.
// ===========================================================================

/// Inputs to the accept-invitation saga.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptInvitationInput {
    pub invitation_id: String,
    pub accepting_user_id: String,
    pub responded_at: String,
    /// The match to re-fan-out, if accepting changed who should see it. None for
    /// team invitations (no feed impact).
    pub match_id: Option<String>,
}

/// Link an accepted invitation's roster entry and, for a match invite, re-run
/// fan-out (acceptance can change the audience). Workflow id: `accept-<inv_id>`.
#[workflow]
#[derive(Default)]
pub struct AcceptInvitation;

#[workflow_methods]
impl AcceptInvitation {
    #[run]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: AcceptInvitationInput,
    ) -> WorkflowResult<()> {
        // 1. Link the roster entry (external → user) and mark it accepted.
        ctx.start_activity(
            AgonActivities::link_accepted_invitation,
            LinkAccepted {
                invitation_id: input.invitation_id.clone(),
                accepting_user_id: input.accepting_user_id.clone(),
                responded_at: input.responded_at.clone(),
            },
            activity_opts(),
        )
        .await?;

        // 2. For a match invite, re-fan-out so the newly-linked participant (and
        //    their followers) pick the match up, and reconcile stats in case the
        //    match is already completed (accepting into a finished match must
        //    credit the player — the roster link alone doesn't re-trigger the
        //    stream-driven stats handler, which only fires on a `#META` write).
        if let Some(match_id) = input.match_id {
            let audience = ctx
                .start_activity(
                    AgonActivities::resolve_fanout_audience,
                    match_id.clone(),
                    activity_opts(),
                )
                .await?;

            if audience.match_exists {
                let now = workflow_now(ctx);
                for chunk in audience.viewers.chunks(FEED_CHUNK) {
                    ctx.start_activity(
                        AgonActivities::write_feed_chunk,
                        WriteFeedChunk {
                            viewers: chunk.to_vec(),
                            match_id: match_id.clone(),
                            starts_at: audience.starts_at.clone(),
                            now: now.clone(),
                        },
                        activity_opts(),
                    )
                    .await?;
                }

                // 3. Reconcile the newly-linked player's stat contribution
                //    (idempotent; a no-op unless the match is completed).
                ctx.start_activity(
                    AgonActivities::reconcile_match_stats,
                    match_id.clone(),
                    activity_opts(),
                )
                .await?;

                // 4. Refresh side roster previews — linking an invitee can
                //    flip a cached preview entry from external to a real user
                //    even when the side's composition doesn't change.
                ctx.start_activity(
                    AgonActivities::refresh_side_roster_previews,
                    match_id.clone(),
                    activity_opts(),
                )
                .await?;

                // 5. Reindex the match — the roster link (a `PLAYER#` write)
                //    doesn't touch `#META`, so the inline indexing stream
                //    handler never fires for it either. Without this, the
                //    accepter's newly-linked user id never lands in the search
                //    doc's `participant_ids`, so the match stays invisible to
                //    their `GET /matches?participant=<id>` activity queries
                //    (profile "recent activity", the sport stats chart/list)
                //    even though their lifetime stats were just reconciled.
                ctx.start_activity(AgonActivities::index_match, match_id, activity_opts())
                    .await?;
            }
        }

        Ok(())
    }
}

// ===========================================================================
// MaterializeSeries — generate a bounded series' remaining occurrences.
// ===========================================================================

/// Generate a bounded series' (`OnDate`/`AfterN`) remaining occurrences —
/// everything past the first, which the API already created synchronously
/// (see `CreateMatchSeriesInput`'s handler, `agon_service`). One-shot and
/// reactive: started once off the series-creation DynamoDB stream event,
/// runs to completion, and ends — the same shape as `FanOutMatch`/
/// `AcceptInvitation`, not a long-running loop (that's only needed for
/// indefinite, `Never`-ending series, a separate later piece of work).
/// Workflow id: `materialize-<series_id>`.
///
/// Resumable by construction rather than by any workflow-level
/// checkpointing: the loop below re-derives "how many occurrences are left"
/// from the series record's own `occurrences_generated` every run, so a
/// worker crash mid-run just means the next attempt (via `UseExisting`
/// re-attaching a redelivered stream event to this same workflow id, or a
/// Temporal-level retry of this run) picks up wherever the series record
/// says generation actually got to — never past it, and never from
/// scratch.
///
/// Indefinite (`Never`) series are out of scope for this workflow — series
/// creation rejects `Never` at the API input level (see
/// `CreateMatchSeriesEndInput`), so no `SeriesRecord` this workflow is ever
/// started for should carry it. The `Never` arm below is defensive-only:
/// reached only by a hand-written or future-schema record, where ending the
/// run without generating anything (leaving `status` at `"generating"`) is
/// safer than guessing at a policy this workflow was never built for.
#[workflow]
#[derive(Default)]
pub struct MaterializeSeries;

#[workflow_methods]
impl MaterializeSeries {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, series_id: String) -> WorkflowResult<()> {
        // 1. Load the series. Gone, or already past "generating" (a replay,
        //    or a redelivered start attaching to a run that already
        //    finished) → nothing to do.
        let Some(series) = ctx
            .start_activity(
                AgonActivities::get_series,
                series_id.clone(),
                activity_opts(),
            )
            .await?
        else {
            return Ok(());
        };
        if series.status != "generating" {
            return Ok(());
        }

        // 2. Anchor + total occurrence count — the same computation
        //    `agon_service`'s create handler validated at creation time,
        //    via the same `agon_core::series` functions, so the two can
        //    never disagree on what "the next occurrence" means.
        let Ok(first_starts_at) = series
            .first_occurrence_starts_at
            .parse::<chrono::DateTime<chrono::Utc>>()
        else {
            return Ok(());
        };
        let first_date = first_starts_at.date_naive();
        let Some(weekday) = agon_core::series::weekday_from_sunday_index(series.weekday) else {
            return Ok(());
        };
        let total = match &series.ends {
            agon_core::dao::records::SeriesEndRecord::AfterN { count } => *count,
            agon_core::dao::records::SeriesEndRecord::OnDate { date } => {
                match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
                    Ok(end_date) => agon_core::series::count_occurrences_through(
                        first_date,
                        &series.frequency,
                        weekday,
                        end_date,
                    ),
                    Err(_) => return Ok(()),
                }
            }
            // Defensive-only — see this workflow's doc comment.
            agon_core::dao::records::SeriesEndRecord::Never => return Ok(()),
        };

        // 3. Generate every occurrence from `occurrences_generated` (the
        //    next one actually needed) up to `total`, one activity call
        //    each — a checkpointed step per occurrence, same reasoning as
        //    `FanOutMatch`'s per-chunk feed writes.
        for index in series.occurrences_generated..total {
            let Some(date) =
                agon_core::series::occurrence_date(first_date, &series.frequency, weekday, index)
            else {
                break;
            };
            let starts_at = date
                .and_time(first_starts_at.time())
                .and_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            ctx.start_activity(
                AgonActivities::generate_series_occurrence,
                GenerateSeriesOccurrence {
                    series_id: series_id.clone(),
                    index,
                    starts_at,
                    created_by_user_id: series.created_by_user_id.clone(),
                    match_type: series.match_type.clone(),
                    name: series.name.clone(),
                    description: series.description.clone(),
                    location: series.location.clone(),
                    sides: series.sides.clone(),
                },
                activity_opts(),
            )
            .await?;
        }

        // 4. Done — flip the series from "generating" to "active".
        ctx.start_activity(AgonActivities::activate_series, series_id, activity_opts())
            .await?;

        Ok(())
    }
}
