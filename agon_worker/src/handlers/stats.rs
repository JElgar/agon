//! Inline handler: reconcile per-user, per-sport stats from a match's state.
//!
//! On any write to a match's `#META`, we recompute what the match *currently*
//! contributes to each participant's stats and reconcile it (see
//! [`Dao::reconcile_match_contribution`]): a match with a **confirmed** score
//! contributes an outcome (won/drawn/lost) for everyone who actually played,
//! plus any sport-specific counters (cricket runs/wickets/fours/sixes/
//! catches/balls faced/balls bowled/runs conceded/dismissals, football
//! goals/assists) derived from the confirmed score's per-player box score;
//! anything else (scheduled, cancelled, pending/disputed score, roster
//! change) is reconciled to its new value, backing out stale contributions.
//!
//! **Idempotency / correctness**: the worker sees a match-meta event on *every*
//! write to it (status changes, but also each like/comment counter bump), and
//! SQS delivers at-least-once. Reconciliation is a diff against the stored
//! per-match contribution, so an unchanged state writes nothing, and a changed
//! one (re-score, cancellation, roster edit) self-corrects. We reconcile the
//! union of current participants and users with an existing contribution, so a
//! player removed from the roster has their contribution backed out too.

use std::collections::{BTreeMap, BTreeSet};

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{MatchFormatRecord, ScoreRecord};
use agon_core::dao::stats::{BowlingSpell, MatchContribution, MatchOutcome};
use agon_core::sport::{SportContribution, SportRecord};
use agon_core::sports::cricket::CricketRecord;
use agon_core::sports::football::FootballRecord;
use agon_core::sports::netball::NetballRecord;

/// Legal deliveries per over when a match hasn't configured a format (or
/// configured a non-cricket one) — the standard rule.
const DEFAULT_BALLS_PER_OVER: u32 = 6;

use crate::error::WorkerResult;
use crate::event::ChangeEvent;

/// Handle a stats-relevant change event: any non-remove write to a match's
/// `#META`. Everything else is ignored.
pub async fn handle(dao: &Dao, ev: &ChangeEvent) -> WorkerResult<()> {
    if ev.kind.is_remove() {
        return Ok(());
    }
    let (Pk::Match(match_id), Sk::Meta) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };
    reconcile_match_stats(dao, match_id).await
}

/// Reconcile every participant's per-sport stat contribution against a match's
/// current state. Shared by the `#META` stream handler and the accept saga: a
/// roster link (a `PLAYER#` write) doesn't touch `#META`, so accepting an invite
/// into an already-completed match must reconcile the newly-linked player here.
///
/// Idempotent (a diff against the stored contribution), so re-running it — from
/// either caller, or a redelivery — converges to the same state.
pub async fn reconcile_match_stats(dao: &Dao, match_id: &str) -> WorkerResult<()> {
    // Re-read the current aggregate rather than trusting the stream image, so we
    // reflect the latest committed sides/players/score. A missing match means
    // there's nothing to attribute (its contributions, if any, are orphaned but
    // harmless — a delete flow would clean them up).
    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };

    // A match only contributes to stats once its score is confirmed — a
    // completed-but-pending (or disputed) score isn't agreed yet, and counting
    // it would inflate `matches_played` with no corresponding win, dragging
    // down (or zeroing) the win rate for a game nobody has actually lost.
    let confirmed_score = agg.match_.confirmed_score.as_ref();
    let sport = agg.match_.match_type.clone();
    let winner_side_id = confirmed_score.and_then(|cs| cs.winner_side_id.clone());

    // Cricket-only: this match's actual legal-ball-per-over count, so a
    // bowling entry's `Overs` converts to a raw ball count exactly (not by
    // assuming the standard 6) regardless of which format the match used.
    let balls_per_over = match &agg.match_.format {
        Some(MatchFormatRecord::Cricket(f)) => f.balls_per_over,
        _ => DEFAULT_BALLS_PER_OVER,
    };

    // Desired contribution per participant who actually played, keyed by user
    // id. "Played" = a match with a confirmed score where the player is the
    // creator/self-added (no embedded invitation) or an accepted invitee.
    // Pending/declined invitees are on the roster but didn't play.
    let mut desired: BTreeMap<String, MatchContribution> = Default::default();
    if let Some(cs) = confirmed_score {
        for player in &agg.players {
            let Some(user_id) = &player.user_id else {
                continue;
            };
            let played = match &player.invitation {
                None => true,
                Some(inv) => inv.status == "accepted",
            };
            if !played {
                continue;
            }
            // No winner recorded => the match was a draw for everyone who
            // played, regardless of side. Otherwise won/lost depends on
            // whether this player's side matches the winner — a player with
            // no side assigned counts as a loss, same as any other side that
            // isn't the winner's.
            let outcome = if winner_side_id.is_none() {
                MatchOutcome::Drawn
            } else if player.side_id == winner_side_id {
                MatchOutcome::Won
            } else {
                MatchOutcome::Lost
            };
            let contribution =
                sport_contribution(&sport, &cs.score, &player.player_id, balls_per_over);

            // If a user somehow appears twice, take the best outcome
            // (won > drawn > lost) and sum counters/bowling figures across
            // both appearances.
            let entry = desired.entry(user_id.clone()).or_default();
            entry.outcome = Some(match (entry.outcome, outcome) {
                (Some(MatchOutcome::Won), _) | (_, MatchOutcome::Won) => MatchOutcome::Won,
                (Some(MatchOutcome::Drawn), _) | (_, MatchOutcome::Drawn) => MatchOutcome::Drawn,
                _ => MatchOutcome::Lost,
            });
            for (k, v) in contribution.counters {
                *entry.counters.entry(k).or_insert(0) += v;
            }
            for (k, v) in contribution.best_candidates {
                *entry.best_candidates.entry(k).or_insert(0) += v;
            }
            if let Some(spell) = contribution.bowling_spell {
                let acc = entry.bowling_spell.get_or_insert(BowlingSpell::default());
                acc.wickets += spell.wickets;
                acc.runs_conceded += spell.runs_conceded;
                acc.balls_bowled += spell.balls_bowled;
                // `overs` is derived once below, from the fully-accumulated
                // `balls_bowled` — two `Overs` values don't sum field-wise
                // (balls can roll over into a whole extra over), so summing
                // the raw ball count first and converting once is what's
                // actually correct for the rare case of one user appearing
                // as more than one player in the same match.
            }
        }
    }
    for contribution in desired.values_mut() {
        if let Some(spell) = &mut contribution.bowling_spell {
            spell.overs =
                agon_core::sports::cricket::balls_to_overs(spell.balls_bowled, balls_per_over);
        }
    }

    // Reconcile the union of current participants and anyone who already has a
    // stored contribution (so removed players / a now-uncompleted match get
    // backed out to zero).
    let mut targets: BTreeSet<String> = desired.keys().cloned().collect();
    for uid in dao.list_stat_contribution_user_ids(match_id).await? {
        targets.insert(uid);
    }

    for user_id in targets {
        let contribution = desired.get(&user_id).cloned().unwrap_or_default();
        dao.reconcile_match_contribution(match_id, &user_id, &sport, &contribution)
            .await?;
    }

    Ok(())
}

fn sport_contribution(
    sport: &str,
    score: &ScoreRecord,
    player_id: &str,
    balls_per_over: u32,
) -> SportContribution {
    match sport {
        "cricket" => CricketRecord::contribution(score, player_id, balls_per_over),
        "football" => FootballRecord::contribution(score, player_id, balls_per_over),
        "netball" => NetballRecord::contribution(score, player_id, balls_per_over),
        _ => SportContribution::default(),
    }
}

// Cricket's contribution logic (including `overs_to_balls`/`balls_to_overs`)
// has moved to `agon_core::sports::cricket` — see that module.
// Football's contribution logic has moved to
// `agon_core::sports::football::FootballRecord` — see that module.
