//! Temporal workflows — deterministic orchestration of the multi-step async
//! work (feed fan-out, the accept-invitation saga, rating repair). Workflows
//! call activities; they never touch DynamoDB / the network directly.
//!
//! Built against the Temporal Rust SDK (crates.io 0.5) — the workflow/activity
//! macros, `WorkflowContext::start_activity` and `workflow_time`, and the
//! unit-struct + `#[run(ctx, input)]` shape all match the SDK's own examples.
//!
//! Idempotency / determinism:
//! - Workflow ids are deterministic (`fanout-<match_id>`, `accept-<inv_id>`,
//!   `repair-<kind>-<owner>-<ladder>`) and started with `UseExisting`, so a
//!   duplicate start attaches to the running run (see docs/async-design.md §3).
//! - Every activity's effects are idempotent (feed writes keyed by match id,
//!   link is a fixed-point update), so activity retries are safe.
//! - Timestamps come from `ctx.workflow_time()` (deterministic on replay), never
//!   the wall clock.

use std::time::Duration;

use agon_core::dao::records::RatingOwnerKindRecord;
use serde::{Deserialize, Serialize};
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ActivityOptions, ApplicationFailure, WorkflowContext, WorkflowResult};

use super::activities::{
    AgonActivities, FinishRatingReplay, LinkAccepted, ReplayRatingChunk, WriteFeedChunk,
};
use super::repair_workflow_id;
use crate::handlers::rating::ReplayState;

/// How many feed rows to write per activity invocation. Each chunk is a
/// separately-retryable, checkpointed step — the whole point of running fan-out
/// on Temporal (a mid-way failure resumes, not restarts).
const FEED_CHUNK: usize = 500;

/// Default activity timeout. Fan-out chunks and single DAO ops are all quick.
fn activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(30))
}

/// Longer timeout for a rating-replay chunk, which is the one activity here
/// that is read-bound rather than write-bound: it costs a match read and a
/// contribution query *per replayed match*, so a full page is a couple of
/// hundred sequential round trips rather than one batched write.
fn replay_activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(120))
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
// RepairRatings — replay one owner's ladder back into line.
// ===========================================================================

/// How many chunks a single repair may run before it gives up.
///
/// A cursor-driven loop is the one shape that can grow a workflow's event
/// history without bound, so it gets a ceiling rather than trust. At
/// `REPLAY_PAGE` (50) matches per chunk this is 20,000 rated matches on one
/// ladder for one account — unreachable for real data, and if it is ever
/// reached the honest conclusion is that the cursor is not advancing.
const MAX_REPLAY_CHUNKS: usize = 400;

/// Which ladder, for which owner, needs replaying.
///
/// Owner-generic on purpose: a team carries ratings of its own and every DAO
/// op behind this takes a `RatingOwner`, so phase 2b-iii inherits this
/// workflow by filling in one match arm in `handlers::rating::rateable_for`
/// and changing nothing here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairRatingsInput {
    pub owner_kind: RatingOwnerKindRecord,
    pub owner_id: String,
    pub ladder: String,
}

impl RepairRatingsInput {
    /// The deterministic workflow id for this repair.
    #[must_use]
    pub fn workflow_id(&self) -> String {
        let kind = match self.owner_kind {
            RatingOwnerKindRecord::User => "user",
            RatingOwnerKindRecord::Team => "team",
        };
        repair_workflow_id(kind, &self.owner_id, &self.ladder)
    }
}

/// Replay one account's (or team's) rating on one ladder from its stored
/// history, correcting it. Started by `handlers::rating::note_repair_needed`
/// when a result arrives out of played order, an already-rated match changes,
/// or one stops counting. Workflow id: `repair-<kind>-<ownerId>-<ladder>`.
///
/// ## Why a replay at all
///
/// A Weng-Lin update is not invertible: what a match did to you depends on
/// every opponent's σ at that moment, so "subtract this match" has no closed
/// form. The only way to correct a rating is to compute it again from the
/// results in the order they were played, which is exactly what the
/// `RATING#<ladder>#<played_at>#<matchId>` item collection is — the replay
/// source and the rating-over-time chart from a single write.
///
/// ## Repair is first-order, and that is a decision, not an omission
///
/// The replay walks *this* owner's matches and moves *this* owner's belief.
/// Every other participant in each match is held at the belief their own
/// contribution says they carried into it. So when A's rating is corrected, B
/// — who played A — has technically received a slightly wrong movement from
/// that match too, and C who played B after that, and so on. This workflow
/// does not chase them.
///
/// It could not, cheaply: the closure of "everyone affected by correcting A"
/// is the connected component of the who-played-whom graph from the disturbed
/// point forward, so every correction would recompute the ladder. There is not
/// even an index for it — history lives in owner partitions, so there is no
/// way to enumerate a ladder's matches in played order at all. And the error
/// being left behind is damped by σ at each hop: the second-order term is a
/// fraction of a movement that was already the smaller of the two (B's
/// uncertainty about A barely moved). First-order repair is the trade, taken
/// deliberately.
///
/// **Nothing downstream may assume ratings are globally consistent.** They are
/// consistent with each owner's own recorded history, which is a weaker and
/// true statement.
///
/// ## Replays the whole ladder, not the tail
///
/// The plan called for resuming from the affected match. It does not, for a
/// reason that only shows up against real history: the state a tail replay
/// would have to resume *from* is the belief the owner carried into that
/// match, and in the out-of-order case that stored value is precisely the one
/// that is wrong (the match was rated after a later one, so its `mu_before`
/// already contains the later result). Getting it right needs a descending
/// history query and a separate count for `matches_rated`, both of which phase
/// 2a deliberately left to phase 3.
///
/// Replaying from the beginning also makes two other things true, and they are
/// worth more than the reads saved. Every run of this workflow does the
/// identical, total job, so `UseExisting` is a sound deduplication rather than
/// a lossy one; and a repair that was missed is picked up by the *next* one,
/// so the state is self-healing rather than accumulating skew. A replay over
/// an unchanged history writes nothing (the engine is deterministic, so it
/// reproduces the stored contributions byte for byte), so the cost of the
/// extra generality is reads.
///
/// ## The race that is left
///
/// `UseExisting` means a trigger arriving while a run is in flight attaches to
/// that run — which may already have walked past the match that just changed.
/// That repair is then skipped, and the ladder stays first-order-wrong until
/// the owner's next repair, which (replaying everything) will fix it. Closing
/// the window properly needs `signal_with_start` and a drain loop, or a
/// workflow id that includes the trigger; both were left out of this phase
/// deliberately, and neither is a silent problem — it is bounded staleness in
/// a number that is already documented as approximate.
///
/// `handlers::rating::note_repair_needed` documents the other end of the same
/// trade: an out-of-order repair cannot be started until the match it is about
/// has been written, so a start that fails there is not re-detected on
/// redelivery.
#[workflow]
#[derive(Default)]
pub struct RepairRatings;

#[workflow_methods]
impl RepairRatings {
    #[run]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: RepairRatingsInput,
    ) -> WorkflowResult<()> {
        // One `applied_at` for the whole repair, from the workflow's own
        // deterministic clock — never `Utc::now()`, which would differ on
        // replay and make every re-run look like a re-score.
        let now = workflow_now(ctx);

        let mut state = ReplayState::default();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_REPLAY_CHUNKS {
            let progress = ctx
                .start_activity(
                    AgonActivities::replay_rating_chunk,
                    ReplayRatingChunk {
                        owner_kind: input.owner_kind,
                        owner_id: input.owner_id.clone(),
                        ladder: input.ladder.clone(),
                        cursor: cursor.clone(),
                        state: state.clone(),
                        now: now.clone(),
                    },
                    replay_activity_opts(),
                )
                .await?;
            state = progress.state;

            let Some(next) = progress.next_cursor else {
                // The history is exhausted. Settle the rating: the last chunk
                // does not necessarily leave the right value behind (an
                // unchanged tail writes nothing, and an emptied history writes
                // nothing at all yet still has to reset the account).
                ctx.start_activity(
                    AgonActivities::finish_rating_replay,
                    FinishRatingReplay {
                        owner_kind: input.owner_kind,
                        owner_id: input.owner_id,
                        ladder: input.ladder,
                        state,
                    },
                    activity_opts(),
                )
                .await?;
                return Ok(());
            };
            cursor = Some(next);
        }

        // Non-retryable: retrying cannot make a non-advancing cursor advance,
        // and the half-replayed rating left behind is corrected by the next
        // repair (every run replays the whole ladder).
        Err(ApplicationFailure::non_retryable(format!(
            "rating repair for {} on {} exceeded {MAX_REPLAY_CHUNKS} chunks",
            input.owner_id, input.ladder
        ))
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user and a team can hold the same id string, and their ratings are
    /// different pools that must never be conflated (see the plan's Part 2.7 —
    /// a team's `μ` and a player's `μ` are not comparable numbers). If the two
    /// produced the same workflow id, one owner's repair would attach to the
    /// other's run under `UseExisting` and silently do nothing.
    #[test]
    fn a_user_and_a_team_with_the_same_id_get_different_repair_workflows() {
        let user = RepairRatingsInput {
            owner_kind: RatingOwnerKindRecord::User,
            owner_id: "abc".into(),
            ladder: "squash".into(),
        };
        let team = RepairRatingsInput {
            owner_kind: RatingOwnerKindRecord::Team,
            ..user.clone()
        };
        assert_eq!(user.workflow_id(), "repair-user-abc-squash");
        assert_ne!(user.workflow_id(), team.workflow_id());
    }

    /// The id is the deduplication key, so two ladders for the same owner have
    /// to be two workflows — a squash repair attaching to a running tennis one
    /// would leave squash wrong and look like it had been handled. Sub-ladders
    /// (`tennis:doubles`, Part 2.5's forward-compatible ladder key) are part
    /// of the same guarantee, which is why `rating::Ladder` separates them with
    /// `:` and never `-`.
    #[test]
    fn each_ladder_repairs_under_its_own_workflow_id() {
        let squash = RepairRatingsInput {
            owner_kind: RatingOwnerKindRecord::User,
            owner_id: "u1".into(),
            ladder: "tennis".into(),
        };
        let doubles = RepairRatingsInput {
            ladder: "tennis:doubles".into(),
            ..squash.clone()
        };
        assert_ne!(squash.workflow_id(), doubles.workflow_id());
    }
}
