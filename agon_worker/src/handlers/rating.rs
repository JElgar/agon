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
//! - **A change to an already-rated match is only *detected* here.** Re-score,
//!   roster edit, cancellation, ranked→friendly: all of them need a replay,
//!   which is `RepairRatings`' job (phase 2b-ii). This handler classifies and
//!   records; see [`note_repair_needed`], the seam that workflow plugs into.
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
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::rating::RatingOwner;
use agon_core::dao::records::{
    MatchPlayerRecord, MatchRecord, RatingContributionRecord, RatingMovementRecord,
    RatingOwnerKindRecord, RatingRecord,
};
use agon_core::rating::{
    Ladder, MatchParticipant, PlayerRating, RatingTable, RatingUpdate, ladder_for_tag, rate_match,
};

use crate::error::{WorkerError, WorkerResult};
use crate::event::ChangeEvent;

/// Handle a rating-relevant change event: any non-remove write to a match's
/// `#META`. Everything else is ignored.
pub async fn handle(dao: &Dao, ev: &ChangeEvent, now: &str) -> WorkerResult<()> {
    if ev.kind.is_remove() {
        return Ok(());
    }
    let (Pk::Match(match_id), Sk::Meta) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };
    reconcile_match_rating(dao, match_id, now).await
}

/// Reconcile a match's rating contributions against its current state.
///
/// Idempotent under at-least-once redelivery, and that is a property of the
/// comparison rather than of a lock: an unchanged match reproduces exactly the
/// contributions already stored (bit-for-bit — the engine is deterministic and
/// starts from the same stored `before` ratings), so the "nothing changed"
/// branch is reached without writing.
pub async fn reconcile_match_rating(dao: &Dao, match_id: &str, now: &str) -> WorkerResult<()> {
    // Re-read the current aggregate rather than trusting the stream image, for
    // the same reason `reconcile_match_stats` does: the image is a snapshot of
    // one item, and eligibility depends on the roster items too. A missing
    // match means there is nothing to attribute.
    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };
    let stored = dao.list_rating_contributions(match_id).await?;

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
            dao.withdraw_rating_contribution(match_id, contribution)
                .await?;
            note_repair_needed(
                RepairTrigger::Withdrawn,
                match_id,
                &owner,
                &contribution.ladder,
            );
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
        .filter(|p| !stored_by_owner.contains_key(p.user_id.as_str()))
        .map(|p| p.user_id.clone())
        .collect();
    let current = dao.batch_get_users(&need_current).await?;
    let current_rating = |user_id: &str| -> Option<RatingRecord> {
        current
            .get(user_id)
            .and_then(|u| u.ratings.get(rateable.ladder.as_str()))
            .cloned()
    };

    let mut base: RatingTable = RatingTable::new();
    for participant in &rateable.participants {
        let belief = match stored_by_owner.get(participant.user_id.as_str()) {
            Some(contribution) => PlayerRating {
                mu: contribution.movement.mu_before,
                sigma: contribution.movement.sigma_before,
            },
            None => current_rating(&participant.user_id)
                .map(|r| PlayerRating {
                    mu: r.mu,
                    sigma: r.sigma,
                })
                .unwrap_or_default(),
        };
        base.insert(participant.user_id.clone(), belief);
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

    // Pass one: classify, writing nothing. A match whose effect has changed
    // needs a replay of every affected ladder, not an incremental apply, and
    // that decision has to be made across the whole match before any of it is
    // written — half-applying a re-score would leave the two sides' ratings
    // disagreeing about what happened.
    let participant_ids: BTreeSet<&str> = rateable
        .participants
        .iter()
        .map(|p| p.user_id.as_str())
        .collect();
    let mut pending: Vec<(&RatingUpdate, RatingContributionRecord)> = Vec::new();
    let mut rerated: Vec<(RatingOwner, String)> = Vec::new();
    for update in &updates {
        let contribution = contribution_for(update, &rateable, now);
        match stored_by_owner.get(update.user_id.as_str()) {
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
    if !rerated.is_empty() {
        // Every affected owner is reported, not just the first: a re-score
        // moves both sides, and `RepairRatings` is keyed per owner and ladder,
        // so half the list would leave half the ladders wrong. Nothing is
        // written — the stored contributions are the replay's input and stay
        // exactly as they are until it runs.
        for (owner, ladder) in &rerated {
            note_repair_needed(RepairTrigger::Rerated, match_id, owner, ladder);
        }
        return Ok(());
    }

    // Pass two: apply the participants this match has not yet been applied to.
    // Usually that is all of them (a first confirmation); it is a strict
    // subset only when an earlier delivery got part-way through — each
    // participant is one transaction, so a crash or a lost optimistic-lock
    // race in the middle is recoverable rather than corrupting, and this is
    // where it recovers.
    for (update, contribution) in pending {
        let stored_rating = current_rating(&update.user_id);
        let owner = RatingOwner::user(&update.user_id);

        // Out-of-order arrival: this match was played *before* the newest one
        // already folded into this rating, so the incremental result is not
        // the one a `starts_at`-ordered replay would produce.
        if let Some(previous) = &stored_rating
            && previous.last_rated_at > rateable.played_at
        {
            note_repair_needed(
                RepairTrigger::OutOfOrder,
                match_id,
                &owner,
                rateable.ladder.as_str(),
            );
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
        dao.apply_rating_contribution(
            match_id,
            &contribution,
            None,
            &rating,
            stored_rating.as_ref(),
        )
        .await?;
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
            user_id: user_id.clone(),
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
/// Every field a repair needs to re-rate the match without the roster —
/// `side_id`, the incoming belief, the ladder — comes from here, which is why
/// this is one function and not a struct literal at each call site.
fn contribution_for(
    update: &RatingUpdate,
    rateable: &Rateable,
    now: &str,
) -> RatingContributionRecord {
    let side_id = rateable
        .participants
        .iter()
        .find(|p| p.user_id == update.user_id)
        .map(|p| p.side_id.clone())
        // Unreachable: every update comes from a participant. Falling back to
        // an empty side rather than panicking because a worker that panics
        // takes the whole consumer down, and an empty `side_id` fails the next
        // comparison loudly instead.
        .unwrap_or_default();
    RatingContributionRecord {
        // Users only for now. Teams carry ratings of their own and the DAO
        // writes for either owner kind, but nothing computes a team's rating
        // yet — a team is a *side*, not a participant, so rating one is a
        // second pass over the same match rather than a variation on this one.
        owner_kind: RatingOwnerKindRecord::User,
        owner_id: update.user_id.clone(),
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
    /// numbers are approximate.
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

/// Record that an owner's rating on a ladder needs replaying.
///
/// **This is the seam phase 2b-ii plugs into.** It will start a
/// `RepairRatings` Temporal workflow keyed `repair-<ownerId>-<ladder>`, whose
/// deterministic id makes a duplicate trigger a no-op — which is why this
/// function is free to be called more than once for the same owner and match.
///
/// It deliberately does not stub a workflow, or write a "pending repair" item,
/// or half-start anything: an inert stub is indistinguishable from working
/// machinery when read, and a pending-repair item would be a storage shape the
/// design has not chosen (phase 2a designed no key for one, and inventing one
/// now would have to be migrated away from when the workflow lands). A
/// structured log line is the honest amount of state for a phase that can
/// detect but not yet repair, and it goes out at `warn` so the condition is
/// visible in Loki rather than only in a debugger.
///
/// The repair it will start is **first-order**, and the code should say so
/// plainly rather than let a reader assume global consistency: replaying A's
/// ladder corrects A, but B — who played A — got a slightly wrong movement
/// from that match too, and so on transitively. Chasing the closure means
/// recomputing the whole graph on every correction. Second-order error is
/// heavily damped by σ at each hop, so first-order is the trade.
fn note_repair_needed(trigger: RepairTrigger, match_id: &str, owner: &RatingOwner, ladder: &str) {
    tracing::warn!(
        trigger = ?trigger,
        match_id,
        owner_kind = ?owner.kind,
        owner_id = %owner.id,
        ladder,
        "rating repair needed; RepairRatings is not implemented yet (phase 2b-ii)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use agon_core::dao::records::{
        ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationKindRecord, MatchSideRecord,
        ScoreRecord,
    };
    use std::collections::HashMap;

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
                    user_id: "u1".into(),
                    side_id: "a".into()
                },
                MatchParticipant {
                    user_id: "u2".into(),
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
        assert!(rated.participants.iter().all(|p| p.user_id != "u3"));
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
            user_id: "u2".into(),
            before: PlayerRating {
                mu: 25.0,
                sigma: 25.0 / 3.0,
            },
            after: PlayerRating {
                mu: 22.0,
                sigma: 7.0,
            },
        };
        let contribution = contribution_for(&update, &rated, "2026-06-02T09:00:00.000Z");
        assert_eq!(contribution.owner_kind, RatingOwnerKindRecord::User);
        assert_eq!(contribution.owner_id, "u2");
        assert_eq!(contribution.ladder, "squash");
        assert_eq!(contribution.side_id, "b");
        assert_eq!(contribution.played_at, rated.played_at);
        assert_eq!(contribution.movement.mu_before, 25.0);
        assert_eq!(contribution.movement.mu_after, 22.0);
        assert_eq!(contribution.movement.display_delta, -90);
    }

    /// `applied_at` is a fresh wall clock on every delivery, so it must not
    /// take part in "has this match's effect changed?" — a match's `#META` is
    /// rewritten by every like and comment, so redelivery is the common case,
    /// and counting the clock would make each one look like a re-score.
    #[test]
    fn a_redelivery_differs_only_in_applied_at_and_so_has_the_same_effect() {
        let rated = rateable(&match_record(), &singles()).expect("rateable");
        let update = RatingUpdate {
            user_id: "u1".into(),
            before: PlayerRating::default(),
            after: PlayerRating {
                mu: 27.6,
                sigma: 7.1,
            },
        };
        let first = contribution_for(&update, &rated, "2026-06-02T09:00:00.000Z");
        let redelivered = contribution_for(&update, &rated, "2026-06-09T18:22:00.000Z");
        assert!(first.has_same_effect_as(&redelivered));
        assert_ne!(first.applied_at, redelivered.applied_at);
    }
}
