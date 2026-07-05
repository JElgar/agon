//! Temporal workflows — deterministic orchestration of the multi-step async
//! work (feed fan-out, the accept-invitation saga). Workflows call activities;
//! they never touch DynamoDB / the network directly.
//!
//! ⚠️ UNVERIFIED: written against the Temporal Rust SDK **Public Preview** API
//! and cannot be compiled here (churning git dependency, no crates.io release,
//! no Temporal server). Signatures likely need adjustment against your pinned
//! SDK revision. Gated behind the `temporal` cargo feature.
//!
//! Idempotency / determinism:
//! - Workflow ids are deterministic (`fanout-<match_id>`, `accept-<inv_id>`), so
//!   a duplicate start attaches to the running/completed run rather than
//!   double-processing (see docs/async-design.md §3).
//! - Every activity's effects are idempotent (feed writes keyed by match id,
//!   link is a fixed-point update), so activity retries are safe.
//! - Timestamps come from the workflow context (`ctx.now()`), never the wall
//!   clock, to keep replay deterministic.

use std::time::Duration;

use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ActivityOptions, WorkflowContext, WorkflowContextView, WorkflowResult};

use super::activities::{AgonActivities, LinkAccepted, WriteFeedChunk};

/// How many feed rows to write per activity invocation. Each chunk is a
/// separately-retryable, checkpointed step — the whole point of running fan-out
/// on Temporal (a mid-way failure resumes, not restarts).
const FEED_CHUNK: usize = 500;

/// Default activity timeout. Fan-out chunks and single DAO ops are all quick.
fn activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(30))
}

// ===========================================================================
// FanOutMatch — fan a match into its audience's feeds.
// ===========================================================================

/// Fan a match into the feeds of everyone who should see it. Started when a
/// match is created / completed. Workflow id: `fanout-<match_id>`.
#[workflow]
pub struct FanOutMatch {
    match_id: String,
}

#[workflow_methods]
impl FanOutMatch {
    #[init]
    fn new(_ctx: &WorkflowContextView, match_id: String) -> Self {
        Self { match_id }
    }

    #[run]
    async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<()> {
        let match_id = ctx.state(|s| s.match_id.clone());

        // 1. Resolve the audience (followers of participants + involved teams +
        //    participants themselves) and the match start time.
        let audience = ctx
            .start_activity(
                AgonActivities::resolve_fanout_audience,
                match_id.clone(),
                activity_opts(),
            )?
            .await?;

        if !audience.match_exists {
            // Match gone before fan-out ran; nothing to do.
            return Ok(());
        }

        // 2. Write feed entries in checkpointed chunks. Each chunk is its own
        //    activity, so a failure resumes at the failed chunk on replay.
        let now = ctx.now().to_rfc3339();
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
            )?
            .await?;
        }

        // 3. Ensure the match is searchable (idempotent; complements the inline
        //    indexing handler).
        ctx.start_activity(AgonActivities::index_match, match_id, activity_opts())?
            .await?;

        Ok(())
    }
}

// ===========================================================================
// AcceptInvitation — the acceptance saga.
// ===========================================================================

/// Inputs to the accept-invitation saga.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
pub struct AcceptInvitation {
    input: AcceptInvitationInput,
}

#[workflow_methods]
impl AcceptInvitation {
    #[init]
    fn new(_ctx: &WorkflowContextView, input: AcceptInvitationInput) -> Self {
        Self { input }
    }

    #[run]
    async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<()> {
        let input = ctx.state(|s| s.input.clone());

        // 1. Link the roster entry (external → user) and mark it accepted.
        ctx.start_activity(
            AgonActivities::link_accepted_invitation,
            LinkAccepted {
                invitation_id: input.invitation_id.clone(),
                accepting_user_id: input.accepting_user_id.clone(),
                responded_at: input.responded_at.clone(),
            },
            activity_opts(),
        )?
        .await?;

        // 2. For a match invite, re-fan-out so the newly-linked participant (and
        //    their followers) pick the match up. Run inline as activities rather
        //    than a child workflow to keep this saga self-contained.
        if let Some(match_id) = input.match_id {
            let audience = ctx
                .start_activity(
                    AgonActivities::resolve_fanout_audience,
                    match_id.clone(),
                    activity_opts(),
                )?
                .await?;

            if audience.match_exists {
                let now = ctx.now().to_rfc3339();
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
                    )?
                    .await?;
                }
            }
        }

        Ok(())
    }
}
