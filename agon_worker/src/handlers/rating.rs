//! Inline handler: fold a confirmed, ranked match into every participant's
//! per-ladder rating.
//!
//! Hooks the same trigger as [`crate::handlers::stats`] — any non-remove write
//! to a match's `#META`, which is what promoting a score to `confirmed_score`
//! produces — and has the same reconcile-by-diff shape: it computes what the
//! match *currently* implies for each participant and compares that against
//! the stored `RATINGCONTRIB#` items before writing anything. An unchanged
//! redelivery therefore writes nothing, which matters more here than it does
//! for stats: a like or a comment rewrites `#META`, so redelivery of a
//! finished match is the common case, not the rare one.
//!
//! ## Why this is not `reconcile_match_stats` with different arithmetic
//!
//! Stats are a *sum*, so a stale contribution can be backed out by
//! subtracting it. A Weng-Lin update is not invertible — what a match did to
//! you depends on every opponent's σ at that moment — so the only way to
//! correct history is to replay it. That single fact shapes everything below:
//!
//! - **A first rating is applied here, incrementally and authoritatively.**
//!   Per participant, not per match: a roster edit can add somebody to a match
//!   the rest of the roster was already rated for, and that person's first
//!   rating is applied here like anyone else's. Repair cannot substitute for
//!   it — it replays *stored history*, so it can only ever correct a
//!   contribution that already exists.
//! - **A change to an already-rated participant's movement is only *detected*
//!   here.** Re-score, reschedule, sport edit, a roster change that moved
//!   everyone's side strengths, cancellation, ranked→friendly: all of them
//!   need a replay, which is `RepairRatings`' job. This handler classifies and
//!   hands off; see [`note_repair_needed`], the one place that workflow is
//!   started from. One delivery routinely does both — see
//!   [`reconcile_match_rating`]'s pass one.
//! - **The replay itself also lives in this module**
//!   ([`replay_rating_chunk`]), not in `temporal/`, because it is the same
//!   eligibility gate and the same projection the incremental path uses —
//!   sharing them is the only reason the two can be expected to agree. The
//!   activity in `temporal/activities.rs` is a thin wrapper, exactly as
//!   `reconcile_match_stats`' is.
//! - **"Has anything changed?" is answered from the stored contributions, not
//!   from the participants' current ratings.** `dao::rating`'s module header
//!   spells out the double-counting bug the obvious version walks into; the
//!   short version is that re-rating match A against a rating that has since
//!   absorbed match B produces a different movement, which looks like a
//!   change and gets applied a second time. Every rating this handler computes
//!   for an already-rated match therefore starts from the `mu_before`/
//!   `sigma_before` stored on that match's own contributions.
//!
//! ## Ordering
//!
//! Matches are rated in `starts_at` order, not confirmation order — a Monday
//! game confirmed on Thursday belongs before Wednesday's. Confirmation order
//! is what the stream hands us, so out-of-order arrival is normal rather than
//! exceptional, and detecting it is [`RepairTrigger::OutOfOrder`]. The result
//! is still applied when that happens; see that variant's doc comment for why
//! refusing to would be worse.

use std::collections::{BTreeMap, BTreeSet};

use agon_core::dao::Dao;
use agon_core::dao::error::{DaoError, DaoResult};
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::match_ops::MatchAggregate;
use agon_core::dao::page::Page;
use agon_core::dao::rating::RatingOwner;
use agon_core::dao::records::{
    MatchPlayerRecord, MatchRecord, RatingContributionRecord, RatingHistoryRecord,
    RatingMovementRecord, RatingOwnerKindRecord, RatingRecord,
};
use agon_core::rating::{
    INITIAL_MU, INITIAL_SIGMA, Ladder, MatchParticipant, PlayerRating, RatingTable, RatingUpdate,
    ladder_for_tag, rate_match,
};
use serde::{Deserialize, Serialize};

use crate::error::{WorkerError, WorkerResult};
use crate::event::ChangeEvent;
use crate::temporal::client::TemporalClient;
use crate::temporal::workflows::RepairRatingsInput;

/// Handle a rating-relevant change event: any non-remove write to a match's
/// `#META`. Everything else is ignored.
///
/// `temporal` is the client repairs are started on. `None` (unit tests, a local
/// run without the `full` compose profile) degrades to detection-only, exactly
/// as `push` does when FCM is unconfigured — the condition is still logged.
pub async fn handle(
    dao: &Dao,
    temporal: Option<&TemporalClient>,
    ev: &ChangeEvent,
    now: &str,
) -> WorkerResult<()> {
    if ev.kind.is_remove() {
        return Ok(());
    }
    let (Pk::Match(match_id), Sk::Meta) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };
    reconcile_match_rating(dao, temporal, match_id, now).await
}

/// Reconcile a match's rating contributions against its current state.
///
/// Idempotent under at-least-once redelivery, and that is a property of the
/// comparison rather than of a lock: an unchanged match reproduces exactly the
/// contributions already stored (bit-for-bit — the engine is deterministic and
/// starts from the same stored `before` ratings), so the "nothing changed"
/// branch is reached without writing.
pub(crate) async fn reconcile_match_rating<S: RatingStore>(
    store: &S,
    temporal: Option<&TemporalClient>,
    match_id: &str,
    now: &str,
) -> WorkerResult<()> {
    // Re-read the current aggregate rather than trusting the stream image, for
    // the same reason `reconcile_match_stats` does: the image is a snapshot of
    // one item, and eligibility depends on the roster items too. A missing
    // match means there is nothing to attribute.
    let Some(agg) = store.get_match(match_id).await? else {
        return Ok(());
    };
    let stored = store.list_rating_contributions(match_id).await?;

    let Some(rateable) = rateable(&agg.match_, &agg.players) else {
        // Not rateable. If it never was, there is nothing to do. If it *was*
        // — cancelled, flipped to friendly, a player unlinked — the stored
        // contributions and history entries now describe a match that no
        // longer counts, so they come out. The owners' stored ratings still
        // contain this match's effect until a replay removes it; that is
        // `withdraw_rating_contribution`'s documented shape, and the reason
        // withdrawal happens here rather than waiting for the repair is that
        // the history items are the replay's *input*. Leaving them would make
        // a later replay faithfully re-apply a match that no longer counts.
        for contribution in &stored {
            let owner = RatingOwner::from(contribution);
            // Repair first, withdrawal second, and the order is load-bearing.
            // Starting after the delete would lose the repair entirely if the
            // workflow start failed: the message redelivers, finds the
            // contributions already gone, and reports nothing to repair — so
            // the owner's rating would keep this match's effect forever.
            // Starting first costs nothing, because the replay re-runs the
            // eligibility gate against the *match*, not against the history
            // item: a run that races ahead of the delete skips this match all
            // the same and still lands on the right number.
            note_repair_needed(
                temporal,
                RepairTrigger::Withdrawn,
                match_id,
                &owner,
                &contribution.ladder,
            )
            .await?;
            store
                .withdraw_rating_contribution(match_id, contribution)
                .await?;
        }
        return Ok(());
    };

    let stored_by_owner: BTreeMap<&str, &RatingContributionRecord> =
        stored.iter().map(|c| (c.owner_id.as_str(), c)).collect();

    // The base each participant is rated *from*. For anyone this match has
    // already been applied to, that is the rating they carried into it, which
    // the contribution preserves precisely so a redelivery recomputes the same
    // movement instead of compounding one. For anyone else it is their current
    // rating (default for a first-ever match on this ladder).
    let need_current: Vec<String> = rateable
        .participants
        .iter()
        .filter(|p| !stored_by_owner.contains_key(p.competitor_id.as_str()))
        .map(|p| p.competitor_id.clone())
        .collect();
    let current = store
        .batch_get_ratings(&need_current, rateable.ladder.as_str())
        .await?;
    let current_rating = |user_id: &str| -> Option<RatingRecord> { current.get(user_id).cloned() };

    let mut base: RatingTable = RatingTable::new();
    for participant in &rateable.participants {
        let belief = match stored_by_owner.get(participant.competitor_id.as_str()) {
            Some(contribution) => PlayerRating {
                mu: contribution.movement.mu_before,
                sigma: contribution.movement.sigma_before,
            },
            None => current_rating(&participant.competitor_id)
                .map(|r| PlayerRating {
                    mu: r.mu,
                    sigma: r.sigma,
                })
                .unwrap_or_default(),
        };
        base.insert(participant.competitor_id.clone(), belief);
    }

    // Any remaining error out of the engine is a state the write path is
    // supposed to make impossible — `rateable` has already excluded every
    // shape that is merely unusual data (a side with nobody rateable on it, a
    // duplicated account). What is left is corruption, e.g. a
    // `winner_side_id` naming a side the match does not have, and the engine's
    // own doc comment is emphatic that such a match must fail loudly rather
    // than rate wrongly: silently scoring it would record it as a draw.
    let updates = rate_match(
        &rateable.participants,
        rateable.winner_side_id.as_deref(),
        &base,
    )
    .map_err(|e| WorkerError::Invariant(format!("match {match_id} cannot be rated: {e}")))?;

    // Pass one: classify every participant, writing nothing. A match whose
    // effect has *changed* under an existing rating needs a replay of that
    // ladder, not an incremental apply, and that decision has to be made
    // across the whole match before any of it is written — half-applying a
    // re-score would leave the two sides' ratings disagreeing about what
    // happened.
    //
    // The two outcomes are not exclusive and one delivery can carry both. Add
    // a player to a completed rated match and the roster edit rewrites
    // `#META` (via `refresh_side_roster_previews`): the players already there
    // have moved — a side gained strength — so they are `rerated`, while the
    // new player has no contribution at all and is `pending`. Both are acted
    // on below, which is a fix rather than a refinement: an earlier version
    // returned as soon as anything was `rerated`, and since `RepairRatings`
    // replays *stored history* it can only ever correct a contribution that
    // exists. A participant who never had one was stranded — rated only if
    // some later, unrelated `#META` write happened to re-run this handler.
    let participant_ids: BTreeSet<&str> = rateable
        .participants
        .iter()
        .map(|p| p.competitor_id.as_str())
        .collect();
    let mut pending: Vec<(&RatingUpdate, RatingContributionRecord)> = Vec::new();
    let mut rerated: Vec<(RatingOwner, String)> = Vec::new();
    for update in &updates {
        let contribution = contribution_for(update, &rateable, RatingOwnerKindRecord::User, now);
        match stored_by_owner.get(update.competitor_id.as_str()) {
            Some(previous) if previous.has_same_effect_as(&contribution) => {}
            // The match itself changed under an existing rating — a re-score,
            // a reschedule, a sport edit, or a roster change that moved
            // everyone's side strengths. Note the *old* ladder: that is the
            // pool holding the movement a replay has to undo.
            Some(previous) => rerated.push((RatingOwner::from(*previous), previous.ladder.clone())),
            None => pending.push((update, contribution)),
        }
    }
    // A stored contribution for somebody who is no longer a participant: the
    // roster shrank (or a player was unlinked) under a rated match. Same
    // conclusion, reached from the other direction.
    for contribution in &stored {
        if !participant_ids.contains(contribution.owner_id.as_str()) {
            rerated.push((RatingOwner::from(contribution), contribution.ladder.clone()));
        }
    }

    // Pass two: apply the participants this match has not yet been applied to.
    // Usually that is all of them (a first confirmation); it is a strict
    // subset when an earlier delivery got part-way through — each participant
    // is one transaction, so a crash or a lost optimistic-lock race in the
    // middle is recoverable rather than corrupting, and this is where it
    // recovers — or when a roster edit added somebody to an already-rated
    // match, where the rest of the roster is `rerated` alongside.
    //
    // Nothing here touches a `rerated` participant's stored contribution.
    // Those stay exactly as they are until the replay runs, because they are
    // the replay's input; the invariant is "do not rewrite a contribution
    // incrementally", not "do not write at all".
    let mut out_of_order: Vec<RatingOwner> = Vec::new();
    for (update, contribution) in pending {
        let stored_rating = current_rating(&update.competitor_id);
        let owner = RatingOwner::user(&update.competitor_id);

        // Out-of-order arrival: this match was played *before* the newest one
        // already folded into this rating, so the incremental result is not
        // the one a `starts_at`-ordered replay would produce. Collected, not
        // started — see below.
        if let Some(previous) = &stored_rating
            && previous.last_rated_at > rateable.played_at
        {
            out_of_order.push(owner.clone());
        }

        let rating = RatingRecord {
            mu: update.after.mu,
            sigma: update.after.sigma,
            matches_rated: stored_rating.as_ref().map_or(0, |r| r.matches_rated) + 1,
            // The `starts_at` of the newest match folded in — a *maximum*, not
            // the last one written. Rating a Monday game after a Wednesday one
            // must not move this backwards, or every subsequent match would
            // look like it arrived out of order too.
            last_rated_at: stored_rating
                .as_ref()
                .map(|r| r.last_rated_at.clone())
                .filter(|previous| previous > &rateable.played_at)
                .unwrap_or_else(|| rateable.played_at.clone()),
        };

        // `stored = None`: we only reach here for participants with no stored
        // contribution, so the write is guarded on there being none — two
        // concurrent deliveries of the same match cannot both apply it.
        store
            .apply_rating_contribution(
                match_id,
                &contribution,
                None,
                &rating,
                stored_rating.as_ref(),
            )
            .await?;
    }

    // Both kinds of repair are started only once every apply above has
    // committed, and that ordering is the opposite of the withdrawal branch's
    // on purpose — do not "fix" it to match. The replay reads the history
    // collection as it stands when it runs, and the history entries this match
    // is about are written by the loop that just finished. Starting inside the
    // loop would routinely hand the workflow a history that does not yet
    // contain the very match it was started for: it would replay, find nothing
    // wrong, settle, and exit — and nothing would start it again. For the
    // roster-edit case the same ordering earns something extra: the replay
    // finds the added participant's contribution already stored, so it rates
    // the match from the belief that records rather than falling back to their
    // *current* rating, which `replay_match` documents as an approximation.
    //
    // The cost of waiting is a window nothing recovers: if the start fails
    // (Temporal unreachable) or an earlier apply in the loop errors, the
    // message redelivers, finds every contribution already written and
    // unchanged, and reports nothing to repair. That is a real hole, and it is
    // the better half of the trade — losing an out-of-order repair leaves the
    // incrementally-applied numbers, which are self-consistent and are exactly
    // what phase 2b-i shipped, whereas racing the workflow would make the
    // repair silently no-op most of the time. The withdrawal branch does not
    // face the choice: its replay decides from the *match*, which is already
    // committed, so starting first there costs nothing.
    for owner in &out_of_order {
        note_repair_needed(
            temporal,
            RepairTrigger::OutOfOrder,
            match_id,
            owner,
            rateable.ladder.as_str(),
        )
        .await?;
    }
    // Every affected owner is reported, not just the first: a re-score moves
    // both sides, and `RepairRatings` is keyed per owner and ladder, so half
    // the list would leave half the ladders wrong.
    for (owner, ladder) in &rerated {
        note_repair_needed(temporal, RepairTrigger::Rerated, match_id, owner, ladder).await?;
    }

    Ok(())
}

/// A match that passed the eligibility gate, projected into what the engine
/// needs.
///
/// Built only by [`rateable`], and its existence is the postcondition: if you
/// hold one, the match is ranked, completed, agreed by every side, played
/// entirely by linked accounts, on a real ladder, and shaped so that
/// `rating::rate_match` cannot fail on it for any reason short of corrupt
/// data.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rateable {
    ladder: Ladder,
    /// The match's `starts_at` — the order ratings are supposed to be applied
    /// in, and the sort segment of every history item written from it.
    played_at: String,
    winner_side_id: Option<String>,
    participants: Vec<MatchParticipant>,
}

/// The `MatchRecord` + `PLAYER#` items → participants adapter, and the
/// eligibility gate that produces it. `None` means "this match does not count
/// towards anybody's rating", for any of the reasons below.
///
/// This is deliberately one function rather than a gate and a separate
/// projection: nearly every rule here is a statement about the roster, and
/// splitting them would mean walking the players twice under two subtly
/// different definitions of "on this side". The engine's module doc explains
/// why it takes loose participants rather than records — the short version is
/// that an 11-a-side roster only exists as separate `MATCH#/PLAYER#` items, so
/// there is no single record to hand it.
///
/// The rules, and what each is protecting against:
///
/// - **`ranked`.** A friendly is a game people agreed not to be judged on.
///   Note the stored default is `false`, so the entire pre-rating back
///   catalogue lands here (see `MatchRecord::ranked`).
/// - **`status == "completed"`.** A cancelled or still-scheduled match has no
///   result to rate.
/// - **A `confirmed_score`.** Presence *is* the "every side confirmed" test:
///   the field's only writer is `respond_to_score_submission`, which promotes
///   a pending score only once every side of the match has confirmed it, and
///   a `ConfirmedScoreRecord` carries no confirmations of its own to re-check.
///   A pending or disputed score is one side's claim, and rating it would move
///   an opponent's number on an assertion they have not agreed to.
/// - **A ladder for the sport.** `Sport::Other` is unrated on purpose (see
///   `rating::ladder_for`).
/// - **Every side-assigned player is a linked account.** An unlinked guest has
///   no rating to give or receive, and Weng-Lin sums a side's beliefs — so
///   rating around them would silently credit their contribution to whoever
///   else was on their side. One guest therefore blocks the whole match, not
///   just their own row. The exception is a *declined* invitation, which is a
///   withdrawn roster row rather than somebody who played.
/// - **Everyone on a side actually played**, by the same rule
///   `reconcile_match_stats` uses: no invitation, or an accepted one. A
///   pending invitee's row means "we asked, they never answered", and rating
///   an unanswered invitation would hand out a movement for a game the person
///   may never have turned up to. Keeping this identical to the stats rule
///   also keeps `matches_rated` from disagreeing with `matches_played`.
/// - **No account on the roster twice.** Their two updates would disagree
///   about their own rating. `reconcile_match_stats` merges duplicates by
///   taking the best outcome; the equivalent fudge here has no meaning (there
///   is no "best" of two Gaussians), so the match is left unrated instead.
/// - **At least two sides, and every declared side represented.** A match
///   where one side has nobody rateable on it cannot say who beat whom.
///
/// Two judgement calls worth naming, because neither is obviously right:
///
/// 1. **An incomplete or uneven roster is rated as recorded.** Seven names
///    logged for an 11-a-side football team rates as a 7v11, and Weng-Lin
///    reads that as the seven being heavy underdogs. Refusing to rate uneven
///    sides was rejected because a genuinely uneven game (somebody didn't
///    show) is common in amateur sport and *should* rate as the upset it was,
///    and nothing stored distinguishes the two cases — no sport here declares
///    an expected side size. Blocking would silently and permanently refuse
///    real results in order to avoid a wrong one, whereas a mis-weighted
///    result is bounded by σ and is fixable by replay once the data exists to
///    tell them apart. Open for revisiting with real match data.
/// 2. **A pending invitee is excluded rather than blocking**, where an
///    unlinked player blocks. The asymmetry is deliberate: for an unlinked
///    player we *know* we cannot rate them, whereas for a pending one the
///    codebase already has an answer for "did they play" and consistency with
///    it beats inventing a second one.
fn rateable(m: &MatchRecord, players: &[MatchPlayerRecord]) -> Option<Rateable> {
    if !m.ranked || m.status != "completed" {
        return None;
    }
    let confirmed = m.confirmed_score.as_ref()?;
    let ladder = ladder_for_tag(&m.match_type)?;

    let mut participants: Vec<MatchParticipant> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for player in players {
        // Nobody assigned to a side is on nobody's side of the result, so they
        // neither rate nor block. (`side_id` is optional until assigned.)
        let Some(side_id) = &player.side_id else {
            continue;
        };
        if withdrawn(player) {
            continue;
        }
        // An unlinked guest: blocks the whole match, per the rules above.
        let user_id = player.user_id.as_ref()?;
        if !played(player) {
            continue;
        }
        if !seen.insert(user_id.as_str()) {
            return None;
        }
        participants.push(MatchParticipant {
            competitor_id: user_id.clone(),
            side_id: side_id.clone(),
        });
    }

    // Every side the match declares must have somebody rateable on it, and
    // there must be at least two of them. Checked against `m.sides` rather
    // than against the sides the participants happen to cover, so a side whose
    // whole roster was filtered out above (all pending invitees, say) fails
    // the match rather than quietly turning a 2v2 into a 2v0.
    if m.sides.len() < 2 {
        return None;
    }
    let covered: BTreeSet<&str> = participants.iter().map(|p| p.side_id.as_str()).collect();
    if !m
        .sides
        .keys()
        .all(|side_id| covered.contains(side_id.as_str()))
    {
        return None;
    }

    Some(Rateable {
        ladder,
        played_at: m.starts_at.clone(),
        winner_side_id: confirmed.winner_side_id.clone(),
        participants,
    })
}

/// Whether a roster row represents somebody who played, by the same definition
/// `reconcile_match_stats` uses: a self-added or organiser-added player (no
/// embedded invitation), or an accepted invitee.
fn played(player: &MatchPlayerRecord) -> bool {
    match &player.invitation {
        None => true,
        Some(invitation) => invitation.status == "accepted",
    }
}

/// Whether a roster row has been explicitly withdrawn — a declined
/// invitation. Distinct from "didn't play" because a declined row is *not* a
/// missing player: it is somebody who said no, so it should not block a match
/// the way an unlinked guest does.
fn withdrawn(player: &MatchPlayerRecord) -> bool {
    player
        .invitation
        .as_ref()
        .is_some_and(|invitation| invitation.status == "declined")
}

/// The contribution record one rating update implies for this match.
///
/// Every field a repair needs to re-rate the match — `side_id`, the incoming
/// belief, the ladder — comes from here, which is why this is one function and
/// not a struct literal at each call site.
///
/// `owner_kind` is a parameter rather than a constant because the replay path
/// builds contributions for whichever owner kind it is repairing. The
/// incremental path above only ever passes `User`: a team is a *side*, not a
/// participant, so rating one is a second pass over the same match (phase
/// 2b-iii), not a variation on this one.
fn contribution_for(
    update: &RatingUpdate,
    rateable: &Rateable,
    owner_kind: RatingOwnerKindRecord,
    now: &str,
) -> RatingContributionRecord {
    let side_id = rateable
        .participants
        .iter()
        .find(|p| p.competitor_id == update.competitor_id)
        .map(|p| p.side_id.clone())
        // Unreachable: every update comes from a participant. Falling back to
        // an empty side rather than panicking because a worker that panics
        // takes the whole consumer down, and an empty `side_id` fails the next
        // comparison loudly instead.
        .unwrap_or_default();
    RatingContributionRecord {
        owner_kind,
        owner_id: update.competitor_id.clone(),
        ladder: rateable.ladder.as_str().to_string(),
        side_id,
        played_at: rateable.played_at.clone(),
        movement: RatingMovementRecord {
            mu_before: update.before.mu,
            sigma_before: update.before.sigma,
            mu_after: update.after.mu,
            sigma_after: update.after.sigma,
            display_delta: update.display_delta(),
        },
        applied_at: now.to_string(),
    }
}

/// Why a rating cannot be brought up to date incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairTrigger {
    /// A match was rated after a *later* one had already been folded in. The
    /// numbers are self-consistent but not the ones a `starts_at`-ordered
    /// replay produces.
    ///
    /// The result is applied anyway, which is worth defending. Refusing to
    /// apply it would leave the match with no history item — and history items
    /// are exactly what a replay reads, so the match would be invisible to the
    /// very repair that was supposed to fix it, i.e. permanently unrated.
    /// Applying puts it in the collection at its correct `starts_at` key, so
    /// the replay finds it in the right place and converges; only the interim
    /// numbers are approximate. That also fixes when the repair may be
    /// started: not until the history entry exists, or the replay would find
    /// nothing to fix.
    OutOfOrder,
    /// An already-rated match's effect changed — a re-score, a reschedule, a
    /// sport edit, or a roster change. Weng-Lin is not invertible, so this
    /// cannot be applied as a delta.
    Rerated,
    /// An already-rated match stopped counting (cancelled, flipped to
    /// friendly, a participant unlinked). Its contribution and history are
    /// removed here; the stored rating still contains its effect until a
    /// replay removes it.
    Withdrawn,
}

/// Start the replay that brings an owner's ladder back into line, and record
/// why it was needed.
///
/// **The single choke point.** All three triggers come through here, and the
/// deterministic workflow id (`repair-<kind>-<ownerId>-<ladder>`) plus
/// `UseExisting` are what make that safe: calling it twice for the same owner
/// and ladder — which every one of the three paths above can do — attaches to
/// the run already going instead of starting a second one. Scattering
/// `start_repair` calls across the three call sites would work exactly as well
/// and would make it far easier for a fourth trigger to forget one of the
/// invariants documented at each of them (chiefly: for a withdrawal, start
/// *before* deleting).
///
/// `temporal = None` degrades to the log line alone. That is the shape phase
/// 2b-i shipped and it stays deliberately: a unit test or a local run without
/// the `full` compose profile should still exercise detection, and a silent
/// no-op would be indistinguishable from working machinery.
///
/// A failed start is returned as an error, so the SQS message is not ACKed and
/// the whole event redelivers. For `Rerated` and `Withdrawn` that is a
/// complete recovery — nothing is consumed before the start, so the next
/// delivery detects the same thing again. For `OutOfOrder` it is not, and the
/// call site says why: the start has to wait until the applies have committed,
/// by which point a redelivery finds nothing left to notice. That window is
/// the one place a repair can be dropped, and dropping it leaves the
/// incrementally-applied numbers rather than corrupting anything.
async fn note_repair_needed(
    temporal: Option<&TemporalClient>,
    trigger: RepairTrigger,
    match_id: &str,
    owner: &RatingOwner,
    ladder: &str,
) -> WorkerResult<()> {
    tracing::info!(
        trigger = ?trigger,
        match_id,
        owner_kind = ?owner.kind,
        owner_id = %owner.id,
        ladder,
        started = temporal.is_some(),
        "rating repair needed"
    );
    let Some(temporal) = temporal else {
        return Ok(());
    };
    temporal
        .start_repair(RepairRatingsInput {
            owner_kind: owner.kind,
            owner_id: owner.id.clone(),
            ladder: ladder.to_string(),
        })
        .await
        // `Sqs` is the variant the consumer already classifies a failed
        // Temporal start under (`maybe_start_workflow`). The name is a
        // misnomer for it; what matters is that it is *not* one of the
        // permanent variants, so the message redelivers rather than being
        // parked in the DLQ.
        .map_err(|e| WorkerError::Sqs(format!("start rating repair: {e}")))
}

// ===========================================================================
// The replay — `RepairRatings`' body, one chunk at a time.
// ===========================================================================

/// How many history items one replay chunk covers.
///
/// Each one costs a match read plus a contribution query, so a chunk is
/// roughly `2 × REPLAY_PAGE` round trips; 50 keeps that comfortably inside the
/// activity's start-to-close timeout while still checkpointing often enough
/// that a crashed repair resumes near where it stopped.
pub const REPLAY_PAGE: u32 = 50;

/// The replay's running state, carried between chunks by the workflow.
///
/// Deliberately small and serializable: it is round-tripped through Temporal's
/// event history on every chunk boundary, and it is the *only* thing a resumed
/// chunk is allowed to assume. Everything else a chunk needs — what the table
/// currently holds, what the match now says — it re-reads, because an activity
/// can be retried after a partial write and must recompute the same answer
/// from the same input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayState {
    /// Matches folded in so far. Ends as the owner's `matches_rated`.
    pub matches_rated: u64,
    pub mu: f64,
    pub sigma: f64,
    /// The greatest `played_at` folded in so far. Ends as `last_rated_at`.
    pub last_rated_at: String,
    /// Matches this run has already folded **and whose history item folding
    /// them moved to a later sort key** — the set the walk has to recognise on
    /// sight, because the page cursor will hand them back.
    ///
    /// This is the one piece of state that exists to defend an invariant
    /// rather than to compute a rating, so it is worth spelling the hazard
    /// out. The walk pages the history collection with a live DynamoDB cursor
    /// and *rewrites that same collection as it goes*: a match rescheduled
    /// since it was rated has its history item deleted from the old
    /// `played_at` key and rewritten at the new one, in the same transaction
    /// (`dao::rating::apply_rating_contribution`, step 4). Move one forward
    /// past the key this page ends on and the next page returns it a second
    /// time — the walk folds it again, `matches_rated` gains one, and the
    /// owner's μ/σ absorb the match twice.
    ///
    /// Nothing detects that afterwards, which is why it is guarded here rather
    /// than reconciled later. The second fold rewrites the contribution too, so
    /// the stored `mu_before` and the movement the *next* incremental delivery
    /// recomputes agree exactly — `has_same_effect_as` says "unchanged", no
    /// repair is triggered, and the corrupted rating is stable and silent.
    ///
    /// Only forward moves need recording: a page starts strictly after the
    /// previous page's last key, so an item rewritten at or below that key can
    /// never come round again, and one rewritten below its own key was already
    /// behind the walk. Membership is therefore bounded by the reschedules a
    /// single replay walks past, not by the ladder's length — which matters,
    /// because this whole struct is round-tripped through Temporal's event
    /// history on every chunk boundary.
    ///
    /// `#[serde(default)]` for the same reason the record fields have it: a
    /// repair already in flight when the worker is upgraded resumes from a
    /// state serialized before this field existed. An empty set is the correct
    /// reading of one — it means "no move seen yet", which is where every run
    /// starts.
    #[serde(default)]
    pub moved_ahead: BTreeSet<String>,
}

impl Default for ReplayState {
    /// Where every replay starts: the belief a never-rated account has.
    fn default() -> Self {
        Self {
            matches_rated: 0,
            mu: INITIAL_MU,
            sigma: INITIAL_SIGMA,
            last_rated_at: String::new(),
            moved_ahead: BTreeSet::new(),
        }
    }
}

impl ReplayState {
    fn belief(&self) -> PlayerRating {
        PlayerRating {
            mu: self.mu,
            sigma: self.sigma,
        }
    }

    /// Fold one replayed match in.
    fn fold(&mut self, after: PlayerRating, played_at: &str) {
        self.matches_rated += 1;
        self.mu = after.mu;
        self.sigma = after.sigma;
        if played_at > self.last_rated_at.as_str() {
            self.last_rated_at = played_at.to_string();
        }
    }

    /// Record that folding `match_id` moved its history item to a later sort
    /// key, so the rest of this run recognises it when the cursor catches up.
    /// See [`ReplayState::moved_ahead`].
    fn note_history_moved(&mut self, match_id: &str) {
        self.moved_ahead.insert(match_id.to_string());
    }

    /// Whether the walk is looking at a match it has already folded, arriving
    /// a second time at the key its own write moved it to.
    ///
    /// Consuming rather than a plain `contains`: once the walk is standing on
    /// the moved item, the cursor is about to pass it and it cannot come round
    /// again, so forgetting it here keeps the set to the moves still ahead of
    /// the walk instead of every move the run has ever made.
    fn is_second_sighting(&mut self, match_id: &str) -> bool {
        self.moved_ahead.remove(match_id)
    }

    /// The rating record this state describes.
    fn rating(&self) -> RatingRecord {
        RatingRecord {
            mu: self.mu,
            sigma: self.sigma,
            matches_rated: self.matches_rated,
            last_rated_at: self.last_rated_at.clone(),
        }
    }
}

/// One chunk's result: the state to carry forward, and where to resume.
/// `next_cursor = None` means the ladder's history is exhausted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayProgress {
    pub state: ReplayState,
    pub next_cursor: Option<String>,
}

/// The slice of the DAO this module touches — the incremental path and the
/// replay both.
///
/// Most of this module is testable as pure functions against records. The two
/// entry points are not, and both of the bugs this seam was introduced for are
/// bugs *of sequencing across several reads and writes*, which no pure
/// function can express:
///
/// - the replay's central hazard is **paging over a collection it is
///   concurrently rewriting** ([`ReplayState::moved_ahead`]), which by
///   construction only shows up when a page boundary falls between an item's
///   two sightings;
/// - the incremental path's is **one delivery carrying both a re-rating and a
///   first rating**, where handling only the first strands the second
///   ([`reconcile_match_rating`]'s pass one).
///
/// Both need a store that really stores, and `Dao` needs DynamoDB. So there
/// are exactly two implementations: `Dao`, which ships, and an in-memory model
/// in this module's tests, which is what makes those two regression tests
/// reproductions rather than assertions about a helper.
///
/// Deliberately narrower than `Dao`, in two places, and narrower is the point
/// — a fake can only be honest about a surface the handler actually uses.
/// `list_rating_history`'s `from` bound is dropped because a replay always
/// walks a ladder whole (see [`crate::temporal::workflows::RepairRatings`] on
/// why it is not a tail replay). And [`RatingStore::batch_get_ratings`]
/// replaces `Dao::batch_get_users`, of which the handler reads exactly one
/// field.
//
// `async fn` in a trait: crate-private, one production implementor, and every
// call site instantiates it concretely, so the futures the Temporal activity
// awaits are still `Send` by auto-trait leakage. The usual objection to AFIT —
// that a *public* trait cannot promise `Send` to its callers — does not apply.
#[allow(async_fn_in_trait)]
pub(crate) trait RatingStore {
    async fn get_rating(
        &self,
        owner: &RatingOwner,
        ladder: &str,
    ) -> DaoResult<Option<RatingRecord>>;

    /// Several accounts' current rating on one ladder, in one read.
    ///
    /// Narrower than the `Dao::batch_get_users` that implements it, and that
    /// is deliberate: `ratings` rides the profile item precisely so a
    /// 22-player match costs the batch read the rest of the system already
    /// does rather than a rating-specific one (phase 2a), but `ratings` on
    /// that ladder is the *only* thing this module reads off a user. An
    /// account with no rating on the ladder is simply absent from the result,
    /// exactly as one with no profile item is.
    ///
    /// User-keyed, unlike everything else here, because the incremental path
    /// only rates users: a team is a *side*, not a participant.
    async fn batch_get_ratings(
        &self,
        user_ids: &[String],
        ladder: &str,
    ) -> DaoResult<std::collections::HashMap<String, RatingRecord>>;

    async fn list_rating_history(
        &self,
        owner: &RatingOwner,
        ladder: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<RatingHistoryRecord>>;

    async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>>;

    async fn list_rating_contributions(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<RatingContributionRecord>>;

    async fn apply_rating_contribution(
        &self,
        match_id: &str,
        contribution: &RatingContributionRecord,
        stored: Option<&RatingContributionRecord>,
        rating: &RatingRecord,
        stored_rating: Option<&RatingRecord>,
    ) -> DaoResult<()>;

    async fn withdraw_rating_contribution(
        &self,
        match_id: &str,
        stored: &RatingContributionRecord,
    ) -> DaoResult<()>;
}

/// The production binding: straight delegation, no behaviour of its own beyond
/// the one projection `batch_get_ratings` names. If another method here ever
/// grows a body, the fake in the tests has stopped standing for the real thing
/// and the regressions it guards have stopped being guarded.
impl RatingStore for Dao {
    async fn get_rating(
        &self,
        owner: &RatingOwner,
        ladder: &str,
    ) -> DaoResult<Option<RatingRecord>> {
        Dao::get_rating(self, owner, ladder).await
    }

    async fn batch_get_ratings(
        &self,
        user_ids: &[String],
        ladder: &str,
    ) -> DaoResult<std::collections::HashMap<String, RatingRecord>> {
        Ok(Dao::batch_get_users(self, user_ids)
            .await?
            .into_iter()
            .filter_map(|(id, user)| Some((id, user.ratings.get(ladder)?.clone())))
            .collect())
    }

    async fn list_rating_history(
        &self,
        owner: &RatingOwner,
        ladder: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<RatingHistoryRecord>> {
        Dao::list_rating_history(self, owner, ladder, None, cursor, limit).await
    }

    async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>> {
        Dao::get_match(self, match_id).await
    }

    async fn list_rating_contributions(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<RatingContributionRecord>> {
        Dao::list_rating_contributions(self, match_id).await
    }

    async fn apply_rating_contribution(
        &self,
        match_id: &str,
        contribution: &RatingContributionRecord,
        stored: Option<&RatingContributionRecord>,
        rating: &RatingRecord,
        stored_rating: Option<&RatingRecord>,
    ) -> DaoResult<()> {
        Dao::apply_rating_contribution(self, match_id, contribution, stored, rating, stored_rating)
            .await
    }

    async fn withdraw_rating_contribution(
        &self,
        match_id: &str,
        stored: &RatingContributionRecord,
    ) -> DaoResult<()> {
        Dao::withdraw_rating_contribution(self, match_id, stored).await
    }
}

/// Replay one page of an owner's rating history on one ladder, writing the
/// corrected contribution, history entry and rating for every match whose
/// effect on this owner has changed.
///
/// ## What "correct" means here, precisely
///
/// The owner's belief walks forward through their own matches in played order.
/// Everybody *else* in each match is held at the belief they carried into it,
/// as recorded by their own `RATINGCONTRIB#` item for that match. That is what
/// makes repair first-order rather than a graph recompute, and it is the
/// single most important thing to understand before changing this function —
/// see [`crate::temporal::workflows::RepairRatings`] for why the second-order
/// term is left on the table.
///
/// ## Why it re-reads the match instead of trusting the contributions alone
///
/// Phase 2a designed the contribution collection to be a self-sufficient
/// replay input, and for the *beliefs* it is. It cannot supply two things: the
/// current `winner_side_id` (a re-score is one of the three triggers, so the
/// stored view is precisely the one that is wrong), and the current roster (a
/// roster edit is another trigger — replaying from the contributions alone
/// would silently drop a player added since, and the match would never reflect
/// them). So the match goes through the very same [`rateable`] gate the
/// incremental path uses, which also means the two can never disagree about
/// what counts.
///
/// ## Idempotence
///
/// A chunk is a pure function of `(owner, ladder, cursor, state)`: it re-reads
/// the owner's stored rating rather than being told it, and it re-reads the
/// page rather than being handed items. So a retry after a partial write
/// recomputes the same movements and finds the ones it already wrote unchanged
/// (`has_same_effect_as`), skipping them. A replay of a *healthy* history
/// writes nothing at all, which is what makes an over-eager repair cost reads
/// and nothing else.
///
/// ## The collection moves underneath the cursor
///
/// The one thing this walk may **not** assume is that the page cursor is
/// walking a stable collection. It rewrites the very item collection it is
/// paging, and a match rescheduled since it was rated has its history item
/// moved to a new sort key by that write. [`ReplayState::moved_ahead`] carries
/// the whole argument, including why folding such a match twice would corrupt
/// a rating permanently and undetectably; the short version is that every
/// match must be folded exactly once per run, and the sort key is not the
/// thing that guarantees it.
///
/// A match that moves is still folded at the position the walk *found* it,
/// not at its new one, so the rating this run settles on is the one played
/// order said before the reschedule. That is knowingly approximate and
/// self-healing rather than sticky: the move leaves the collection in true
/// played order, so the owner's next repair — from any trigger — lands on the
/// right number. It is the same bounded staleness `RepairRatings`' "the race
/// that is left" section documents, and the alternative (deferring the fold to
/// the new position) needs a way to relocate a history item without rating it,
/// which the DAO has no operation for.
pub(crate) async fn replay_rating_chunk<S: RatingStore>(
    store: &S,
    owner: &RatingOwner,
    ladder: &str,
    cursor: Option<&str>,
    state: ReplayState,
    now: &str,
) -> WorkerResult<ReplayProgress> {
    // The optimistic-lock guard for every rating write below. Re-read per
    // chunk rather than threaded through the workflow: the workflow's copy
    // could only ever be staler, and a stale guard is a `Conflict`, which
    // costs an activity retry.
    let mut stored_rating = store.get_rating(owner, ladder).await?;
    let mut state = state;

    // Ascending by `played_at`, which is the order ratings are defined to be
    // applied in — see `Sk::Rating`.
    let page = store
        .list_rating_history(owner, ladder, cursor, REPLAY_PAGE)
        .await?;

    for entry in &page.items {
        // Already folded, earlier in this same run, at the key this walk has
        // since moved it away from. Folding it again is the double-count
        // `ReplayState::moved_ahead` exists to stop — and the guard has to sit
        // here, before the reads and before `fold`, because nothing further
        // down can tell the second sighting from a first one.
        if state.is_second_sighting(&entry.match_id) {
            continue;
        }

        // Only this owner kind's contributions. `RATINGCONTRIB#<id>`
        // deliberately does not say whether the id names a user or a team (so
        // that one `begins_with` lists a match's whole set), which means both
        // kinds land in the same collection once phase 2b-iii rates teams —
        // and a team's `μ` is not a number that belongs anywhere near a
        // player's side strength.
        let stored: BTreeMap<String, RatingContributionRecord> = store
            .list_rating_contributions(&entry.match_id)
            .await?
            .into_iter()
            .filter(|c| c.owner_kind == owner.kind)
            .map(|c| (c.owner_id.clone(), c))
            .collect();

        let Some(update) =
            replay_match(store, owner, ladder, &entry.match_id, &state, &stored, now).await?
        else {
            // The match no longer counts for this owner — cancelled, demoted
            // to a friendly, or they were dropped from the roster. Take it out
            // of the history here rather than leaving it: it is what the *next*
            // replay reads, and a history entry describing a movement the
            // rating no longer contains would be shown on the chart forever.
            // The inline handler does the same thing, but it only ever sees
            // `#META` writes — a roster edit is a `PLAYER#` write, which never
            // reaches it — so this is not redundant with it.
            if let Some(previous) = stored.get(&owner.id) {
                store
                    .withdraw_rating_contribution(&entry.match_id, previous)
                    .await?;
            }
            continue;
        };

        state.fold(update.after, &update.played_at);

        let previous = stored.get(&owner.id);
        if previous.is_some_and(|p| p.has_same_effect_as(&update.contribution)) {
            // Byte-identical to what is stored, so there is nothing to write.
            // This is the common case even inside a real repair: only matches
            // at or after the disturbance actually move. Nothing was written,
            // so the history item cannot have moved either.
            continue;
        }

        let rating = state.rating();
        match store
            .apply_rating_contribution(
                &entry.match_id,
                &update.contribution,
                previous,
                &rating,
                stored_rating.as_ref(),
            )
            .await
        {
            Ok(()) => {
                stored_rating = Some(rating);
                // Did that write move this entry's sort key? It does whenever
                // the match's `starts_at` has changed since it was rated —
                // `apply_rating_contribution` writes the history item at the
                // contribution's own key and deletes the previous one. A key
                // that moved *forward* may land beyond the cursor this page
                // ends on, in which case the next page hands the match back;
                // see `ReplayState::moved_ahead`. Compared as rendered strings
                // because that is precisely what DynamoDB orders the
                // collection on.
                let read_at = Sk::Rating {
                    ladder: entry.ladder.clone(),
                    played_at: entry.played_at.clone(),
                    match_id: entry.match_id.clone(),
                }
                .to_string();
                if update.contribution.history_sk(&entry.match_id).to_string() > read_at {
                    state.note_history_moved(&entry.match_id);
                }
            }
            // The owner's profile item is gone (a deleted account or team).
            // Stop rather than fail forever: the plan's team open question
            // settles this the same way stat contributions already are — stop
            // rating a deleted entity and leave its history orphaned.
            Err(DaoError::NotFound(what)) => {
                tracing::warn!(
                    owner_id = %owner.id,
                    ladder,
                    "abandoning rating repair: {what} no longer exists"
                );
                return Ok(ReplayProgress {
                    state,
                    next_cursor: None,
                });
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(ReplayProgress {
        state,
        next_cursor: page.next_cursor,
    })
}

/// What replaying one match did to the owner being repaired, or `None` if the
/// match no longer counts for them at all.
struct ReplayedMatch {
    after: PlayerRating,
    played_at: String,
    contribution: RatingContributionRecord,
}

/// Re-rate one match as of the replay's current position, from the owner's
/// point of view.
async fn replay_match<S: RatingStore>(
    store: &S,
    owner: &RatingOwner,
    ladder: &str,
    match_id: &str,
    state: &ReplayState,
    stored: &BTreeMap<String, RatingContributionRecord>,
    now: &str,
) -> WorkerResult<Option<ReplayedMatch>> {
    let Some(agg) = store.get_match(match_id).await? else {
        return Ok(None);
    };
    let Some(rateable) = rateable_for(owner.kind, &agg.match_, &agg.players)? else {
        return Ok(None);
    };
    // The match's sport was edited onto a different ladder. It is not this
    // ladder's business any more; the incremental path moves the history item
    // to the new ladder's keyspace when it re-applies the match.
    if rateable.ladder.as_str() != ladder {
        return Ok(None);
    }
    if !rateable
        .participants
        .iter()
        .any(|p| p.competitor_id == owner.id)
    {
        return Ok(None);
    }

    let mut base: RatingTable = RatingTable::new();
    for participant in &rateable.participants {
        let belief = if participant.competitor_id == owner.id {
            state.belief()
        } else if let Some(contribution) = stored.get(&participant.competitor_id) {
            PlayerRating {
                mu: contribution.movement.mu_before,
                sigma: contribution.movement.sigma_before,
            }
        } else {
            // A participant this match was never rated for: they joined the
            // roster after it was. Nothing records what they believed at the
            // time, so their current rating is the closest available stand-in
            // — wrong by whatever they have played since, but far closer than
            // the alternative of treating an established player as brand new.
            // Only reachable on the roster-edit trigger.
            store
                .get_rating(
                    &RatingOwner {
                        kind: owner.kind,
                        id: participant.competitor_id.clone(),
                    },
                    ladder,
                )
                .await?
                .map(|r| PlayerRating {
                    mu: r.mu,
                    sigma: r.sigma,
                })
                .unwrap_or_default()
        };
        base.insert(participant.competitor_id.clone(), belief);
    }

    let updates = rate_match(
        &rateable.participants,
        rateable.winner_side_id.as_deref(),
        &base,
    )
    .map_err(|e| WorkerError::Invariant(format!("match {match_id} cannot be replayed: {e}")))?;

    let update = updates
        .iter()
        .find(|u| u.competitor_id == owner.id)
        .ok_or_else(|| {
            WorkerError::Invariant(format!(
                "match {match_id} produced no rating update for {}",
                owner.id
            ))
        })?;

    Ok(Some(ReplayedMatch {
        after: update.after,
        played_at: rateable.played_at.clone(),
        contribution: contribution_for(update, &rateable, owner.kind, now),
    }))
}

/// Settle an owner's rating at the end of a replay.
///
/// Separate from the per-match writes because the last of those does not
/// necessarily leave the right value behind: a replay whose *tail* was
/// unchanged writes nothing for it, and a replay of an empty history (every
/// match withdrawn) writes nothing at all and still has to put the account
/// back to unrated. This is also the only write in a repair that can lower
/// `matches_rated`.
///
/// Takes a `Dao` rather than a [`RatingStore`], unlike the two entry points
/// either side of it. One read and one write with no sequencing between them
/// is exactly what a pure function *can* stand in for, so there is no
/// reproduction here that needs a fake — and widening the trait with
/// `put_rating` to buy nothing would make the fake stand for a surface no test
/// exercises, which is how a fake stops being honest.
pub async fn finish_rating_replay(
    dao: &Dao,
    owner: &RatingOwner,
    ladder: &str,
    state: &ReplayState,
) -> WorkerResult<()> {
    let stored = dao.get_rating(owner, ladder).await?;
    let settled = state.rating();
    if stored.as_ref() == Some(&settled) {
        return Ok(());
    }
    // Never rated, nothing replayed: there is no rating to settle, and writing
    // one would enrol an account into a ladder it has never played on.
    if stored.is_none() && state.matches_rated == 0 {
        return Ok(());
    }
    match dao
        .put_rating(owner, ladder, &settled, stored.as_ref())
        .await
    {
        Ok(()) => Ok(()),
        Err(DaoError::NotFound(what)) => {
            tracing::warn!(
                owner_id = %owner.id,
                ladder,
                "abandoning rating repair: {what} no longer exists"
            );
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// The `MatchRecord` → participants projection for one owner kind.
///
/// The **only** owner-specific step in the whole repair path: everything else
/// — the workflow, the chunking, the state, every DAO call — takes a
/// [`RatingOwner`] and does not care which kind it holds. That is deliberate,
/// because phase 2b-iii rates teams as entities in their own right and is
/// meant to inherit this replay whole; its job here is to fill in the `Team`
/// arm with the side pass (one competitor per side, keyed on the side's
/// `team_id`), and nothing else.
///
/// It errors rather than returning `None` for a kind it cannot project, and
/// that distinction matters: `None` means "this match does not count", so a
/// silent one would replay a team's whole history as *no* matches and reset a
/// real rating to unrated. Unreachable today — nothing writes a team rating,
/// so no team repair can be triggered — which is exactly why it must be loud
/// if it ever becomes reachable.
fn rateable_for(
    owner_kind: RatingOwnerKindRecord,
    m: &MatchRecord,
    players: &[MatchPlayerRecord],
) -> WorkerResult<Option<Rateable>> {
    match owner_kind {
        RatingOwnerKindRecord::User => Ok(rateable(m, players)),
        RatingOwnerKindRecord::Team => Err(WorkerError::Invariant(
            "team ratings are not computed yet (phase 2b-iii), so a team ladder has nothing to replay"
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agon_core::dao::records::{
        ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationKindRecord, MatchSideRecord,
        ScoreRecord,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ops::Bound;

    fn side(side_id: &str) -> (String, MatchSideRecord) {
        (
            side_id.to_string(),
            MatchSideRecord {
                side_id: side_id.to_string(),
                team_id: None,
                name: None,
                player_count: 1,
                roster_preview: Vec::new(),
            },
        )
    }

    /// A completed, ranked, confirmed 1v1 — the shape everything below varies
    /// one field of.
    fn match_record() -> MatchRecord {
        MatchRecord {
            id: "m1".into(),
            created_by_user_id: "u1".into(),
            name: "Test".into(),
            description: String::new(),
            match_type: "squash".into(),
            status: "completed".into(),
            starts_at: "2026-06-01T10:00:00.000Z".into(),
            location: None,
            sides: HashMap::from([side("a"), side("b")]),
            header_photos: Vec::new(),
            confirmed_score: Some(ConfirmedScoreRecord {
                score: ScoreRecord::Simple {
                    entries: HashMap::from([("a".to_string(), 3), ("b".to_string(), 1)]),
                },
                winner_side_id: Some("a".into()),
            }),
            pending_score: None,
            like_count: 0,
            comment_count: 0,
            live_seq: 0,
            live_tip_seq: None,
            format: None,
            ranked: true,
            rating_requirement: None,
            created_at: "2026-05-01T10:00:00.000Z".into(),
        }
    }

    fn player(player_id: &str, user_id: Option<&str>, side_id: Option<&str>) -> MatchPlayerRecord {
        MatchPlayerRecord {
            player_id: player_id.into(),
            user_id: user_id.map(Into::into),
            display_name: None,
            side_id: side_id.map(Into::into),
            is_member_of_team: None,
            invitation: None,
        }
    }

    fn invited(mut player: MatchPlayerRecord, status: &str) -> MatchPlayerRecord {
        player.invitation = Some(EmbeddedInvitationRecord {
            id: "i1".into(),
            status: status.into(),
            invited_by_user_id: "u1".into(),
            invited_at: "2026-05-02T10:00:00.000Z".into(),
            responded_at: None,
            kind: InvitationKindRecord::User {
                invited_user_id: "u2".into(),
            },
        });
        player
    }

    fn singles() -> Vec<MatchPlayerRecord> {
        vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("b")),
        ]
    }

    /// The happy path, and the shape of what the engine is handed: one
    /// participant per side, the ladder from the sport tag, and `played_at`
    /// taken from `starts_at` — *not* from a confirmation time, since played
    /// order is the order ratings are supposed to be applied in.
    #[test]
    fn a_confirmed_ranked_match_projects_to_one_participant_per_side() {
        let rated = rateable(&match_record(), &singles()).expect("rateable");
        assert_eq!(rated.ladder.as_str(), "squash");
        assert_eq!(rated.played_at, "2026-06-01T10:00:00.000Z");
        assert_eq!(rated.winner_side_id.as_deref(), Some("a"));
        assert_eq!(
            rated.participants,
            vec![
                MatchParticipant {
                    competitor_id: "u1".into(),
                    side_id: "a".into()
                },
                MatchParticipant {
                    competitor_id: "u2".into(),
                    side_id: "b".into()
                },
            ]
        );
    }

    /// A friendly is a game people agreed not to be judged on. This is also
    /// the gate every match written before the rating system existed falls
    /// through, since `MatchRecord::ranked` deserializes to `false` — without
    /// it, the entire back catalogue would enrol itself into the ladders one
    /// like at a time.
    #[test]
    fn a_friendly_match_is_not_rated() {
        let mut m = match_record();
        m.ranked = false;
        assert_eq!(rateable(&m, &singles()), None);
    }

    /// A score that only one side has agreed to is a claim, not a result.
    /// Rating it would move the opponent's number on an assertion they have
    /// not accepted — and `confirmed_score`'s presence is exactly the
    /// "everyone confirmed" test, because the only code that sets it does so
    /// after checking every side confirmed.
    #[test]
    fn an_unconfirmed_or_disputed_score_is_not_rated() {
        let mut m = match_record();
        m.confirmed_score = None;
        assert_eq!(rateable(&m, &singles()), None);
    }

    /// Cancelled and still-scheduled matches have no result to rate. Both
    /// arrive here as ordinary `#META` writes, so the status check is the only
    /// thing separating them from a played game.
    #[test]
    fn only_a_completed_match_is_rated() {
        for status in ["scheduled", "in_progress", "cancelled"] {
            let mut m = match_record();
            m.status = status.into();
            assert_eq!(rateable(&m, &singles()), None, "status {status}");
        }
    }

    /// `Sport::Other` has no ladder on purpose — it is not one sport, it is
    /// every sport we haven't modelled, and pooling them would be the
    /// disjoint-graph mistake per-sport ladders exist to avoid.
    #[test]
    fn a_sport_with_no_ladder_is_not_rated() {
        let mut m = match_record();
        m.match_type = "other".into();
        assert_eq!(rateable(&m, &singles()), None);
        m.match_type = "kabaddi".into();
        assert_eq!(rateable(&m, &singles()), None);
    }

    /// One unlinked guest blocks the *whole* match, not just their own row.
    /// Weng-Lin sums a side's beliefs, so quietly rating around them would
    /// credit the guest's contribution to whoever else was on their side —
    /// beating a 2-player side would be recorded as beating a 1-player one.
    #[test]
    fn an_unlinked_guest_on_a_side_blocks_the_whole_match() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("b")),
            player("p3", None, Some("b")),
        ];
        assert_eq!(rateable(&match_record(), &players), None);
    }

    /// ...but an unlinked player who is not on a side isn't part of the
    /// result at all, so they neither rate nor block.
    #[test]
    fn an_unassigned_player_neither_rates_nor_blocks() {
        let mut players = singles();
        players.push(player("p3", None, None));
        players.push(player("p4", Some("u4"), None));
        let rated = rateable(&match_record(), &players).expect("rateable");
        assert_eq!(rated.participants.len(), 2);
    }

    /// A declined invitation is a withdrawn roster row, not somebody who
    /// played — so it does not rate, and (unlike an unlinked guest) it does
    /// not block either, even when the declining row was never linked to an
    /// account.
    #[test]
    fn a_declined_invitee_neither_rates_nor_blocks() {
        let mut players = singles();
        players.push(invited(player("p3", None, Some("b")), "declined"));
        players.push(invited(player("p4", Some("u4"), Some("a")), "declined"));
        let rated = rateable(&match_record(), &players).expect("rateable");
        assert_eq!(
            rated.participants.len(),
            2,
            "a declined row is not a participant"
        );
    }

    /// A pending invitee is "we asked, they never answered". Rating them would
    /// hand out a movement for a game they may not have turned up to, and this
    /// is the same rule `reconcile_match_stats` applies — deliberately, so
    /// `matches_rated` cannot disagree with `matches_played`.
    #[test]
    fn a_pending_invitee_does_not_play_and_so_does_not_rate() {
        let mut players = singles();
        players.push(invited(player("p3", Some("u3"), Some("a")), "pending"));
        let rated = rateable(&match_record(), &players).expect("rateable");
        assert_eq!(rated.participants.len(), 2);
        assert!(rated.participants.iter().all(|p| p.competitor_id != "u3"));
    }

    /// An accepted invitee played, and rates.
    #[test]
    fn an_accepted_invitee_rates() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            invited(player("p2", Some("u2"), Some("b")), "accepted"),
        ];
        let rated = rateable(&match_record(), &players).expect("rateable");
        assert_eq!(rated.participants.len(), 2);
    }

    /// A side with nobody rateable left on it fails the whole match rather
    /// than turning a 2v2 into a 2v0 — the engine would happily rate one side
    /// against itself's absence, and the sides are checked against
    /// `MatchRecord::sides` rather than against whoever survived the filter
    /// precisely so this can be caught.
    #[test]
    fn a_side_with_no_rateable_players_fails_the_match() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            invited(player("p2", Some("u2"), Some("b")), "pending"),
        ];
        assert_eq!(rateable(&match_record(), &players), None);
    }

    /// A single-sided match has nothing to compare against. (The engine
    /// rejects it too; catching it here keeps that rejection reserved for
    /// genuinely corrupt data.)
    #[test]
    fn a_match_with_fewer_than_two_sides_is_not_rated() {
        let mut m = match_record();
        m.sides = HashMap::from([side("a")]);
        assert_eq!(rateable(&m, &[player("p1", Some("u1"), Some("a"))]), None);
    }

    /// One account on the roster twice — on either side — leaves two updates
    /// disagreeing about the same rating. `reconcile_match_stats` merges
    /// duplicates by taking the best outcome; there is no equivalent fudge for
    /// a Gaussian, so the match is left unrated for a human to fix.
    #[test]
    fn one_account_appearing_twice_is_not_rated() {
        let across_sides = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("b")),
            player("p3", Some("u1"), Some("b")),
        ];
        assert_eq!(rateable(&match_record(), &across_sides), None);

        let same_side = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u1"), Some("a")),
            player("p3", Some("u2"), Some("b")),
        ];
        assert_eq!(rateable(&match_record(), &same_side), None);
    }

    /// A draw — no `winner_side_id` — is rated, not skipped. Weng-Lin models a
    /// tie natively (equal ranks), so a draw between mismatched players is
    /// real information and moves both ratings towards each other.
    #[test]
    fn a_draw_is_rateable() {
        let mut m = match_record();
        m.confirmed_score.as_mut().unwrap().winner_side_id = None;
        let rated = rateable(&m, &singles()).expect("rateable");
        assert_eq!(rated.winner_side_id, None);
    }

    /// Uneven sides rate as recorded. This is the judgement call documented on
    /// `rateable`: nothing stored distinguishes "one player didn't show" from
    /// "only seven of the eleven were logged", so refusing to rate the shape
    /// would refuse real results to avoid wrong ones. The test exists to make
    /// the choice explicit rather than accidental — if a later phase adds an
    /// expected side size and blocks on it, this is the test that should
    /// change.
    #[test]
    fn an_uneven_roster_rates_as_recorded() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("a")),
            player("p3", Some("u3"), Some("b")),
        ];
        let rated = rateable(&match_record(), &players).expect("rateable");
        assert_eq!(rated.participants.len(), 3);
    }

    /// The contribution has to carry everything a repair needs to re-rate the
    /// match without the roster: which side the owner played for, and the
    /// belief they carried in. If `side_id` were dropped, the whole
    /// `RATINGCONTRIB#` collection would stop being a self-sufficient replay
    /// input and repair would depend on a roster that may since have changed.
    #[test]
    fn the_contribution_carries_the_side_and_the_incoming_belief() {
        let rated = rateable(&match_record(), &singles()).expect("rateable");
        let update = RatingUpdate {
            competitor_id: "u2".into(),
            before: PlayerRating {
                mu: 25.0,
                sigma: 25.0 / 3.0,
            },
            after: PlayerRating {
                mu: 22.0,
                sigma: 7.0,
            },
        };
        let contribution = contribution_for(
            &update,
            &rated,
            RatingOwnerKindRecord::User,
            "2026-06-02T09:00:00.000Z",
        );
        assert_eq!(contribution.owner_kind, RatingOwnerKindRecord::User);
        assert_eq!(contribution.owner_id, "u2");
        assert_eq!(contribution.ladder, "squash");
        assert_eq!(contribution.side_id, "b");
        assert_eq!(contribution.played_at, rated.played_at);
        assert_eq!(contribution.movement.mu_before, 25.0);
        assert_eq!(contribution.movement.mu_after, 22.0);
        assert_eq!(contribution.movement.display_delta, -90);
    }

    /// A replay walks matches in played order, so `last_rated_at` is the
    /// *last* one it folds in — but it is written as a maximum anyway, and
    /// that is not belt-and-braces. `Sk::Rating` sorts on `played_at` and then
    /// on match id, so two matches at the same instant come back in id order,
    /// and a history whose entries were written by an out-of-order incremental
    /// run is exactly the input a repair is handed. Taking the maximum makes
    /// the field mean what `RatingRecord::last_rated_at` says it means —
    /// "the newest match played" — regardless of how the collection is walked.
    #[test]
    fn a_replay_tracks_the_newest_match_played_not_the_last_one_folded_in() {
        let mut state = ReplayState::default();
        assert_eq!(state.matches_rated, 0);
        assert_eq!(state.belief(), PlayerRating::default());

        let moved = PlayerRating {
            mu: 27.0,
            sigma: 7.0,
        };
        state.fold(moved, "2026-06-10T10:00:00.000Z");
        state.fold(moved, "2026-06-03T10:00:00.000Z");

        assert_eq!(state.matches_rated, 2, "both matches count");
        assert_eq!(
            state.rating().last_rated_at,
            "2026-06-10T10:00:00.000Z",
            "an earlier match must not drag the newest-played marker backwards"
        );
    }

    /// An empty replay is how a rating gets *unwound*: withdraw the only match
    /// on a ladder and the replay finds no history at all, which has to mean
    /// "never rated" rather than "leave it alone". The default state is
    /// therefore the same belief a brand-new account has, with a zero count —
    /// which is what puts the account back below `PLACEMENT_MATCHES` and so
    /// back to an `Unrated` band.
    #[test]
    fn a_replay_over_no_history_settles_on_the_unrated_default() {
        let settled = ReplayState::default().rating();
        assert_eq!(settled.matches_rated, 0);
        assert_eq!(settled.mu, INITIAL_MU);
        assert_eq!(settled.sigma, INITIAL_SIGMA);
        assert_eq!(
            settled.last_rated_at, "",
            "an empty marker sorts below every real `played_at`, so it can \
             never make the next match look out of order"
        );
    }

    /// The one owner-kind-specific step in the repair path must fail loudly
    /// for a kind it cannot project, never quietly return "no participants".
    ///
    /// `None` from this function means "this match does not count", so a
    /// silent `None` for teams would make a team's repair replay its entire
    /// history as zero matches and reset a real rating to unrated — a data
    /// loss dressed up as a no-op. Unreachable today (nothing writes a team
    /// rating, so no team repair can be triggered), which is precisely why the
    /// guard has to be in place before phase 2b-iii makes it reachable.
    #[test]
    fn a_replay_refuses_an_owner_kind_it_cannot_project_rather_than_wiping_it() {
        let m = match_record();
        assert!(
            rateable_for(RatingOwnerKindRecord::User, &m, &singles())
                .expect("users project")
                .is_some()
        );
        let team = rateable_for(RatingOwnerKindRecord::Team, &m, &singles());
        assert!(
            matches!(team, Err(WorkerError::Invariant(_))),
            "a team ladder must fail the replay, not replay as nothing: {team:?}"
        );
    }

    /// A replay's running belief has to survive the Temporal payload boundary
    /// unchanged, to the last bit.
    ///
    /// It crosses that boundary on every chunk: the activity returns a
    /// [`ReplayProgress`], the workflow deserializes it and serializes it back
    /// into the next chunk's input. Temporal's converter routes that through
    /// `serde_json::Value`, and serde_json's *default* float parser is fast
    /// rather than correctly rounded — it can land a single ULP away. One ULP
    /// is enough to break the property the whole repair design rests on: a
    /// replay is supposed to reproduce an untouched history exactly and write
    /// nothing, so a drifting μ would make every repair rewrite every rating,
    /// and each rewrite would then look like a re-score to the next
    /// redelivery.
    ///
    /// The fix is the `float_roundtrip` feature on `serde_json` in
    /// `Cargo.toml`, which reaches the SDK's own copy through Cargo's feature
    /// unification and is therefore invisible at every call site — hence this
    /// test. The values are a real pair observed drifting in a local repair
    /// run, not synthetic: `29.883225470630926` came back as
    /// `29.88322547063093`.
    #[test]
    fn a_rating_survives_the_temporal_payload_round_trip() {
        use serde::Deserialize;

        let state = ReplayState {
            matches_rated: 2,
            mu: 29.883225470630926,
            sigma: 7.824_035_397_027_08,
            last_rated_at: "2026-09-03T06:45:46.710Z".into(),
            // Non-empty on purpose: this set crosses the same boundary, and a
            // repair in flight when the worker restarts resumes from whatever
            // came back through it.
            moved_ahead: BTreeSet::from(["m1".to_string()]),
        };
        // Exactly what `temporalio-common-wasm`'s `SerdeJsonPayloadConverter`
        // does: serialize to bytes, parse to a `Value`, deserialize from that.
        let bytes = serde_json::to_vec(&state).expect("serialize");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("parse");
        let back = ReplayState::deserialize(value).expect("deserialize");

        assert_eq!(
            back, state,
            "a rating drifted crossing the workflow boundary: {} -> {}",
            state.mu, back.mu
        );
    }

    // -----------------------------------------------------------------------
    // The replay walk, against an in-memory store.
    // -----------------------------------------------------------------------

    /// An in-memory stand-in for the item collections [`replay_rating_chunk`]
    /// reads and rewrites.
    ///
    /// The two hazards it exists to reproduce are both about *sequencing*
    /// across several reads and writes, so neither can be written as a pure
    /// function: paging over a collection the walk is rewriting, and one
    /// delivery that has to both re-rate some participants and first-rate
    /// others. A real `Dao` would need DynamoDB.
    ///
    /// Faithful in exactly the three respects those depend on:
    ///
    /// - **Paging is DynamoDB's.** A page is the first `limit` items whose
    ///   sort key is strictly greater than the cursor, in key order, and the
    ///   cursor handed back is the last key of a *full* page — which is what
    ///   `LastEvaluatedKey` means. So an item rewritten beyond that key comes
    ///   round again and one rewritten before it does not, which is the whole
    ///   mechanism.
    /// - **A page is a snapshot.** The items are cloned out before the walk
    ///   sees them, so a rewrite mid-walk cannot change the page in hand.
    /// - **`apply_rating_contribution` moves the history item** the way the
    ///   real one does: write at the contribution's own key, and delete the
    ///   previous key when the two differ (`dao::rating`, step 4).
    ///
    /// What it deliberately does not model: the optimistic-lock guards (a
    /// single-threaded test never races), transactionality (nothing here fails
    /// part-way) and `attribute_exists` on the owner. One history map, not one
    /// per owner, because the only thing that writes history here is a replay,
    /// which only ever writes the owner it is repairing.
    #[derive(Default)]
    struct FakeStore {
        /// The owner's history collection, keyed by its rendered sort key —
        /// a `BTreeMap` because sort order is the entire point.
        history: RefCell<BTreeMap<String, RatingHistoryRecord>>,
        /// Match id → the record and its roster, rebuilt into a
        /// `MatchAggregate` on read.
        matches: RefCell<HashMap<String, (MatchRecord, Vec<MatchPlayerRecord>)>>,
        /// Match id → that match's `RATINGCONTRIB#` collection.
        contributions: RefCell<HashMap<String, Vec<RatingContributionRecord>>>,
        /// Owner id → their stored rating on the ladder under test. An owner
        /// absent from here has never been rated, which is what both entry
        /// points read as "start from the unrated default".
        ratings: RefCell<HashMap<String, RatingRecord>>,
    }

    impl RatingStore for FakeStore {
        async fn get_rating(
            &self,
            owner: &RatingOwner,
            _ladder: &str,
        ) -> DaoResult<Option<RatingRecord>> {
            Ok(self.ratings.borrow().get(&owner.id).cloned())
        }

        async fn batch_get_ratings(
            &self,
            user_ids: &[String],
            _ladder: &str,
        ) -> DaoResult<HashMap<String, RatingRecord>> {
            let ratings = self.ratings.borrow();
            Ok(user_ids
                .iter()
                .filter_map(|id| Some((id.clone(), ratings.get(id)?.clone())))
                .collect())
        }

        async fn list_rating_history(
            &self,
            _owner: &RatingOwner,
            _ladder: &str,
            cursor: Option<&str>,
            limit: u32,
        ) -> DaoResult<Page<RatingHistoryRecord>> {
            let history = self.history.borrow();
            let lower = match cursor {
                Some(c) => Bound::Excluded(c.to_string()),
                None => Bound::Unbounded,
            };
            let page: Vec<(String, RatingHistoryRecord)> = history
                .range((lower, Bound::Unbounded))
                .take(limit as usize)
                .map(|(key, entry)| (key.clone(), entry.clone()))
                .collect();
            // A cursor comes back only when the page was cut short by `limit`,
            // which is when DynamoDB sets `LastEvaluatedKey`.
            let next_cursor = (page.len() == limit as usize)
                .then(|| page.last().map(|(key, _)| key.clone()))
                .flatten();
            Ok(Page {
                items: page.into_iter().map(|(_, entry)| entry).collect(),
                next_cursor,
            })
        }

        async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>> {
            Ok(self
                .matches
                .borrow()
                .get(match_id)
                .map(|(record, players)| MatchAggregate {
                    sides: record.sides.values().cloned().collect(),
                    match_: record.clone(),
                    players: players.clone(),
                }))
        }

        async fn list_rating_contributions(
            &self,
            match_id: &str,
        ) -> DaoResult<Vec<RatingContributionRecord>> {
            Ok(self
                .contributions
                .borrow()
                .get(match_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn apply_rating_contribution(
            &self,
            match_id: &str,
            contribution: &RatingContributionRecord,
            stored: Option<&RatingContributionRecord>,
            rating: &RatingRecord,
            _stored_rating: Option<&RatingRecord>,
        ) -> DaoResult<()> {
            let mut contributions = self.contributions.borrow_mut();
            let for_match = contributions.entry(match_id.to_string()).or_default();
            for_match.retain(|c| c.owner_id != contribution.owner_id);
            for_match.push(contribution.clone());

            self.ratings
                .borrow_mut()
                .insert(contribution.owner_id.clone(), rating.clone());

            let mut history = self.history.borrow_mut();
            let key = contribution.history_sk(match_id).to_string();
            history.insert(key.clone(), contribution.history_entry(match_id));
            if let Some(previous) = stored {
                let previous_key = previous.history_sk(match_id).to_string();
                if previous_key != key {
                    history.remove(&previous_key);
                }
            }
            Ok(())
        }

        async fn withdraw_rating_contribution(
            &self,
            match_id: &str,
            stored: &RatingContributionRecord,
        ) -> DaoResult<()> {
            if let Some(for_match) = self.contributions.borrow_mut().get_mut(match_id) {
                for_match.retain(|c| c.owner_id != stored.owner_id);
            }
            self.history
                .borrow_mut()
                .remove(&stored.history_sk(match_id).to_string());
            Ok(())
        }
    }

    impl FakeStore {
        /// Seed one already-rated 1v1: the match, its roster, both players'
        /// contributions and the owner's history entry, all agreeing on
        /// `played_at`.
        ///
        /// The seeded movements are the unrated default rather than the
        /// numbers an incremental run would really have left. That is
        /// deliberate and it makes the test *harder*, not softer: every match
        /// then looks changed to the replay, so every one of them goes down
        /// the write path where the history key can move.
        fn seed_rated_match(&self, match_id: &str, played_at: &str, owner_id: &str, foe_id: &str) {
            let mut record = match_record();
            record.id = match_id.to_string();
            record.starts_at = played_at.to_string();
            self.matches.borrow_mut().insert(
                match_id.to_string(),
                (
                    record,
                    vec![
                        player("p1", Some(owner_id), Some("a")),
                        player("p2", Some(foe_id), Some("b")),
                    ],
                ),
            );

            let unmoved = RatingMovementRecord {
                mu_before: INITIAL_MU,
                sigma_before: INITIAL_SIGMA,
                mu_after: INITIAL_MU,
                sigma_after: INITIAL_SIGMA,
                display_delta: 0,
            };
            let contribution = |owner_id: &str, side_id: &str| RatingContributionRecord {
                owner_kind: RatingOwnerKindRecord::User,
                owner_id: owner_id.to_string(),
                ladder: "squash".to_string(),
                side_id: side_id.to_string(),
                played_at: played_at.to_string(),
                movement: unmoved,
                applied_at: "2026-06-01T12:00:00.000Z".to_string(),
            };
            let owner_contribution = contribution(owner_id, "a");
            self.history.borrow_mut().insert(
                owner_contribution.history_sk(match_id).to_string(),
                owner_contribution.history_entry(match_id),
            );
            self.contributions.borrow_mut().insert(
                match_id.to_string(),
                vec![owner_contribution, contribution(foe_id, "b")],
            );
        }

        /// Move a match's `starts_at` without touching its contribution or its
        /// history item — exactly the state a `PATCH { starts_at }` on a
        /// completed rated match leaves behind for the repair to find.
        fn reschedule(&self, match_id: &str, starts_at: &str) {
            let mut matches = self.matches.borrow_mut();
            let (record, _) = matches.get_mut(match_id).expect("a seeded match");
            record.starts_at = starts_at.to_string();
        }

        /// Put a participant on a match's roster without giving them a
        /// contribution — a player added to an already-rated match.
        fn add_player(&self, match_id: &str, player_id: &str, user_id: &str, side_id: &str) {
            let mut matches = self.matches.borrow_mut();
            let (_, players) = matches.get_mut(match_id).expect("a seeded match");
            players.push(player(player_id, Some(user_id), Some(side_id)));
        }

        /// Replace a match's stored contributions with the ones the engine
        /// really produces for its current roster, from unrated beliefs — i.e.
        /// what an honest first confirmation would have written.
        fn rate_as_stored(&self, match_id: &str, applied_at: &str) {
            let (record, players) = {
                let matches = self.matches.borrow();
                let (record, players) = matches.get(match_id).expect("a seeded match");
                (record.clone(), players.clone())
            };
            let rated = rateable(&record, &players).expect("the seeded match is rateable");
            let updates = rate_match(
                &rated.participants,
                rated.winner_side_id.as_deref(),
                &RatingTable::new(),
            )
            .expect("the seeded match rates");
            self.contributions.borrow_mut().insert(
                match_id.to_string(),
                updates
                    .iter()
                    .map(|u| contribution_for(u, &rated, RatingOwnerKindRecord::User, applied_at))
                    .collect(),
            );
        }

        /// One match's contributions, keyed by owner.
        fn contributions_by_owner(
            &self,
            match_id: &str,
        ) -> HashMap<String, RatingContributionRecord> {
            self.contributions
                .borrow()
                .get(match_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.owner_id.clone(), c))
                .collect()
        }
    }

    /// **Regression.** A participant added to an already-rated match used to
    /// be left with no rating for it at all, indefinitely.
    ///
    /// One delivery can carry both outcomes, and this is the shape that does:
    /// a third player joins the roster of a match the other two were already
    /// rated for. The two who were there have *moved* — the side they face
    /// gained a player, so their movements differ from what is stored — and
    /// need a replay; the new player has no contribution at all and needs a
    /// first apply. The handler used to return the moment anything needed a
    /// replay, and that dropped the second group on the floor.
    ///
    /// Repair could not cover for it, which is what makes this data loss
    /// rather than a delay: `RepairRatings` replays *stored history*, so it
    /// can only ever correct a contribution that already exists. Nothing
    /// creates a missing one but this path.
    ///
    /// The two who were already rated must still be left strictly alone —
    /// their stored contributions are the replay's input — so that is asserted
    /// as well, or "fold everything in unconditionally" would look like a fix.
    #[test]
    fn a_participant_added_to_a_rated_match_is_applied_even_when_the_others_need_a_replay() {
        let store = FakeStore::default();
        store.seed_rated_match("m1", "2026-06-01T10:00:00.000Z", "hero", "foe");
        // Honest stored contributions for the 1v1 it was when it was rated,
        // so the change detected below is the roster edit and nothing else.
        store.rate_as_stored("m1", "2026-06-01T12:00:00.000Z");
        let before = store.contributions_by_owner("m1");

        store.add_player("m1", "p3", "latecomer", "b");

        futures::executor::block_on(reconcile_match_rating(
            &store,
            None,
            "m1",
            "2026-06-02T09:00:00.000Z",
        ))
        .expect("the delivery reconciles");

        let after = store.contributions_by_owner("m1");
        assert!(
            after.contains_key("latecomer"),
            "the added participant must be rated for the match they played in: {:?}",
            after.keys().collect::<Vec<_>>()
        );
        assert_eq!(after["latecomer"].side_id, "b");
        assert_eq!(
            store
                .ratings
                .borrow()
                .get("latecomer")
                .map(|r| r.matches_rated),
            Some(1),
            "and carry the rating that apply produced"
        );

        for who in ["hero", "foe"] {
            assert_eq!(
                after[who], before[who],
                "{who} was already rated and is now stale — their contribution is the \
                 replay's input and must not be rewritten incrementally"
            );
        }
    }

    /// Drive a whole replay the way `RepairRatings` does — chunk, carry the
    /// state and the cursor, stop when the history is exhausted — and return
    /// the settled state.
    fn replay_whole_ladder(store: &FakeStore, owner: &RatingOwner) -> (ReplayState, usize) {
        let mut state = ReplayState::default();
        let mut cursor: Option<String> = None;
        let mut chunks = 0usize;
        loop {
            assert!(chunks < 10, "a replay of this size cannot need 10 chunks");
            let progress = futures::executor::block_on(replay_rating_chunk(
                store,
                owner,
                "squash",
                cursor.as_deref(),
                state,
                "2026-06-02T09:00:00.000Z",
            ))
            .expect("the replay runs");
            state = progress.state;
            chunks += 1;
            match progress.next_cursor {
                Some(next) => cursor = Some(next),
                None => return (state, chunks),
            }
        }
    }

    /// **Regression.** A match rescheduled since it was rated used to be
    /// folded *twice* by one replay, corrupting the rating permanently.
    ///
    /// The mechanism, and why the numbers below are what they are. The replay
    /// pages the owner's history with a DynamoDB cursor and rewrites that same
    /// collection as it walks: re-rating a rescheduled match writes its
    /// history item at the new `played_at` key and deletes the old one. Here
    /// the *first* match of `REPLAY_PAGE + 1` is moved to after the last, so
    /// its new key lands beyond the key page one ends on — and page two hands
    /// the walk the very same match again. It passes the `rateable` gate
    /// again, so it is folded again: `matches_rated` becomes
    /// `REPLAY_PAGE + 2`, and the owner's μ/σ absorb the result twice.
    ///
    /// What makes it worth a test of this size is that nothing afterwards
    /// notices. The second fold rewrites the contribution as well, so the
    /// movement the next incremental delivery recomputes matches the stored
    /// one exactly, `has_same_effect_as` reports "unchanged", and no repair is
    /// ever triggered. The corruption is stable and silent.
    ///
    /// `REPLAY_PAGE + 1` matches rather than a smaller history with a smaller
    /// page: the page size is the production constant, so the boundary this
    /// crosses is the real one and the test cannot quietly stop exercising it
    /// if `list_rating_history`'s paging changes shape.
    #[test]
    fn a_match_rescheduled_mid_replay_is_folded_once_not_twice() {
        let owner = RatingOwner::user("hero");
        let store = FakeStore::default();

        let total = REPLAY_PAGE as usize + 1;
        for i in 0..total {
            store.seed_rated_match(
                &format!("m{i:02}"),
                &format!("2026-06-01T10:{i:02}:00.000Z"),
                &owner.id,
                &format!("foe{i:02}"),
            );
        }
        // The organiser corrects the earliest match's date to after the
        // latest. `starts_at` is freely editable on a completed match, and the
        // resulting `#META` write is what starts the repair in the first place.
        store.reschedule("m00", "2026-06-01T23:00:00.000Z");

        let (state, chunks) = replay_whole_ladder(&store, &owner);

        assert!(
            chunks >= 2,
            "the walk must cross a page boundary or the bug cannot occur at all"
        );
        assert_eq!(
            state.matches_rated, total as u64,
            "every match counts exactly once; the rescheduled one was folded twice"
        );
        assert_eq!(
            store.history.borrow().len(),
            total,
            "the rescheduled match keeps one history item, at its new key"
        );
        assert_eq!(
            state.last_rated_at, "2026-06-01T23:00:00.000Z",
            "the rescheduled match is now the newest played"
        );
    }

    /// The ordinary case the guard must not disturb: an untouched history
    /// replays every match exactly once and moves no history keys.
    ///
    /// Same size and the same page boundary as the regression above, so the
    /// two differ in nothing but the reschedule — without this, a "fix" that
    /// skipped entries too eagerly would look like a pass.
    #[test]
    fn an_untouched_history_replays_every_match_exactly_once_across_pages() {
        let owner = RatingOwner::user("hero");
        let store = FakeStore::default();

        let total = REPLAY_PAGE as usize + 1;
        for i in 0..total {
            store.seed_rated_match(
                &format!("m{i:02}"),
                &format!("2026-06-01T10:{i:02}:00.000Z"),
                &owner.id,
                &format!("foe{i:02}"),
            );
        }

        let (state, _) = replay_whole_ladder(&store, &owner);

        assert_eq!(state.matches_rated, total as u64);
        assert!(
            state.moved_ahead.is_empty(),
            "nothing was rescheduled, so no history key can have moved"
        );
        assert_eq!(store.history.borrow().len(), total);
        assert_eq!(state.last_rated_at, "2026-06-01T10:50:00.000Z");
    }

    /// `applied_at` is a fresh wall clock on every delivery, so it must not
    /// take part in "has this match's effect changed?" — a match's `#META` is
    /// rewritten by every like and comment, so redelivery is the common case,
    /// and counting the clock would make each one look like a re-score.
    #[test]
    fn a_redelivery_differs_only_in_applied_at_and_so_has_the_same_effect() {
        let rated = rateable(&match_record(), &singles()).expect("rateable");
        let update = RatingUpdate {
            competitor_id: "u1".into(),
            before: PlayerRating::default(),
            after: PlayerRating {
                mu: 27.6,
                sigma: 7.1,
            },
        };
        let kind = RatingOwnerKindRecord::User;
        let first = contribution_for(&update, &rated, kind, "2026-06-02T09:00:00.000Z");
        let redelivered = contribution_for(&update, &rated, kind, "2026-06-09T18:22:00.000Z");
        assert!(first.has_same_effect_as(&redelivered));
        assert_ne!(first.applied_at, redelivered.applied_at);
    }
}
