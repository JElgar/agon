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
use temporalio_sdk::{ActivityOptions, ContinueAsNewOptions, WorkflowContext, WorkflowResult};

use super::activities::{
    AgonActivities, DeleteFeedChunk, GenerateSeriesOccurrence, LinkAccepted, WriteFeedChunk,
};

/// How long before a match's `starts_at` its reminder notification fires —
/// see `MatchReminder`.
const REMINDER_LEAD: Duration = Duration::from_secs(24 * 3600);

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

/// The workflow's deterministic clock as a `DateTime<Utc>` — the form
/// `MaterializeSeries`'s window/sleep math needs (see `workflow_now` above
/// for the string form `FanOutMatch`/`AcceptInvitation` use). Falls back to
/// the Unix epoch if the context has no time yet (shouldn't happen inside
/// `run`) — deterministic, unlike a wall-clock fallback would be.
fn workflow_now_utc(ctx: &WorkflowContext<impl Sized>) -> chrono::DateTime<chrono::Utc> {
    ctx.workflow_time()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
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
// MaterializeSeries — keep a series' rolling occurrence window topped up.
// ===========================================================================

/// How many not-yet-started occurrences `materialize_series` tries to keep
/// generated ahead of the series' current progress, for every series —
/// bounded or indefinite alike. A default pick, not tuned against any
/// particular cadence (a weekly series tops up every window-worth of weeks;
/// a yearly one plans multiple years ahead) — revisit if it turns out to be
/// wrong in either direction.
const MATERIALIZE_WINDOW: u32 = 4;

/// The longest `materialize_series` ever sleeps between cycles, even for a
/// monthly/yearly series whose next occurrence is genuinely a long way off,
/// and the flat poll interval used while a series is `"paused"`. Caps how
/// long a `"paused"`/`"ended"` transition (written by a separate pause/end
/// endpoint, not part of this workflow) can go unnoticed.
fn materialize_poll_cap() -> chrono::Duration {
    chrono::Duration::hours(24)
}

/// A `chrono::Duration`, known non-negative, as the `std::time::Duration`
/// `ctx.timer` needs. Falls back to zero on the (here, unreachable) negative
/// case rather than propagating an error for a timer duration.
fn to_std_duration(d: chrono::Duration) -> Duration {
    d.to_std().unwrap_or(Duration::ZERO)
}

/// What one `MaterializeSeries` execution does once it decides it's neither
/// gone, `"ended"`, nor `"paused"` — i.e. once it's actually generating.
/// Pure (no Temporal context), so the window/termination logic is directly
/// unit-tested (see `tests` below) the same way `agon_core::series` keeps
/// its date math pure and separate from the activity calls that use it.
#[derive(Debug, Clone, PartialEq)]
struct MaterializeCycle {
    /// 0-based indices to generate this cycle, in order — enough to top the
    /// rolling window back up to `MATERIALIZE_WINDOW` not-yet-started
    /// occurrences (fewer if a bounded series' `total` is reached first, or
    /// if date math can't produce any more). Empty if the window is already
    /// full.
    generate: Vec<u32>,
    /// What the workflow does once this cycle's generation (if any) lands.
    outcome: MaterializeOutcome,
}

#[derive(Debug, Clone, PartialEq)]
enum MaterializeOutcome {
    /// A bounded series has now generated every occurrence its `ends`
    /// implies (or date math can't go any further) — end the run for good;
    /// no further wakes are needed.
    Finish,
    /// Sleep this long, then `continue_as_new` into the next cycle.
    SleepThenContinue(chrono::Duration),
}

/// Decide what one `materialize_series` cycle should generate and what it
/// does next, from the series' current generation state and the wall-clock
/// time alone.
///
/// Because occurrences are generated in strictly increasing date order, the
/// already-generated ones that haven't started yet are always exactly a
/// suffix of `0..occurrences_generated` — so counting backward from the most
/// recently generated occurrence and stopping at the first one that's
/// already started is enough to find how many are still pending, without
/// examining every generated occurrence.
///
/// The window's oldest still-pending occurrence is what determines the next
/// wake time, and — this is the detail that makes the sleep duration
/// trivial to compute — its index never moves within one cycle's planning:
/// topping the window up with fresh occurrences only adds entries *ahead* of
/// it, so `occurrences_generated - not_yet_started` names it whether or not
/// this cycle generates anything at all.
fn plan_materialize_cycle(
    first_starts_at: chrono::DateTime<chrono::Utc>,
    timezone: chrono_tz::Tz,
    frequency: &str,
    weekday: chrono::Weekday,
    occurrences_generated: u32,
    total: Option<u32>,
    now: chrono::DateTime<chrono::Utc>,
) -> MaterializeCycle {
    let not_yet_started = (0..occurrences_generated)
        .rev()
        .take_while(|&index| {
            agon_core::series::occurrence_starts_at(
                first_starts_at,
                timezone,
                frequency,
                weekday,
                index,
            )
            .is_some_and(|starts_at| starts_at > now)
        })
        .count() as u32;

    // Top the window back up, one occurrence at a time, stopping early at
    // `total` for a bounded series (or if date math genuinely runs out).
    let mut generate = Vec::new();
    let mut next_index = occurrences_generated;
    let mut window = not_yet_started;
    while window < MATERIALIZE_WINDOW && total.is_none_or(|total| next_index < total) {
        if agon_core::series::occurrence_starts_at(
            first_starts_at,
            timezone,
            frequency,
            weekday,
            next_index,
        )
        .is_none()
        {
            break;
        }
        generate.push(next_index);
        next_index += 1;
        window += 1;
    }

    let generated_after = occurrences_generated + generate.len() as u32;
    let outcome = if total.is_some_and(|total| generated_after == total) || window == 0 {
        // Either the bounded total is now fully generated, or nothing is
        // pending and nothing more could be generated this cycle (date math
        // broke) — either way, no further wake is needed.
        MaterializeOutcome::Finish
    } else {
        let oldest_pending_index = occurrences_generated - not_yet_started;
        match agon_core::series::occurrence_starts_at(
            first_starts_at,
            timezone,
            frequency,
            weekday,
            oldest_pending_index,
        ) {
            Some(oldest_pending_starts_at) => {
                let sleep = (oldest_pending_starts_at - now)
                    .max(chrono::Duration::zero())
                    .min(materialize_poll_cap());
                MaterializeOutcome::SleepThenContinue(sleep)
            }
            // Unreachable in practice: `oldest_pending_index` is always
            // either an already-generated occurrence (whose date math
            // succeeded when it was generated, and depends on nothing but
            // pure inputs that haven't changed since) or this cycle's first
            // newly-generated one (whose date math this loop just checked
            // above) — but finishing defensively beats an infinite
            // zero-length sleep loop if that invariant is ever wrong.
            None => MaterializeOutcome::Finish,
        }
    };

    MaterializeCycle { generate, outcome }
}

/// Keep a series' rolling window of not-yet-started occurrences topped up —
/// `MATERIALIZE_WINDOW` ahead for as long as the series is `"generating"`/
/// `"active"`, pausing generation (but continuing to poll) while
/// `"paused"`, and stopping for good once `"ended"` (or, for a bounded
/// series, once every occurrence its `ends` implies has been generated).
/// Deliberately never generates a bounded series' occurrences all upfront
/// either — the same rolling window applies regardless of whether `ends` is
/// `OnDate`/`AfterN` or `Never`, so a 99-occurrence series and an indefinite
/// one are generated exactly the same way, just with the former eventually
/// finishing. Workflow id: `materialize-<series_id>` (unchanged from the
/// one-shot version this replaces — see docs/async-design.md §3 — so an
/// `UseExisting` start still attaches to whichever cycle is currently
/// running).
///
/// Long-running via `ctx.continue_as_new` on every cycle (not just
/// occasionally): the entire state this workflow needs is `series_id` —
/// everything else is re-read from the series record each time — so there's
/// no reason to let workflow history grow across cycles at all, unlike
/// `FanOutMatch`/`AcceptInvitation`, which are one-shot and never loop.
///
/// Resumable the same way the one-shot version was: every cycle re-derives
/// "how many occurrences are needed to top up the window" from the series
/// record's own `occurrences_generated`, so a worker crash mid-cycle just
/// means the next attempt (a Temporal-level retry of this run, or — since
/// `"paused"`/`"ended"` are driven by a separate endpoint, not by this
/// workflow — a later cycle after `continue_as_new`) picks up wherever
/// generation actually got to.
///
/// `"paused"`/`"ended"` aren't set by anything in this change — this
/// workflow only *reads* `status` and reacts to it; driving those
/// transitions (and, for `"ended"`, cancelling not-yet-started generated
/// occurrences) is a separate endpoint's job.
#[workflow]
#[derive(Default)]
pub struct MaterializeSeries;

#[workflow_methods]
impl MaterializeSeries {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, series_id: String) -> WorkflowResult<()> {
        // 1. Load the series. Gone, or "ended" → nothing more to do, ever.
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

        // 2. "Paused": generate nothing this cycle, but keep checking back
        //    (at the same bounded interval the active/generating path caps
        //    its own sleep at) so a later resume is noticed reasonably
        //    promptly. Simpler than a signal-driven wake, and the pause/end
        //    endpoint this reacts to doesn't exist in this change anyway.
        if series.status == "paused" {
            ctx.timer(to_std_duration(materialize_poll_cap())).await;
            ctx.continue_as_new(&series_id, ContinueAsNewOptions::default())?;
        }
        if series.status != "generating" && series.status != "active" {
            // "ended", or any unrecognised/future status — treat as
            // terminal rather than guessing at a policy this workflow
            // wasn't built for.
            return Ok(());
        }

        // 3. Anchor + bounded total (`None` for `Never`) — the same
        //    computation `agon_service`'s create handler validated at
        //    creation time, via the same `agon_core::series` functions, so
        //    the two can never disagree on what "the next occurrence"
        //    means.
        let Ok(first_starts_at) = series
            .first_occurrence_starts_at
            .parse::<chrono::DateTime<chrono::Utc>>()
        else {
            return Ok(());
        };
        let Some(weekday) = agon_core::series::weekday_from_sunday_index(series.weekday) else {
            return Ok(());
        };
        let Ok(timezone) = series.timezone.parse::<chrono_tz::Tz>() else {
            return Ok(());
        };
        let total = match &series.ends {
            agon_core::dao::records::SeriesEndRecord::AfterN { count } => Some(*count),
            agon_core::dao::records::SeriesEndRecord::OnDate { date } => {
                match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
                    Ok(end_date) => Some(agon_core::series::count_occurrences_through(
                        first_starts_at.with_timezone(&timezone).date_naive(),
                        &series.frequency,
                        weekday,
                        end_date,
                    )),
                    Err(_) => return Ok(()),
                }
            }
            agon_core::dao::records::SeriesEndRecord::Never => None,
        };

        // 4. Decide this cycle's work.
        let cycle = plan_materialize_cycle(
            first_starts_at,
            timezone,
            &series.frequency,
            weekday,
            series.occurrences_generated,
            total,
            workflow_now_utc(ctx),
        );

        // 5. Generate this cycle's occurrences — reuses
        //    `generate_series_occurrence`'s claim-item scheme exactly as
        //    before; nothing about *how* one occurrence is generated
        //    changes here, only how many are generated per cycle.
        for index in cycle.generate {
            let Some(starts_at) = agon_core::series::occurrence_starts_at(
                first_starts_at,
                timezone,
                &series.frequency,
                weekday,
                index,
            ) else {
                break;
            };

            ctx.start_activity(
                AgonActivities::generate_series_occurrence,
                GenerateSeriesOccurrence {
                    series_id: series_id.clone(),
                    index,
                    starts_at: starts_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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

        // 6. First time the window is topped up, flip "generating" ->
        //    "active" — same semantics as the one-shot version, just
        //    triggered after the first window instead of after everything.
        //    `update_series_status` is itself guarded on the series still
        //    reading "generating", so calling this every cycle for as long
        //    as that's true (never again once it's "active") is a harmless,
        //    idempotent no-op on every cycle after the first.
        if series.status == "generating" {
            ctx.start_activity(
                AgonActivities::activate_series,
                series_id.clone(),
                activity_opts(),
            )
            .await?;
        }

        match cycle.outcome {
            MaterializeOutcome::Finish => Ok(()),
            MaterializeOutcome::SleepThenContinue(sleep) => {
                ctx.timer(to_std_duration(sleep)).await;
                ctx.continue_as_new(&series_id, ContinueAsNewOptions::default())?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod materialize_tests {
    use super::*;

    fn dt(s: &str) -> chrono::DateTime<chrono::Utc> {
        s.parse().unwrap()
    }

    /// Fresh series, nothing generated past the synchronous first
    /// occurrence: the window should top up from 1 to `MATERIALIZE_WINDOW`,
    /// and the next wake should line up with the first occurrence's own
    /// start (the oldest, and at this point only originally-pending, one).
    #[test]
    fn tops_up_the_window_from_a_freshly_created_series() {
        let first = dt("2026-06-02T18:00:00Z"); // a Tuesday
        // Close enough to `first` that the sleep below isn't capped — the
        // point here is checking it lines up with `first`, not the cap
        // (`sleep_is_capped_even_for_a_distant_next_occurrence` covers that).
        let now = dt("2026-06-02T10:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "weekly",
            chrono::Weekday::Tue,
            1,
            None,
            now,
        );
        assert_eq!(cycle.generate, vec![1, 2, 3]);
        match cycle.outcome {
            MaterializeOutcome::SleepThenContinue(sleep) => {
                assert_eq!(first - now, sleep);
            }
            other => panic!("expected SleepThenContinue, got {other:?}"),
        }
    }

    /// Once the window is already full of not-yet-started occurrences,
    /// nothing new needs generating this cycle.
    #[test]
    fn generates_nothing_once_the_window_is_already_full() {
        let first = dt("2026-06-02T18:00:00Z");
        let now = dt("2026-05-01T00:00:00Z");
        // 4 generated (indices 0..=3), none of which have started yet.
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "weekly",
            chrono::Weekday::Tue,
            4,
            None,
            now,
        );
        assert_eq!(cycle.generate, Vec::<u32>::new());
    }

    /// Time has passed and some already-generated occurrences have started:
    /// the window should top back up to catch up, exactly as it would from
    /// a fresh series, regardless of *why* the window ran low.
    #[test]
    fn catches_up_when_time_has_passed_without_generation() {
        let first = dt("2026-06-02T18:00:00Z");
        // 4 generated (0..=3: June 2, 9, 16, 23); "now" is after the first
        // two have started (June 2, June 9), leaving only indices 2 and 3
        // still pending.
        let now = dt("2026-06-10T00:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "weekly",
            chrono::Weekday::Tue,
            4,
            None,
            now,
        );
        assert_eq!(cycle.generate, vec![4, 5]);
    }

    /// A bounded series stops generating (and the workflow finishes for
    /// good) once its implied total is reached, even if that's short of a
    /// full window.
    #[test]
    fn finishes_a_bounded_series_once_its_total_is_reached() {
        let first = dt("2026-06-02T18:00:00Z");
        let now = dt("2026-05-01T00:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "weekly",
            chrono::Weekday::Tue,
            1,
            Some(2),
            now,
        );
        assert_eq!(cycle.generate, vec![1]);
        assert_eq!(cycle.outcome, MaterializeOutcome::Finish);
    }

    /// A bounded series already fully generated (e.g. a replayed/redelivered
    /// start after the series finished) generates nothing and finishes.
    #[test]
    fn finishes_immediately_if_the_bounded_total_is_already_generated() {
        let first = dt("2026-06-02T18:00:00Z");
        let now = dt("2026-05-01T00:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "weekly",
            chrono::Weekday::Tue,
            2,
            Some(2),
            now,
        );
        assert_eq!(cycle.generate, Vec::<u32>::new());
        assert_eq!(cycle.outcome, MaterializeOutcome::Finish);
    }

    /// An indefinite (`Never`, `total: None`) series never finishes on its
    /// own — it always sleeps and continues, cycle after cycle.
    #[test]
    fn an_indefinite_series_never_finishes() {
        let first = dt("2026-06-02T18:00:00Z");
        let now = dt("2026-05-01T00:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "yearly",
            chrono::Weekday::Tue,
            1,
            None,
            now,
        );
        assert!(matches!(
            cycle.outcome,
            MaterializeOutcome::SleepThenContinue(_)
        ));
    }

    /// The sleep duration is capped, even for a far-future next occurrence
    /// (a yearly series' window can plan years ahead of "now").
    #[test]
    fn sleep_is_capped_even_for_a_distant_next_occurrence() {
        let first = dt("2026-06-02T18:00:00Z");
        let now = dt("2026-05-01T00:00:00Z");
        let cycle = plan_materialize_cycle(
            first,
            chrono_tz::UTC,
            "yearly",
            chrono::Weekday::Tue,
            1,
            None,
            now,
        );
        match cycle.outcome {
            MaterializeOutcome::SleepThenContinue(sleep) => {
                assert_eq!(sleep, materialize_poll_cap());
            }
            other => panic!("expected SleepThenContinue, got {other:?}"),
        }
    }

    /// `to_std_duration` round-trips an ordinary positive duration.
    #[test]
    fn to_std_duration_converts_an_ordinary_duration() {
        assert_eq!(
            to_std_duration(chrono::Duration::hours(2)),
            Duration::from_secs(2 * 60 * 60)
        );
    }
}

// ===========================================================================
// MatchReminder — a pre-kickoff heads-up for a match.
// ===========================================================================

/// Inputs to the reminder workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchReminderInput {
    pub match_id: String,
    /// RFC-3339; the match's `starts_at` as of the write that started this
    /// workflow. Not re-read if the match is rescheduled afterwards — a
    /// reschedule re-fans-out (see `FanOutMatch`) but doesn't restart this
    /// workflow, so a reminder can fire relative to a since-changed kickoff
    /// time. Acceptable for a first cut: rescheduling a match close enough
    /// to its reminder window to matter is rare, and the activity itself
    /// re-reads the match's *current* state before sending anything.
    pub starts_at: String,
}

/// Wait until `REMINDER_LEAD` before a match's kickoff, then send everyone
/// who might play in it a heads-up. Started off the same `#META` insert that
/// triggers `FanOutMatch` for any match created as part of a series (see
/// `workflow_for` in `agon_worker::consumer`) — a series occurrence has no
/// personal invite list of its own, so this is the only advance notice its
/// audience gets. Workflow id: `reminder-<match_id>`.
///
/// One-shot: waits out a single timer, sends one activity call, and ends —
/// no checkpointing needed beyond Temporal's own durable timer (a worker
/// crash mid-wait just resumes the same timer on replay).
#[workflow]
#[derive(Default)]
pub struct MatchReminder;

#[workflow_methods]
impl MatchReminder {
    #[run]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: MatchReminderInput,
    ) -> WorkflowResult<()> {
        let Ok(starts_at) = input.starts_at.parse::<chrono::DateTime<chrono::Utc>>() else {
            return Ok(());
        };
        let now = ctx
            .workflow_time()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .unwrap_or_else(chrono::Utc::now);
        // The match has already started (or is about to) by the time this
        // workflow got to run — too late for a "coming up" reminder to make
        // sense.
        if now >= starts_at {
            return Ok(());
        }

        let fire_at = starts_at - chrono::Duration::from_std(REMINDER_LEAD).unwrap();
        if fire_at > now
            && let Ok(wait) = (fire_at - now).to_std()
        {
            ctx.timer(wait).await;
        }

        ctx.start_activity(
            AgonActivities::send_match_reminder,
            input.match_id,
            activity_opts(),
        )
        .await?;

        Ok(())
    }
}
