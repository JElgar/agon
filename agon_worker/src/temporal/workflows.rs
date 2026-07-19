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

use super::activities::{AgonActivities, LinkAccepted, WriteFeedChunk};

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

/// Fan a match into the feeds of everyone who should see it. Started when a
/// match is created / completed. Workflow id: `fanout-<match_id>`.
#[workflow]
#[derive(Default)]
pub struct FanOutMatch;

#[workflow_methods]
impl FanOutMatch {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, match_id: String) -> WorkflowResult<()> {
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
        for chunk in audience.viewer_ids.chunks(FEED_CHUNK) {
            ctx.start_activity(
                AgonActivities::write_feed_chunk,
                WriteFeedChunk {
                    viewer_ids: chunk.to_vec(),
                    match_id: match_id.clone(),
                    starts_at: audience.starts_at.clone(),
                    now: now.clone(),
                },
                activity_opts(),
            )
            .await?;
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
                for chunk in audience.viewer_ids.chunks(FEED_CHUNK) {
                    ctx.start_activity(
                        AgonActivities::write_feed_chunk,
                        WriteFeedChunk {
                            viewer_ids: chunk.to_vec(),
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
            }
        }

        Ok(())
    }
}
