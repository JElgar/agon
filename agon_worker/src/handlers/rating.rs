//! Inline handler: rate the players of a match once its score is confirmed.
//!
//! Hooks the same trigger as [`crate::handlers::stats`] — any non-remove write
//! to a match's `#META`, which is what promoting a score to `confirmed_score`
//! produces — and has the same reconcile-by-diff shape: it works out what the
//! match *currently* implies for each player, compares that with the
//! `RATINGCONTRIB#` items already stored, and writes only the difference. An
//! unchanged redelivery therefore writes nothing, which matters more here than
//! it does for stats: every like and comment rewrites `#META`, so redelivery of
//! a finished match is the common case, not the rare one.
//!
//! ## Why this is not `reconcile_match_stats` with different arithmetic
//!
//! Stats are a *sum*, so a stale contribution can be backed out by subtracting
//! it. A Weng-Lin update is not invertible — what a match did to you depends on
//! every opponent's σ at that moment — so the only way to correct history is to
//! replay it. That one fact shapes everything below:
//!
//! - **A first rating is applied here, incrementally and authoritatively**, per
//!   player rather than per match. A roster edit can add somebody to a match
//!   everyone else was already rated for, and their first rating is applied
//!   here like anyone's. A replay cannot stand in for it: it walks *stored*
//!   history, so it can correct a contribution but never create a missing one.
//! - **A change to a rating already applied is only *detected* here.** A
//!   re-score, a reschedule, a roster change that moved a side's strength, or a
//!   match played before one already rated all need the player's ladder
//!   replayed, which is `RepairRatings`' job and not built yet. This handler
//!   classifies and hands off through [`note_repair_needed`], the one place
//!   that replay will be started from.
//! - **"Has anything changed?" is answered from the stored contributions, not
//!   from the players' current ratings.** `dao::rating`'s module header spells
//!   out the double count the obvious version walks into, and this handler is
//!   that header's protocol, step for step.
//!
//! ## Who is rated
//!
//! Everyone who played in a **ranked** match, and nobody else. It counts if
//! it is ranked, completed, its score is agreed by every side, and its sport
//! has a ladder. There is nothing further to check, because playing a ranked
//! match is the whole of the agreement: no participant has to be refused,
//! chased for consent or stood in for with a placeholder prior, so a ringer
//! is just a player. What is withheld, and only for five matches, is whether
//! the resulting number is shown (`rating::placement`).
//!
//! A friendly rates nobody at all — not even the players who would quite
//! like it to. Weng-Lin is approximately conservative, so what one side
//! gains the other loses; writing the movement for some of a match's players
//! and not the rest hands the pool rating with no counterpart, in exactly
//! the matches whose scores nobody checks. Either everybody in a match
//! absorbs the result or nobody does.
//!
//! Two things still leave a ranked match unrated for everybody. A **guest with no
//! account on a side** has nothing to key a rating to, and Weng-Lin sums a
//! side's beliefs, so rating around them would misattribute their part of the
//! result; an unassigned guest played for no side and is simply ignored. And
//! **one account in two spots on a side** would produce two updates that
//! disagree about its own rating. See [`rateable`].
//!
//! ## Ordering
//!
//! Ratings are meant to be applied in played (`starts_at`) order, not
//! confirmation order — a Monday game confirmed on Thursday belongs before
//! Wednesday's. Confirmation order is what the stream delivers, so arriving
//! out of order is normal rather than exceptional, and detecting it is
//! [`RepairTrigger::OutOfOrder`]. The result is still applied when that
//! happens; that variant's doc says why refusing would be worse.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use agon_core::dao::Dao;
use agon_core::dao::error::DaoResult;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::match_ops::MatchAggregate;
use agon_core::dao::records::{
    MatchPlayerRecord, MatchRecord, RatingContributionRecord, RatingMovementRecord, RatingRecord,
    UserRecord,
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
    let done = reconcile_match_rating(dao, match_id, now).await?;
    // Only a delivery that wrote something is worth a line. Rating a match
    // happens once per player per match, so this is quiet; the deliveries
    // that write nothing (every like on every match) say nothing.
    if !done.applied.is_empty() || !done.withdrawn.is_empty() {
        tracing::info!(
            match_id,
            applied = ?done.applied,
            withdrawn = ?done.withdrawn,
            repairs_needed = done.repairs.len(),
            "match ratings reconciled"
        );
    }
    Ok(())
}

/// Reconcile a match's rating contributions with its current state.
///
/// Idempotent under at-least-once redelivery, and that is a property of the
/// comparison rather than of a lock: an unchanged match reproduces exactly the
/// contributions already stored (bit for bit — the engine is deterministic and
/// starts from the same stored `before` beliefs), so the "nothing changed"
/// branch is reached without writing. The writes it does make are each guarded
/// on the state read here (`apply_rating_contribution`'s optimistic lock), so
/// two deliveries racing on one match cost the loser a `Conflict` and a
/// redelivery, never a double count.
async fn reconcile_match_rating<S: RatingStore>(
    store: &S,
    match_id: &str,
    now: &str,
) -> WorkerResult<Reconciled> {
    let mut done = Reconciled::default();

    // Re-read the current aggregate rather than trusting the stream image, for
    // the reason `reconcile_match_stats` does: the image is one item, and who
    // is rated depends on the roster items too. A missing match means there is
    // nothing to rate.
    let Some(agg) = store.get_match(match_id).await? else {
        return Ok(done);
    };
    let stored = store.list_rating_contributions(match_id).await?;

    // The gate. The match and roster cost nothing and turn away every
    // friendly and everything unfinished before a single profile is read,
    // which is the overwhelming majority of deliveries — a like on *any*
    // match lands here.
    let (rateable, users) = match rateable(&agg.match_, &agg.players) {
        Ok(rateable) => {
            let users = store.batch_get_users(&rateable.user_ids()).await?;
            (rateable, users)
        }
        Err(not_rated) => {
            log_not_rated(match_id, &not_rated);
            // Not rated. If it never was, nothing is stored and there is
            // nothing to do. If it *was* — cancelled, or somebody who can't be
            // rated now holds a spot — every stored contribution describes a
            // match that no longer counts, so all of them come out, rather
            // than rating a subset. Withdrawal happens now, not at the repair,
            // because history items are the replay's *input*: left in place, a
            // replay would faithfully re-apply a match that no longer counts.
            for contribution in &stored {
                done.withdraw(store, match_id, contribution).await?;
            }
            return Ok(done);
        }
    };
    let ladder = rateable.ladder.as_str();

    let stored_by_user: BTreeMap<&str, &RatingContributionRecord> =
        stored.iter().map(|c| (c.user_id.as_str(), c)).collect();
    let current_rating = |user_id: &str| -> Option<RatingRecord> {
        users
            .get(user_id)
            .and_then(|user| user.ratings.get(ladder))
            .cloned()
    };
    // The belief each participant is rated *from*. For anyone this match has
    // already been applied to, that is the rating they carried into it, which
    // the contribution preserves precisely so a redelivery recomputes the same
    // movement instead of compounding one. For anyone else it is their current
    // rating, or the unrated default for a first match on this ladder.
    let mut base = RatingTable::new();
    for participant in &rateable.participants {
        let belief = match stored_by_user.get(participant.competitor_id.as_str()) {
            Some(contribution) => PlayerRating {
                mu: contribution.movement.mu_before,
                sigma: contribution.movement.sigma_before,
            },
            // No stored contribution, so this is their current rating — or,
            // for a first match on this ladder, the unrated default, which is
            // `INITIAL_MU` at `INITIAL_SIGMA`. That default is why nothing
            // has to be seeded when somebody plays their first ranked game:
            // "we have no idea how good you are" is a belief the engine can
            // already hold.
            None => current_rating(&participant.competitor_id)
                .map(|r| PlayerRating {
                    mu: r.mu,
                    sigma: r.sigma,
                })
                .unwrap_or_default(),
        };
        base.insert(participant.competitor_id.clone(), belief);
    }

    // Any error left out of the engine is a state the write path is supposed
    // to make impossible: `rateable` has already turned away every shape that
    // is merely unusual (a side with nobody in a spot, one account twice).
    // What remains is corruption, such as a `winner_side_id` naming a side the
    // match doesn't have, and the engine is emphatic that it must fail loudly
    // rather than rate wrongly (silently, it would score it as a draw). An
    // `Invariant` parks the message in the DLQ, which is why this handler runs
    // last in `handlers::route`.
    let updates = rate_match(
        &rateable.participants,
        rateable.winner_side_id.as_deref(),
        &base,
    )
    .map_err(|e| WorkerError::Invariant(format!("match {match_id} cannot be rated: {e}")))?;

    // Pass one: classify every player, writing nothing. A changed movement
    // under an existing rating needs that ladder replayed, not an incremental
    // apply, and the decision is made across the whole match before any of it
    // is written: half-applying a re-score would leave the two sides'
    // ratings disagreeing about what happened.
    //
    // The outcomes are not exclusive, and one delivery can carry all three.
    // Add a player to a rated match and the players already there have moved
    // (a side gained strength) while the newcomer has nothing stored; remove
    // one and the rest have moved while the one removed no longer counts. All
    // of them are acted on below. An earlier version returned as soon as
    // anybody needed a replay, which stranded the newcomer: a replay can only
    // correct a contribution that exists, so they were rated only if some
    // unrelated later write happened to re-run this handler.
    // Who this match writes to: everybody in it. Collected as a set for the
    // withdrawal below, which is the only thing that needs to ask the
    // question the other way round.
    let participant_ids: BTreeSet<&str> = rateable
        .participants
        .iter()
        .map(|p| p.competitor_id.as_str())
        .collect();
    let mut pending: Vec<(&RatingUpdate, RatingContributionRecord)> = Vec::new();
    let mut rerated: Vec<&RatingContributionRecord> = Vec::new();
    for update in &updates {
        let contribution = contribution_for(update, &rateable, now);
        match stored_by_user.get(update.competitor_id.as_str()) {
            Some(previous) if previous.has_same_effect_as(&contribution) => {}
            // The match changed under an existing rating: a re-score, a
            // reschedule, or a roster edit that moved a side's strength.
            Some(previous) => rerated.push(previous),
            None => pending.push((update, contribution)),
        }
    }
    // A stored contribution this match no longer implies: the player was
    // removed, lost their side, or no longer takes a spot. Withdrawn
    // (`dao::rating`'s protocol, step 5), and the rating it leaves stale is
    // the repair's. A match switched to friendly takes a different route —
    // `rateable` refuses it outright and every contribution comes out above.
    for contribution in stored
        .iter()
        .filter(|c| !participant_ids.contains(c.user_id.as_str()))
    {
        done.withdraw(store, match_id, contribution).await?;
    }

    // Pass two: apply the players this match has not been applied to yet.
    // Usually that is everyone (a first confirmation). It is a strict subset
    // when an earlier delivery got part-way through — each player is their own
    // transaction, so a crash or a lost optimistic-lock race in the middle is
    // recoverable rather than corrupting, and this is where it recovers — or
    // when a roster edit added somebody to a match already rated.
    //
    // Nothing here touches a `rerated` player's stored contribution. Those
    // stay exactly as they are until the replay runs, because they are its
    // input: the invariant is "never rewrite a contribution incrementally",
    // not "never write at all".
    let mut out_of_order: Vec<&str> = Vec::new();
    for (update, contribution) in pending {
        let user_id = update.competitor_id.as_str();
        let stored_rating = current_rating(user_id);

        // Played before the newest match already folded into this rating, so
        // the result is not the one a played-order replay would produce.
        // Collected, not flagged yet; see below.
        if let Some(previous) = &stored_rating
            && previous.last_rated_at > rateable.played_at
        {
            out_of_order.push(user_id);
        }

        let rating = RatingRecord {
            mu: update.after.mu,
            sigma: update.after.sigma,
            // Every rated match is a ranked one, so this single counter is
            // both the games-played figure and the placement gate
            // (`rating::published`).
            matches_rated: stored_rating.as_ref().map_or(0, |r| r.matches_rated) + 1,
            // The `starts_at` of the newest match folded in: a *maximum*, not
            // the match just written. Rating a Monday game after a Wednesday
            // one must not move this backwards, or every later match would
            // look out of order too.
            last_rated_at: stored_rating
                .as_ref()
                .map(|r| r.last_rated_at.clone())
                .filter(|previous| previous > &rateable.played_at)
                .unwrap_or_else(|| rateable.played_at.clone()),
        };

        // `stored = None`: only players with no stored contribution reach
        // here, so the write is guarded on there still being none, and two
        // concurrent deliveries can't both apply it.
        store
            .apply_rating_contribution(
                match_id,
                &contribution,
                None,
                &rating,
                stored_rating.as_ref(),
            )
            .await?;
        done.applied.push(user_id.to_string());
    }

    // Out-of-order and re-rated players are flagged only once every apply
    // above has committed. While the flag is a log line the order changes
    // nothing, but it is chosen for the replay that replaces it, which reads
    // the history collection as it stands when it runs: the history entry an
    // out-of-order match is about is written by the loop above, so starting
    // the replay first would routinely hand it a history without that match,
    // and it would find nothing to fix. Withdrawals are the opposite way round
    // (see `Reconciled::withdraw`), deliberately.
    for user_id in out_of_order {
        done.repair(RepairTrigger::OutOfOrder, match_id, user_id, ladder);
    }
    // Every affected player is flagged, not just the first: a re-score moves
    // both sides, and a replay is per player per ladder. The stored ladder,
    // because that is the pool holding the movement a replay has to undo.
    for previous in rerated {
        done.repair(
            RepairTrigger::Rerated,
            match_id,
            &previous.user_id,
            &previous.ladder,
        );
    }

    Ok(done)
}

/// What one delivery did: who it applied, who it withdrew, and which ladders
/// it found in need of a replay.
///
/// Returned rather than only logged so the tests can assert what was
/// *detected*. The repair seam is a log line until `RepairRatings` exists, and
/// a log line is not something `cargo test` can see.
#[derive(Debug, Default)]
struct Reconciled {
    /// Players given a rating for this match by this delivery.
    applied: Vec<String>,
    /// Players whose contribution for this match this delivery withdrew.
    withdrawn: Vec<String>,
    repairs: Vec<RepairNeeded>,
}

/// One ladder [`note_repair_needed`] was told about.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RepairNeeded {
    trigger: RepairTrigger,
    user_id: String,
    ladder: String,
}

impl Reconciled {
    /// Flag a player's ladder for a replay, and record that it was flagged.
    /// The only caller of [`note_repair_needed`], so replacing that is one
    /// change.
    fn repair(&mut self, trigger: RepairTrigger, match_id: &str, user_id: &str, ladder: &str) {
        let need = RepairNeeded {
            trigger,
            user_id: user_id.to_string(),
            ladder: ladder.to_string(),
        };
        note_repair_needed(need.trigger, match_id, &need.user_id, &need.ladder);
        self.repairs.push(need);
    }

    /// Withdraw one stored contribution, flagging its ladder first.
    ///
    /// Flag first, delete second. While the flag is a log line that is
    /// harmless either way; once it starts a replay it is load-bearing, and
    /// the opposite of the order out-of-order and re-rated players use. A
    /// replay started *after* the delete would be lost outright if starting it
    /// failed: the message redelivers, finds the contribution already gone,
    /// and has nothing left to flag, so the rating would keep this match's
    /// effect for good. Starting first costs nothing, because a replay decides
    /// what counts from the match itself, which is already committed as not
    /// counting.
    async fn withdraw<S: RatingStore>(
        &mut self,
        store: &S,
        match_id: &str,
        contribution: &RatingContributionRecord,
    ) -> WorkerResult<()> {
        self.repair(
            RepairTrigger::Withdrawn,
            match_id,
            &contribution.user_id,
            &contribution.ladder,
        );
        store
            .withdraw_rating_contribution(match_id, contribution)
            .await?;
        self.withdrawn.push(contribution.user_id.clone());
        Ok(())
    }
}

/// A match that passed the match-and-roster half of the gate, projected into
/// what the engine needs.
///
/// Built only by [`rateable`]. Holding one means the match is ranked,
/// completed, agreed by every side, on a real ladder, and has a roster the
/// engine cannot fail on short of corrupt data — which, since nobody has to
/// consent, is the whole of the question. The keys it collects are what the
/// profile read that follows asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rateable {
    ladder: Ladder,
    /// The match's `starts_at` — the order ratings are meant to be applied
    /// in, and the ordering segment of every history item written from it.
    played_at: String,
    winner_side_id: Option<String>,
    /// The players rated: everyone in a spot who is on a side.
    ///
    /// Unassigned players are absent entirely. They played for no side, so
    /// they take no part in the result and the engine has nowhere to put
    /// them — and unlike stats, which can record a player's own row in
    /// isolation, a rating is relative, so there is no "their own" part of
    /// this to record.
    participants: Vec<MatchParticipant>,
}

impl Rateable {
    /// The accounts to batch-read: every participant, whose stored rating is
    /// the belief this match is rated from.
    fn user_ids(&self) -> Vec<String> {
        self.participants
            .iter()
            .map(|p| p.competitor_id.clone())
            .collect()
    }
}

/// Why a match isn't rated.
///
/// The first four are about the match and are routine — every friendly and
/// every unfinished match lands on one of them on every like. The rest are
/// about the roster of a match that would otherwise be rated, and are never
/// routine; see [`NotRated::warrants_warning`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum NotRated {
    /// Not a ranked match, so it moves nobody.
    Friendly,
    NotCompleted,
    Unconfirmed,
    NoLadder,
    /// Players on a side with no account, by `player_id`.
    UnlinkedPlayers {
        player_ids: Vec<String>,
    },
    /// One account on two sides, or twice on one.
    DuplicateAccount {
        user_id: String,
    },
    /// Fewer than two sides.
    TooFewSides(usize),
    /// A side with nobody in a spot on it.
    EmptySide {
        side_id: String,
    },
}

impl NotRated {
    /// Whether this refusal deserves a warning.
    ///
    /// Only a completed, agreed match on a real ladder that still isn't rated
    /// is surprising, and it is the case somebody will come asking about ("we
    /// both confirmed, why didn't my rating move?"). The routine refusals must
    /// stay silent: `#META` is rewritten by every like and comment on every
    /// match, so warning on an unfinished one would bury the ones that matter.
    fn warrants_warning(&self) -> bool {
        !matches!(
            self,
            NotRated::Friendly
                | NotRated::NotCompleted
                | NotRated::Unconfirmed
                | NotRated::NoLadder
        )
    }
}

/// The match-and-roster half of the gate, and the projection into
/// participants. `Err` says why the match doesn't count towards anybody's
/// rating.
///
/// One function rather than a gate and a separate projection: nearly every
/// rule is a statement about the roster, and splitting them would walk the
/// players twice under two subtly different ideas of "in the match". The
/// engine takes loose participants rather than records because an 11-a-side
/// roster only exists as separate `MATCH#/PLAYER#` items (see `rating`'s
/// module doc), and this is the adapter that doc defers to.
///
/// The rules, and what each protects against:
///
/// - **`ranked`.** A friendly moves nobody. Checked first and before anything
///   is read, because it is the cheapest answer and the commonest one. It is
///   also what makes switching a rated match to friendly correct rather than
///   merely detected: the match stops being rateable for everybody at once,
///   and the caller withdraws every contribution it wrote.
/// - **`status == "completed"`.** A cancelled or unplayed match has no result.
/// - **A `confirmed_score`.** Its presence *is* "every side agreed": its only
///   writer promotes a pending score once every side has confirmed it, and it
///   carries no confirmations of its own to re-check. A pending or disputed
///   score is one side's claim, and rating it would move an opponent's number
///   on something they haven't accepted.
/// - **A ladder for the sport.** `other` is unrated on purpose
///   (`rating::ladder_for`).
/// - **Only players who take a spot count** ([`MatchPlayerRecord::occupies_slot`],
///   the same rule the roster counts and the stats reconciler use). A pending
///   or declined invitee and anyone on the waitlist didn't play, so they
///   neither rate nor block, even when they have no account: an external
///   invite on a ranked match is allowed, and until somebody accepts it the
///   guest takes no spot. Sharing the stats rule also keeps `matches_rated`
///   from disagreeing with `matches_played` about who played.
/// - **Everyone *on a side* has an account.** An unlinked player has no rating
///   to give or take, and Weng-Lin sums a side's beliefs, so rating around
///   them would quietly credit their part in the result to whoever else was on
///   their side. One blocks the whole match. Judged on side membership rather
///   than on holding a spot: a guest who was never assigned played for nobody,
///   so there is no side whose strength they distort and nothing to refuse.
///   This is the one unrated participant the design does not solve, and it is
///   a known limitation rather than a decision — see the project notes.
/// - **No account on two sides, or twice on one.** Its two updates would
///   disagree about its own rating. The stats reconciler merges duplicates by
///   taking the best outcome; there is no "best" of two Gaussians, so the
///   match waits for the roster to be fixed. Also judged on side membership,
///   for the same reason: an account that holds a spot *and* appears
///   unassigned is rated exactly once, so there is no conflict to resolve.
/// - **At least two sides, each with somebody in a spot on it.** Judged against
///   the match's declared sides, not against the sides the participants happen
///   to cover, so a side whose players are all still pending fails the match
///   rather than turning a 1v1 into a 1v0.
///
/// Two judgement calls, because neither is obviously right:
///
/// 1. **An unassigned player in a spot is ignored entirely** — not rated, and
///    not a reason to refuse. They were on no side of the result, so there is
///    no side for the engine to put them on and no relative claim to record.
///    They used to be checked here, back when holding a spot obliged you to
///    have consented; nothing is gated on consent now, so checking a group
///    wider than the one being rated would only find reasons to refuse
///    matches that are perfectly rateable.
///
///    The stats reconciler does the same thing for the same reason (it gives
///    an unassigned player no outcome rather than a loss), so the two agree
///    about who played.
/// 2. **An uneven roster is rated as recorded.** Seven names logged for an
///    11-a-side side rate as a 7v11, and Weng-Lin reads the seven as heavy
///    underdogs. Refusing uneven sides was rejected: a genuinely short side is
///    common in amateur sport and *should* rate as the upset it was, nothing
///    stored tells it apart from an incomplete roster, and blocking would
///    refuse real results for good where a mis-weighted one is bounded by σ
///    and fixable by replay.
fn rateable(m: &MatchRecord, players: &[MatchPlayerRecord]) -> Result<Rateable, NotRated> {
    if !m.ranked {
        return Err(NotRated::Friendly);
    }
    if m.status != "completed" {
        return Err(NotRated::NotCompleted);
    }
    let Some(confirmed) = &m.confirmed_score else {
        return Err(NotRated::Unconfirmed);
    };
    let Some(ladder) = ladder_for_tag(&m.match_type) else {
        return Err(NotRated::NoLadder);
    };

    let mut participants: Vec<MatchParticipant> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut unlinked: Vec<String> = Vec::new();
    // Only players on a side. Someone in a spot with no side yet played for
    // neither, so they are neither rated nor a reason to refuse — the walk
    // skips them before either check, which is what makes an unassigned guest
    // harmless where a guest on a side is not.
    for player in players.iter().filter(|p| p.occupies_slot()) {
        let Some(side_id) = &player.side_id else {
            continue;
        };
        let Some(user_id) = &player.user_id else {
            unlinked.push(player.player_id.clone());
            continue;
        };
        if !seen.insert(user_id) {
            return Err(NotRated::DuplicateAccount {
                user_id: user_id.clone(),
            });
        }
        participants.push(MatchParticipant {
            competitor_id: user_id.clone(),
            side_id: side_id.clone(),
        });
    }
    if !unlinked.is_empty() {
        return Err(NotRated::UnlinkedPlayers {
            player_ids: unlinked,
        });
    }

    if m.sides.len() < 2 {
        return Err(NotRated::TooFewSides(m.sides.len()));
    }
    let covered: BTreeSet<&str> = participants.iter().map(|p| p.side_id.as_str()).collect();
    // The smallest uncovered id, so the reason logged is the same on every
    // delivery whatever order the sides map iterates in.
    if let Some(side_id) = m
        .sides
        .keys()
        .filter(|side_id| !covered.contains(side_id.as_str()))
        .min()
    {
        return Err(NotRated::EmptySide {
            side_id: side_id.clone(),
        });
    }

    Ok(Rateable {
        ladder,
        played_at: m.starts_at.clone(),
        winner_side_id: confirmed.winner_side_id.clone(),
        participants,
    })
}

/// Warn about a match that isn't rated, when that is surprising (see
/// [`NotRated::warrants_warning`]). Once per delivery, so a blocked match that
/// keeps collecting likes keeps saying so; that is rare, and it is the point.
fn log_not_rated(match_id: &str, not_rated: &NotRated) {
    if !not_rated.warrants_warning() {
        return;
    }
    match not_rated {
        NotRated::Friendly
        | NotRated::NotCompleted
        | NotRated::Unconfirmed
        | NotRated::NoLadder => {}
        NotRated::UnlinkedPlayers { player_ids } => tracing::warn!(
            match_id,
            player_ids = ?player_ids,
            "match not rated: a player on a side has no account"
        ),
        NotRated::DuplicateAccount { user_id } => tracing::warn!(
            match_id,
            user_id = %user_id,
            "match not rated: one account plays for two sides"
        ),
        NotRated::TooFewSides(sides) => {
            tracing::warn!(match_id, sides, "match not rated: fewer than two sides")
        }
        NotRated::EmptySide { side_id } => tracing::warn!(
            match_id,
            side_id = %side_id,
            "match not rated: a side has nobody in a spot"
        ),
    }
}

/// The contribution record one rating update implies for this match.
///
/// Every field change detection compares — the side, the incoming belief, the
/// ladder, the played time — is set here, which is why this is one function
/// rather than a struct literal at each call site.
fn contribution_for(
    update: &RatingUpdate,
    rateable: &Rateable,
    now: &str,
) -> RatingContributionRecord {
    let side_id = rateable
        .participants
        .iter()
        .find(|p| p.competitor_id == update.competitor_id)
        .map(|p| p.side_id.clone())
        // Unreachable: every update comes from a participant. An empty side
        // rather than a panic, because a panicking worker takes the whole
        // consumer down, and an empty `side_id` fails the next comparison
        // loudly instead.
        .unwrap_or_default();
    RatingContributionRecord {
        user_id: update.competitor_id.clone(),
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

/// Why a rating can't be brought up to date incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairTrigger {
    /// A match was rated after a *later* one had already been folded in. The
    /// numbers are self-consistent, but not the ones a played-order replay
    /// produces.
    ///
    /// The result is applied anyway, and that is worth defending. Refusing to
    /// apply it would leave the match with no history item, and history items
    /// are exactly what a replay reads, so the match would be invisible to the
    /// very repair meant to fix it: permanently unrated. Applying puts it in
    /// the collection at its correct `starts_at` key, where a replay finds it
    /// in the right place; only the interim numbers are approximate. The same
    /// fact fixes when a repair may be started: not before that history entry
    /// exists.
    OutOfOrder,
    /// An applied rating's movement changed: a re-score, a reschedule, or a
    /// roster change that moved a side's strength. Weng-Lin isn't invertible,
    /// so this can't be applied as a delta.
    Rerated,
    /// A contribution was withdrawn: the match stopped counting (cancelled,
    /// or somebody who can't be rated took a spot), or the player no longer
    /// takes part. The stored rating still contains its effect until a replay
    /// removes it.
    Withdrawn,
}

/// Record that a player's rating on a ladder needs replaying.
///
/// **This is the seam `RepairRatings` plugs into.** It will start that
/// workflow, keyed per player and ladder, so a deterministic workflow id makes
/// a duplicate trigger a no-op — which is why this is free to be called more
/// than once for one player and match, as every redelivery of a changed match
/// does until the replay has run.
///
/// It deliberately does not stub a workflow, write a "pending repair" item or
/// half-start anything. An inert stub reads as working machinery, and a
/// pending-repair item would be a storage shape the design has no key for,
/// which would then have to be migrated away from. A structured log line is the
/// honest amount of state for a pipeline that can detect but not yet repair,
/// and it goes out at `warn` so the condition is visible rather than only in a
/// debugger.
///
/// The repair it will start is **first-order**: replaying A's ladder corrects
/// A, but B, who played A, got a slightly wrong movement from that match too,
/// and so on transitively. Chasing the closure would mean recomputing the whole
/// graph on every correction. σ damps the error at each hop, so first-order is
/// the trade.
fn note_repair_needed(trigger: RepairTrigger, match_id: &str, user_id: &str, ladder: &str) {
    tracing::warn!(
        trigger = ?trigger,
        match_id,
        user_id,
        ladder,
        "rating repair needed; RepairRatings is not built yet"
    );
}

/// The reads and writes [`reconcile_match_rating`] makes, as a seam.
///
/// The gate and the projection are pure functions and are tested as such. The
/// reconcile is not, and the bugs it has had are bugs of *sequencing across
/// several reads and writes*, which no pure function can express: one delivery
/// carrying both a first rating and a re-rating (where handling only the second
/// stranded the first), and a match left part-applied by a failed transaction.
/// Both need a store that really stores, and `Dao` needs DynamoDB, which
/// `cargo test` doesn't have. So there are exactly two implementations: `Dao`,
/// which ships, and an in-memory model in this module's tests.
///
/// As narrow as this handler's use of `Dao`, because a fake can only be honest
/// about the surface it stands in for. `batch_get_users` hands back whole
/// records rather than a projection of the `ratings` map, so the fake stands
/// in for the read rather than for an interpretation of it.
//
// `async fn` in a private trait, with one production implementor and every
// call site concrete, so the futures are still `Send` by auto-trait leakage.
trait RatingStore {
    async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>>;

    async fn list_rating_contributions(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<RatingContributionRecord>>;

    async fn batch_get_users(&self, user_ids: &[String]) -> DaoResult<HashMap<String, UserRecord>>;

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

/// The production binding: straight delegation, no behaviour of its own. If a
/// method here ever grows a body, the fake has stopped standing for the real
/// thing and the regressions it guards have stopped being guarded.
impl RatingStore for Dao {
    async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>> {
        Dao::get_match(self, match_id).await
    }

    async fn list_rating_contributions(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<RatingContributionRecord>> {
        Dao::list_rating_contributions(self, match_id).await
    }

    async fn batch_get_users(&self, user_ids: &[String]) -> DaoResult<HashMap<String, UserRecord>> {
        Dao::batch_get_users(self, user_ids).await
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use agon_core::dao::error::DaoError;
    use agon_core::dao::records::{
        ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationKindRecord, MatchSideRecord,
        RatingHistoryRecord, ScoreRecord, UserStatsRecord,
    };
    use agon_core::rating::{INITIAL_MU, INITIAL_SIGMA};

    use super::*;

    const PLAYED_AT: &str = "2026-06-01T10:00:00.000Z";
    const FIRST_DELIVERY: &str = "2026-06-01T12:00:00.000Z";
    const LATER_DELIVERY: &str = "2026-06-03T18:30:00.000Z";

    fn side(side_id: &str) -> (String, MatchSideRecord) {
        (
            side_id.to_string(),
            MatchSideRecord {
                side_id: side_id.to_string(),
                team_id: None,
                name: None,
                colour: None,
                max_players: None,
                team_join_enabled: false,
                player_count: 1,
                roster_preview: Vec::new(),
            },
        )
    }

    /// A completed, ranked, agreed squash 1v1 won by side "a" — the shape
    /// everything below varies one thing about.
    fn match_record() -> MatchRecord {
        MatchRecord {
            id: "m1".into(),
            created_by_user_id: "u1".into(),
            name: "Tuesday".into(),
            description: String::new(),
            match_type: "squash".into(),
            status: "completed".into(),
            series_id: None,
            starts_at: PLAYED_AT.into(),
            allow_unassigned: true,
            total_player_count: 2,
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
            authority: HashMap::new(),
            created_at: "2026-05-01T10:00:00.000Z".into(),
        }
    }

    /// The same match, played as a friendly, which moves nobody at all.
    fn friendly() -> MatchRecord {
        MatchRecord {
            ranked: false,
            ..match_record()
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
            joined_via: None,
            // Main's shape today. The switch-to-ranked prompt survives as a
            // notification and loses its enforcement, so this field is on its
            // way out; nothing here reads it either way.
        }
    }

    fn invited(mut player: MatchPlayerRecord, status: &str) -> MatchPlayerRecord {
        player.invitation = Some(EmbeddedInvitationRecord {
            id: format!("inv-{}", player.player_id),
            status: status.into(),
            invited_by_user_id: "u1".into(),
            invited_at: "2026-05-02T10:00:00.000Z".into(),
            responded_at: None,
            kind: InvitationKindRecord::Token {
                invite_token: "token".into(),
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

    fn user(id: &str) -> UserRecord {
        UserRecord {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: id.into(),
            profile_image_url: None,
            follower_count: 0,
            following_count: 0,
            unread_count: 0,
            stats: UserStatsRecord::default(),
            ratings: HashMap::new(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    /// Profiles for `ids`, each opted in to squash.
    fn opted_in(ids: &[&str]) -> HashMap<String, UserRecord> {
        ids.iter().map(|id| (id.to_string(), user(id))).collect()
    }

    /// The gate. One step: it used to be two, with a batch read between them
    /// to re-check everyone's opt-in. Kept as a named helper because the
    /// tests below read better saying "gate" than "rateable".
    fn gate(m: &MatchRecord, players: &[MatchPlayerRecord]) -> Result<Rateable, NotRated> {
        rateable(m, players)
    }

    // -----------------------------------------------------------------------
    // The gate and the projection
    // -----------------------------------------------------------------------

    /// The happy path, and the shape the engine is handed: one participant per
    /// side, the ladder from the sport tag, and `played_at` from `starts_at` —
    /// not from any confirmation time, since played order is the order ratings
    /// are meant to be applied in.
    #[test]
    fn a_completed_agreed_match_projects_to_its_sides() {
        let rated = gate(&match_record(), &singles()).expect("rated");
        assert_eq!(rated.ladder.as_str(), "squash");
        assert_eq!(rated.played_at, PLAYED_AT);
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
        assert_eq!(rated.user_ids(), vec!["u1".to_string(), "u2".to_string()]);
    }

    /// A doubles match puts both partners in one group, which is what lets
    /// Weng-Lin sum their beliefs into the side's strength.
    #[test]
    fn partners_project_onto_the_same_side() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("a")),
            player("p3", Some("u3"), Some("b")),
            player("p4", Some("u4"), Some("b")),
        ];
        let rated = gate(&match_record(), &players).expect("rated");
        let on = |side: &str| -> Vec<&str> {
            rated
                .participants
                .iter()
                .filter(|p| p.side_id == side)
                .map(|p| p.competitor_id.as_str())
                .collect()
        };
        assert_eq!(on("a"), vec!["u1", "u2"]);
        assert_eq!(on("b"), vec!["u3", "u4"]);
    }

    /// A friendly is turned away first, before the status and the roster are
    /// looked at. Cheapest answer, commonest case, and the one that keeps a
    /// like on somebody's kickabout from reading any profiles.
    #[test]
    fn a_friendly_is_not_rated() {
        assert_eq!(gate(&friendly(), &singles()), Err(NotRated::Friendly));
        let mut unfinished = friendly();
        unfinished.status = "scheduled".into();
        assert_eq!(gate(&unfinished, &singles()), Err(NotRated::Friendly));
    }

    /// Scheduled, live and cancelled matches have no result. All of them reach
    /// the handler as ordinary `#META` writes.
    #[test]
    fn only_a_completed_match_is_rated() {
        for status in ["scheduled", "in_progress", "cancelled"] {
            let mut m = match_record();
            m.status = status.into();
            assert_eq!(
                gate(&m, &singles()),
                Err(NotRated::NotCompleted),
                "{status}"
            );
        }
    }

    /// A score only one side has agreed to is a claim, not a result: rating it
    /// would move the opponent's number on something they haven't accepted. A
    /// pending score and a disputed one both leave `confirmed_score` unset.
    #[test]
    fn an_unconfirmed_score_is_not_rated() {
        let mut m = match_record();
        m.confirmed_score = None;
        assert_eq!(gate(&m, &singles()), Err(NotRated::Unconfirmed));
    }

    /// `other` has no ladder on purpose — it is every sport not modelled, and
    /// pooling them is the disjoint-graph mistake per-sport ladders avoid. A tag
    /// this build doesn't know degrades the same way.
    #[test]
    fn a_sport_with_no_ladder_is_not_rated() {
        for tag in ["other", "kabaddi"] {
            let mut m = match_record();
            m.match_type = tag.into();
            assert_eq!(gate(&m, &singles()), Err(NotRated::NoLadder), "{tag}");
        }
    }

    /// Pending and declined invitees take no spot, so they are not
    /// participants — and they block nothing, even with no account. The
    /// external case is the one that matters: a ranked match may
    /// send an external invite, and under the rule this replaced (an unlinked
    /// player on a side blocks unless declined) that unanswered invite would
    /// have left the match unrated for good.
    ///
    /// Waitlisted players used to be covered here too, back when waiting was
    /// a flag on a roster row. They have no roster row at all now (see
    /// `MatchWaitlistEntryRecord`), so there is nothing for this gate to
    /// exclude: they simply aren't in the roster it reads.
    #[test]
    fn players_who_take_no_spot_neither_rate_nor_block() {
        let mut players = singles();
        players.push(invited(player("p3", Some("u3"), Some("a")), "pending"));
        players.push(invited(player("p4", None, Some("b")), "pending"));
        players.push(invited(player("p5", None, Some("b")), "declined"));
        players.push(invited(player("p6", Some("u6"), Some("a")), "declined"));
        let rated = gate(&match_record(), &players).expect("rated");
        assert_eq!(rated.user_ids(), vec!["u1".to_string(), "u2".to_string()]);
        assert_eq!(rated.participants.len(), 2);
    }

    /// An accepted invitee is in, and rates like anyone added directly.
    #[test]
    fn an_accepted_invitee_rates() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            invited(player("p2", Some("u2"), Some("b")), "accepted"),
        ];
        let rated = gate(&match_record(), &players).expect("rated");
        assert_eq!(rated.participants.len(), 2);
    }

    /// One player in a spot with no account blocks the *whole* match, not just
    /// their own row: Weng-Lin sums a side's beliefs, so rating around them
    /// would credit their part in the result to their partner.
    #[test]
    fn an_unlinked_player_in_a_spot_blocks_the_whole_match() {
        let mut players = singles();
        players.push(player("p3", None, Some("b")));
        assert_eq!(
            gate(&match_record(), &players),
            Err(NotRated::UnlinkedPlayers {
                player_ids: vec!["p3".into()]
            })
        );
    }

    /// A guest with no account on a side blocks the match, because Weng-Lin
    /// sums a side's beliefs and rating around them would credit their part
    /// of the result to their team-mates. Unassigned, there is no side whose
    /// strength they distort, so there is nothing to refuse — this is the one
    /// unrated participant the design does not solve, kept as narrow as it
    /// can be.
    #[test]
    fn a_guest_blocks_only_from_a_side() {
        let mut unassigned_guest = singles();
        unassigned_guest.push(player("p4", None, None));
        assert_eq!(
            gate(&match_record(), &unassigned_guest)
                .expect("rated")
                .participants
                .len(),
            2
        );

        let mut guest_on_a_side = singles();
        guest_on_a_side.push(player("p4", None, Some("b")));
        assert_eq!(
            gate(&match_record(), &guest_on_a_side),
            Err(NotRated::UnlinkedPlayers {
                player_ids: vec!["p4".into()]
            })
        );
    }

    /// Somebody in a spot on no side played for neither, so they are not
    /// rated and — the part that changed — are not a reason to refuse either.
    /// They used to be checked here on the grounds that holding a spot
    /// obliged you to have consented. Nothing is gated on consent now, so
    /// checking a group wider than the one being rated would only find
    /// reasons to refuse matches that rate perfectly well. See `rateable`'s
    /// first judgement call.
    #[test]
    fn an_unassigned_player_is_ignored_entirely() {
        let mut players = singles();
        players.push(player("p3", Some("u3"), None));

        let rated = gate(&match_record(), &players).expect("rated");
        assert_eq!(rated.participants.len(), 2);
        assert!(!rated.user_ids().contains(&"u3".to_string()));
    }

    /// A side with nobody in a spot fails the match rather than turning a 1v1
    /// into a 1v0 — checked against the match's declared sides, not the sides
    /// the survivors happen to cover.
    #[test]
    fn a_side_with_nobody_in_a_spot_fails_the_match() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            invited(player("p2", Some("u2"), Some("b")), "pending"),
        ];
        assert_eq!(
            gate(&match_record(), &players),
            Err(NotRated::EmptySide {
                side_id: "b".into()
            })
        );
    }

    /// A single-sided match has nothing to compare against. Caught here so the
    /// engine's own refusal stays reserved for corrupt data.
    #[test]
    fn a_match_with_fewer_than_two_sides_is_not_rated() {
        let mut m = match_record();
        m.sides = HashMap::from([side("a")]);
        assert_eq!(
            gate(&m, &[player("p1", Some("u1"), Some("a"))]),
            Err(NotRated::TooFewSides(1))
        );
    }

    /// One account in two spots, on either side, leaves two updates disagreeing
    /// about the same rating, and there is no "best" of two Gaussians to keep.
    #[test]
    fn one_account_in_two_spots_is_not_rated() {
        for players in [
            vec![
                player("p1", Some("u1"), Some("a")),
                player("p2", Some("u2"), Some("b")),
                player("p3", Some("u1"), Some("b")),
            ],
            vec![
                player("p1", Some("u1"), Some("a")),
                player("p2", Some("u1"), Some("a")),
                player("p3", Some("u2"), Some("b")),
            ],
        ] {
            assert_eq!(
                gate(&match_record(), &players),
                Err(NotRated::DuplicateAccount {
                    user_id: "u1".into()
                })
            );
        }
    }

    /// A draw — no winner — is rated, not skipped: Weng-Lin models a tie
    /// natively, and a draw between mismatched players is real information.
    #[test]
    fn a_draw_is_rated() {
        let mut m = match_record();
        m.confirmed_score.as_mut().unwrap().winner_side_id = None;
        let rated = gate(&m, &singles()).expect("rated");
        assert_eq!(rated.winner_side_id, None);
    }

    /// Uneven sides rate as recorded (`rateable`'s second judgement call).
    /// Pinned so that blocking them, if a later change adds an expected side
    /// size, is a decision rather than an accident.
    #[test]
    fn an_uneven_roster_rates_as_recorded() {
        let players = vec![
            player("p1", Some("u1"), Some("a")),
            player("p2", Some("u2"), Some("a")),
            player("p3", Some("u3"), Some("b")),
        ];
        let rated = gate(&match_record(), &players).expect("rated");
        assert_eq!(rated.participants.len(), 3);
    }

    /// Unplayed matches and ladderless sports must be refused in silence:
    /// `#META` is rewritten by every like on every match, so a warning there
    /// would bury the refusals somebody will actually ask about — a
    /// completed, agreed match that still didn't rate.
    #[test]
    fn only_a_refused_roster_on_a_rateable_match_warrants_a_warning() {
        for routine in [
            NotRated::NotCompleted,
            NotRated::Unconfirmed,
            NotRated::NoLadder,
        ] {
            assert!(!routine.warrants_warning(), "{routine:?}");
        }
        for surprising in [
            NotRated::UnlinkedPlayers {
                player_ids: vec!["p3".into()],
            },
            NotRated::DuplicateAccount {
                user_id: "u1".into(),
            },
            NotRated::TooFewSides(1),
            NotRated::EmptySide {
                side_id: "b".into(),
            },
        ] {
            assert!(surprising.warrants_warning(), "{surprising:?}");
        }
    }

    /// The contribution records the side and the incoming belief, because
    /// those are what change detection re-rates from on the next delivery. If
    /// `side_id` were dropped, a player moved across sides would look
    /// unchanged; if the `before` half were, redelivery would have to re-rate
    /// from the current rating and would double count.
    #[test]
    fn the_contribution_carries_the_side_and_the_incoming_belief() {
        let rated = gate(&match_record(), &singles()).expect("rated");
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
        let contribution = contribution_for(&update, &rated, FIRST_DELIVERY);
        assert_eq!(contribution.user_id, "u2");
        assert_eq!(contribution.ladder, "squash");
        assert_eq!(contribution.side_id, "b");
        assert_eq!(contribution.played_at, PLAYED_AT);
        assert_eq!(contribution.movement.mu_before, 25.0);
        assert_eq!(contribution.movement.mu_after, 22.0);
        assert_eq!(contribution.movement.display_delta, -90);
        assert_eq!(contribution.applied_at, FIRST_DELIVERY);
    }

    /// `applied_at` is a fresh clock on every delivery, so it must not take
    /// part in "has this match's effect changed?" — redelivery is the common
    /// case, and counting the clock would make each like look like a re-score.
    #[test]
    fn a_redelivery_differs_only_in_applied_at_and_so_has_the_same_effect() {
        let rated = gate(&match_record(), &singles()).expect("rated");
        let update = RatingUpdate {
            competitor_id: "u1".into(),
            before: PlayerRating::default(),
            after: PlayerRating {
                mu: 27.6,
                sigma: 7.1,
            },
        };
        let first = contribution_for(&update, &rated, FIRST_DELIVERY);
        let redelivered = contribution_for(&update, &rated, LATER_DELIVERY);
        assert!(first.has_same_effect_as(&redelivered));
        assert_ne!(first.applied_at, redelivered.applied_at);
    }

    // -----------------------------------------------------------------------
    // The reconcile, against an in-memory store
    // -----------------------------------------------------------------------

    /// An in-memory stand-in for the items [`reconcile_match_rating`] reads and
    /// writes: matches with their rosters, profiles, contributions and history.
    ///
    /// Faithful in the respects the sequencing tests depend on:
    ///
    /// - **Writes are guarded like the real ones.** `apply_rating_contribution`
    ///   refuses (`Conflict`) when the contribution or the rating is not what
    ///   the caller said it read, and `withdraw_rating_contribution` when the
    ///   contribution has changed, so a handler passing the wrong guard fails
    ///   here as it would against DynamoDB.
    /// - **Each write is all or nothing**, like the transactions they stand
    ///   for. `fail_apply_for` makes one player's apply fail before it writes
    ///   anything, which is how a part-applied match is staged.
    /// - **An apply moves the history item** the way the real one does: written
    ///   at the contribution's key, with the previous key deleted when the two
    ///   differ.
    ///
    /// `writes` counts successful writes, which is how "nothing was written" is
    /// asserted rather than inferred.
    #[derive(Default)]
    struct FakeStore {
        matches: RefCell<HashMap<String, (MatchRecord, Vec<MatchPlayerRecord>)>>,
        users: RefCell<HashMap<String, UserRecord>>,
        /// Match id → user id → that match's contribution for them.
        contributions: RefCell<HashMap<String, BTreeMap<String, RatingContributionRecord>>>,
        /// (user id, rendered sort key) → history item. Ordered, because a
        /// history collection's order is its meaning.
        history: RefCell<BTreeMap<(String, String), RatingHistoryRecord>>,
        writes: Cell<usize>,
        fail_apply_for: RefCell<Option<String>>,
    }

    impl RatingStore for FakeStore {
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
                .map(|by_user| by_user.values().cloned().collect())
                .unwrap_or_default())
        }

        async fn batch_get_users(
            &self,
            user_ids: &[String],
        ) -> DaoResult<HashMap<String, UserRecord>> {
            let users = self.users.borrow();
            Ok(user_ids
                .iter()
                .filter_map(|id| Some((id.clone(), users.get(id)?.clone())))
                .collect())
        }

        async fn apply_rating_contribution(
            &self,
            match_id: &str,
            contribution: &RatingContributionRecord,
            stored: Option<&RatingContributionRecord>,
            rating: &RatingRecord,
            stored_rating: Option<&RatingRecord>,
        ) -> DaoResult<()> {
            let user_id = &contribution.user_id;
            if self.fail_apply_for.borrow().as_deref() == Some(user_id.as_str()) {
                return Err(DaoError::Conflict(format!(
                    "injected failure for {user_id}"
                )));
            }
            let mut users = self.users.borrow_mut();
            let Some(profile) = users.get_mut(user_id) else {
                return Err(DaoError::NotFound(format!("user {user_id}")));
            };
            let mut contributions = self.contributions.borrow_mut();
            let for_match = contributions.entry(match_id.to_string()).or_default();

            let contribution_guard_holds = match (stored, for_match.get(user_id)) {
                (None, None) => true,
                (Some(read), Some(current)) => current.has_same_effect_as(read),
                _ => false,
            };
            let rating_guard_holds = profile.ratings.get(&contribution.ladder) == stored_rating;
            if !contribution_guard_holds || !rating_guard_holds {
                return Err(DaoError::Conflict(format!(
                    "rating state for {user_id} changed concurrently"
                )));
            }

            for_match.insert(user_id.clone(), contribution.clone());
            profile
                .ratings
                .insert(contribution.ladder.clone(), rating.clone());
            let mut history = self.history.borrow_mut();
            let key = contribution.history_sk(match_id).to_string();
            if let Some(previous) = stored {
                let previous_key = previous.history_sk(match_id).to_string();
                if previous_key != key {
                    history.remove(&(user_id.clone(), previous_key));
                }
            }
            history.insert((user_id.clone(), key), contribution.history_entry(match_id));
            self.writes.set(self.writes.get() + 1);
            Ok(())
        }

        async fn withdraw_rating_contribution(
            &self,
            match_id: &str,
            stored: &RatingContributionRecord,
        ) -> DaoResult<()> {
            let mut contributions = self.contributions.borrow_mut();
            let current = contributions
                .get(match_id)
                .and_then(|by_user| by_user.get(&stored.user_id));
            if !current.is_some_and(|c| c.has_same_effect_as(stored)) {
                return Err(DaoError::Conflict(format!(
                    "contribution for {} changed concurrently",
                    stored.user_id
                )));
            }
            if let Some(by_user) = contributions.get_mut(match_id) {
                by_user.remove(&stored.user_id);
            }
            self.history.borrow_mut().remove(&(
                stored.user_id.clone(),
                stored.history_sk(match_id).to_string(),
            ));
            self.writes.set(self.writes.get() + 1);
            Ok(())
        }
    }

    impl FakeStore {
        /// A store holding one match, its roster, and a squash-opted-in profile
        /// for each of `users`.
        fn with_match(
            record: MatchRecord,
            players: Vec<MatchPlayerRecord>,
            users: &[&str],
        ) -> Self {
            let store = FakeStore::default();
            store.put_match(record, players);
            store.users.borrow_mut().extend(opted_in(users));
            store
        }

        fn put_match(&self, record: MatchRecord, players: Vec<MatchPlayerRecord>) {
            self.matches
                .borrow_mut()
                .insert(record.id.clone(), (record, players));
        }

        fn edit_match(&self, match_id: &str, edit: impl FnOnce(&mut MatchRecord)) {
            edit(
                &mut self
                    .matches
                    .borrow_mut()
                    .get_mut(match_id)
                    .expect("a match")
                    .0,
            );
        }

        fn edit_roster(&self, match_id: &str, edit: impl FnOnce(&mut Vec<MatchPlayerRecord>)) {
            edit(
                &mut self
                    .matches
                    .borrow_mut()
                    .get_mut(match_id)
                    .expect("a match")
                    .1,
            );
        }

        fn contributions(&self, match_id: &str) -> BTreeMap<String, RatingContributionRecord> {
            self.contributions
                .borrow()
                .get(match_id)
                .cloned()
                .unwrap_or_default()
        }

        fn rating(&self, user_id: &str) -> Option<RatingRecord> {
            self.users
                .borrow()
                .get(user_id)
                .and_then(|u| u.ratings.get("squash"))
                .cloned()
        }

        /// Seed a squash rating, for the cases that need a player to arrive
        /// with a history rather than build one.
        fn set_rating(&self, user_id: &str, rating: RatingRecord) {
            self.users
                .borrow_mut()
                .get_mut(user_id)
                .expect("user exists")
                .ratings
                .insert("squash".into(), rating);
        }

        /// One user's history, in key (played) order.
        fn history_of(&self, user_id: &str) -> Vec<RatingHistoryRecord> {
            self.history
                .borrow()
                .iter()
                .filter(|((owner, _), _)| owner == user_id)
                .map(|(_, entry)| entry.clone())
                .collect()
        }
    }

    fn run(store: &FakeStore, match_id: &str, now: &str) -> Reconciled {
        futures::executor::block_on(reconcile_match_rating(store, match_id, now))
            .expect("the delivery reconciles")
    }

    fn repairs(done: &Reconciled) -> BTreeSet<(RepairTrigger, &str)> {
        done.repairs
            .iter()
            .map(|r| {
                assert_eq!(r.ladder, "squash");
                (r.trigger, r.user_id.as_str())
            })
            .collect()
    }

    impl PartialOrd for RepairTrigger {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for RepairTrigger {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            (*self as u8).cmp(&(*other as u8))
        }
    }

    /// A first confirmation writes everything a rated match needs, for every
    /// participant: the rating (moved the right way, one match counted, played
    /// time recorded), the contribution that produced it, and the history
    /// entry — the three the DAO writes as one transaction so they can't
    /// disagree.
    #[test]
    fn a_first_confirmation_rates_every_participant() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        let done = run(&store, "m1", FIRST_DELIVERY);

        assert_eq!(done.applied, vec!["u1".to_string(), "u2".to_string()]);
        assert!(done.withdrawn.is_empty());
        assert!(done.repairs.is_empty(), "{:?}", done.repairs);

        let contributions = store.contributions("m1");
        for (user_id, side_id) in [("u1", "a"), ("u2", "b")] {
            let contribution = &contributions[user_id];
            let rating = store.rating(user_id).expect("rated");
            assert_eq!(contribution.side_id, side_id);
            assert_eq!(contribution.played_at, PLAYED_AT);
            assert_eq!(contribution.movement.mu_before, INITIAL_MU);
            assert_eq!(contribution.movement.sigma_before, INITIAL_SIGMA);
            assert_eq!(contribution.movement.mu_after, rating.mu);
            assert_eq!(contribution.movement.sigma_after, rating.sigma);
            assert!(
                rating.sigma < INITIAL_SIGMA,
                "{user_id}: a result settles σ"
            );
            assert_eq!(rating.matches_rated, 1);
            assert_eq!(rating.last_rated_at, PLAYED_AT);
            assert_eq!(
                store.history_of(user_id),
                vec![contribution.history_entry("m1")]
            );
        }
        assert!(
            store.rating("u1").unwrap().mu > INITIAL_MU,
            "the winner rises"
        );
        assert!(
            store.rating("u2").unwrap().mu < INITIAL_MU,
            "the loser falls"
        );
    }

    /// **Redelivery is a no-op.** A like rewrites `#META`, so this handler
    /// re-runs on finished matches constantly. Nothing may be written — not
    /// even a refreshed `applied_at` — and nothing may be flagged.
    #[test]
    fn a_redelivered_rated_match_writes_nothing() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        run(&store, "m1", FIRST_DELIVERY);
        let writes = store.writes.get();
        let contributions = store.contributions("m1");
        let ratings = (store.rating("u1"), store.rating("u2"));

        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(store.writes.get(), writes, "no write on redelivery");
        assert!(done.applied.is_empty() && done.withdrawn.is_empty());
        assert!(done.repairs.is_empty(), "{:?}", done.repairs);
        assert_eq!(store.contributions("m1"), contributions);
        assert_eq!((store.rating("u1"), store.rating("u2")), ratings);
    }

    /// **A re-score is detected, and nothing is written.** Both players' stored
    /// contributions are the replay's input and must survive untouched; a
    /// recomputed "correction" applied now would start from ratings that have
    /// moved on since, and would destroy the only record of what was applied.
    #[test]
    fn a_re_scored_match_is_flagged_for_every_player_and_nothing_is_written() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        run(&store, "m1", FIRST_DELIVERY);
        let writes = store.writes.get();
        let contributions = store.contributions("m1");
        let ratings = (store.rating("u1"), store.rating("u2"));

        store.edit_match("m1", |m| {
            m.confirmed_score.as_mut().unwrap().winner_side_id = Some("b".into());
        });
        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(
            repairs(&done),
            BTreeSet::from([
                (RepairTrigger::Rerated, "u1"),
                (RepairTrigger::Rerated, "u2")
            ])
        );
        assert_eq!(store.writes.get(), writes, "detected, not applied");
        assert!(done.applied.is_empty() && done.withdrawn.is_empty());
        assert_eq!(store.contributions("m1"), contributions);
        assert_eq!((store.rating("u1"), store.rating("u2")), ratings);
    }

    /// **A part-applied match completes on redelivery**, to exactly the state a
    /// clean run reaches. Each player's apply is its own transaction, so a
    /// crash or a lost lock between two of them leaves a subset written. The
    /// redelivery rates the one already written from their stored `before` (so
    /// recomputes the same movement and leaves it alone) and the one missing
    /// from their current rating, which nothing has touched.
    #[test]
    fn a_partially_applied_match_completes_on_redelivery() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        *store.fail_apply_for.borrow_mut() = Some("u2".into());
        let failed =
            futures::executor::block_on(reconcile_match_rating(&store, "m1", FIRST_DELIVERY));
        assert!(failed.is_err(), "the second apply failed");
        let first_half = store.contributions("m1");
        assert_eq!(first_half.keys().collect::<Vec<_>>(), vec!["u1"]);

        *store.fail_apply_for.borrow_mut() = None;
        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(done.applied, vec!["u2".to_string()]);
        assert!(done.repairs.is_empty(), "{:?}", done.repairs);
        assert_eq!(
            store.contributions("m1")["u1"],
            first_half["u1"],
            "the half already applied is left exactly as it was"
        );

        let clean = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        run(&clean, "m1", FIRST_DELIVERY);
        for user_id in ["u1", "u2"] {
            assert!(
                store.contributions("m1")[user_id]
                    .has_same_effect_as(&clean.contributions("m1")[user_id]),
                "{user_id}"
            );
            assert_eq!(store.rating(user_id), clean.rating(user_id), "{user_id}");
        }
    }

    /// **Regression (#127's review).** A player added to an already-rated match
    /// was left with no rating for it, indefinitely.
    ///
    /// The delivery carries two outcomes at once: the two already rated have
    /// *moved* (the side they face gained a player) and need a replay, while
    /// the newcomer has nothing stored and needs a first apply. The handler
    /// used to return the moment anybody needed a replay, dropping the
    /// newcomer — and a replay could not cover for it, since it walks stored
    /// history and can only correct a contribution that exists.
    ///
    /// The two already rated must still be left strictly alone, or "apply
    /// everything unconditionally" would pass this too.
    #[test]
    fn a_player_added_to_a_rated_match_is_applied_even_when_the_others_need_a_replay() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2", "u3"]);
        run(&store, "m1", FIRST_DELIVERY);
        let before = store.contributions("m1");

        store.edit_roster("m1", |players| {
            players.push(player("p3", Some("u3"), Some("b")));
        });
        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(done.applied, vec!["u3".to_string()]);
        let after = store.contributions("m1");
        assert_eq!(after["u3"].side_id, "b");
        assert_eq!(store.rating("u3").map(|r| r.matches_rated), Some(1));
        assert_eq!(
            repairs(&done),
            BTreeSet::from([
                (RepairTrigger::Rerated, "u1"),
                (RepairTrigger::Rerated, "u2")
            ])
        );
        for user_id in ["u1", "u2"] {
            assert_eq!(
                after[user_id], before[user_id],
                "{user_id} is stale, and their contribution is the replay's input"
            );
        }
    }

    /// **A rated match that stops being eligible is withdrawn whole**:
    /// contributions and history entries go, and each player's ladder is
    /// flagged. The ratings themselves are left holding the match's effect,
    /// which looks like a bug and is the design: a Weng-Lin update can't be
    /// subtracted, only replayed, and the replay needs the history without it.
    ///
    /// Two ways in: the match is cancelled, or it is switched back to
    /// friendly. Both stop it counting for everybody at once, which is the
    /// only shape a withdrawal of the whole match takes now that no
    /// individual player can become ineligible on their own.
    #[test]
    fn a_rated_match_that_stops_being_eligible_has_its_contributions_withdrawn() {
        let cancel: fn(&FakeStore) =
            |store| store.edit_match("m1", |m| m.status = "cancelled".into());
        // Switching a rated match back to friendly stops it counting for
        // everybody, without touching the roster at all — the same
        // withdrawal path a cancellation reaches, by a completely different
        // route.
        let make_friendly: fn(&FakeStore) = |store| store.edit_match("m1", |m| m.ranked = false);

        for (case, stop_counting) in [
            ("cancelled", cancel),
            ("switched to friendly", make_friendly),
        ] {
            let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
            run(&store, "m1", FIRST_DELIVERY);
            let ratings = (store.rating("u1"), store.rating("u2"));

            stop_counting(&store);
            let done = run(&store, "m1", LATER_DELIVERY);

            assert_eq!(
                done.withdrawn,
                vec!["u1".to_string(), "u2".to_string()],
                "{case}"
            );
            assert_eq!(
                repairs(&done),
                BTreeSet::from([
                    (RepairTrigger::Withdrawn, "u1"),
                    (RepairTrigger::Withdrawn, "u2")
                ]),
                "{case}"
            );
            assert!(done.applied.is_empty(), "{case}: nobody is rated instead");
            assert!(store.contributions("m1").is_empty(), "{case}");
            assert!(store.history_of("u1").is_empty(), "{case}");
            assert!(store.history_of("u2").is_empty(), "{case}");
            assert_eq!(
                (store.rating("u1"), store.rating("u2")),
                ratings,
                "{case}: left stale for the replay"
            );
        }
    }

    /// End to end, the change this whole design turns on: a ranked match
    /// rates **everybody** in it, whether or not they ever agreed to be
    /// rated. The match used to be refused outright unless every player had
    /// consented, which is what made consent a precondition of the maths and
    /// forced every switch-to-ranked to chase it.
    #[test]
    fn a_ranked_match_rates_everybody_in_it() {
        let store = FakeStore::with_match(match_record(), singles(), &[]);
        // Profiles that have consented to nothing: the point of the test.
        for id in ["u1", "u2"] {
            store.users.borrow_mut().insert(id.into(), user(id));
        }

        let done = run(&store, "m1", FIRST_DELIVERY);

        assert_eq!(done.applied, vec!["u1".to_string(), "u2".to_string()]);
        let winner = store.rating("u1").expect("rated");
        let loser = store.rating("u2").expect("rated");
        assert!(winner.mu > INITIAL_MU, "the winner went up");
        assert!(loser.mu < INITIAL_MU, "the loser went down");
        assert_eq!(winner.matches_rated, 1);
        assert_eq!(loser.matches_rated, 1);
    }

    /// A friendly rates nobody, and is turned away before a single profile is
    /// read. Rating one for some of its players and not the rest is what
    /// breaks Weng-Lin's conservation, so the rule is all or nothing and a
    /// friendly is nothing.
    #[test]
    fn a_friendly_rates_nobody() {
        let store = FakeStore::with_match(friendly(), singles(), &[]);

        let done = run(&store, "m1", FIRST_DELIVERY);

        assert!(done.applied.is_empty());
        assert_eq!(store.rating("u1"), None);
        assert_eq!(store.rating("u2"), None);
        assert!(store.contributions("m1").is_empty());
        assert_eq!(gate(&friendly(), &singles()), Err(NotRated::Friendly));
    }

    /// A rating enters its next match on exactly the confidence its own play
    /// earned. Nothing resets `sigma` any more — the old design threw it away
    /// at opt-in, to undo a prior built out of casual games, and there is no
    /// such prior to undo now that friendlies move nobody.
    #[test]
    fn a_settled_rating_enters_its_next_match_on_the_confidence_it_earned() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        let settled = RatingRecord {
            mu: 27.0,
            sigma: 2.0,
            matches_rated: 30,
            last_rated_at: "2026-01-01T00:00:00.000Z".into(),
        };
        store.set_rating("u1", settled);

        run(&store, "m1", FIRST_DELIVERY);

        let contribution = store.contributions("m1").remove("u1").expect("rated");
        assert_eq!(contribution.movement.sigma_before, 2.0);
        assert_eq!(contribution.movement.mu_before, 27.0);
        assert_eq!(store.rating("u1").expect("rated").matches_rated, 31);
    }

    /// A player removed from a rated match (or who no longer takes a spot) has
    /// their contribution withdrawn, while the players left have moved and are
    /// flagged without a write — `dao::rating`'s protocol steps 3 and 5 in one
    /// delivery.
    #[test]
    fn a_player_removed_from_a_rated_match_is_withdrawn_and_the_rest_flagged() {
        let mut players = singles();
        players.push(player("p3", Some("u3"), Some("b")));
        let store = FakeStore::with_match(match_record(), players, &["u1", "u2", "u3"]);
        run(&store, "m1", FIRST_DELIVERY);
        let before = store.contributions("m1");
        let u3_rating = store.rating("u3");

        store.edit_roster("m1", |players| players.retain(|p| p.player_id != "p3"));
        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(done.withdrawn, vec!["u3".to_string()]);
        assert!(done.applied.is_empty());
        assert_eq!(
            repairs(&done),
            BTreeSet::from([
                (RepairTrigger::Withdrawn, "u3"),
                (RepairTrigger::Rerated, "u1"),
                (RepairTrigger::Rerated, "u2"),
            ])
        );
        let after = store.contributions("m1");
        assert!(!after.contains_key("u3"));
        assert!(store.history_of("u3").is_empty());
        assert_eq!(store.rating("u3"), u3_rating, "stale until replayed");
        for user_id in ["u1", "u2"] {
            assert_eq!(after[user_id], before[user_id], "{user_id}");
        }
    }

    /// A match played before one already rated is applied, then flagged — and
    /// `last_rated_at` stays on the newest match *played*. Refusing to apply it
    /// would leave it with no history entry for a replay to find; letting
    /// `last_rated_at` move back would make every later match look out of order.
    #[test]
    fn a_match_confirmed_out_of_order_is_applied_then_flagged() {
        let store = FakeStore::with_match(match_record(), singles(), &["u1", "u2"]);
        let mut later = match_record();
        later.id = "m2".into();
        later.starts_at = "2026-06-08T10:00:00.000Z".into();
        store.put_match(later, singles());

        run(&store, "m2", FIRST_DELIVERY);
        let done = run(&store, "m1", LATER_DELIVERY);

        assert_eq!(done.applied, vec!["u1".to_string(), "u2".to_string()]);
        assert_eq!(
            repairs(&done),
            BTreeSet::from([
                (RepairTrigger::OutOfOrder, "u1"),
                (RepairTrigger::OutOfOrder, "u2")
            ])
        );
        for user_id in ["u1", "u2"] {
            let rating = store.rating(user_id).expect("rated");
            assert_eq!(rating.matches_rated, 2, "{user_id}");
            assert_eq!(
                rating.last_rated_at, "2026-06-08T10:00:00.000Z",
                "{user_id}"
            );
            let played: Vec<String> = store
                .history_of(user_id)
                .into_iter()
                .map(|h| h.match_id)
                .collect();
            assert_eq!(
                played,
                vec!["m1", "m2"],
                "{user_id}: history in played order"
            );
        }
    }
}
