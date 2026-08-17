//! Inline handler: reconcile per-user, per-sport stats from a match's state.
//!
//! On any write to a match's `#META`, we recompute what the match *currently*
//! contributes to each participant's stats and reconcile it (see
//! [`Dao::reconcile_match_contribution`]): a match with a **confirmed** score
//! contributes an outcome (won/drawn/lost) for everyone who actually played,
//! plus any sport-specific counters (cricket runs/wickets, football
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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::ScoreRecord;
use agon_core::dao::stats::{MatchContribution, MatchOutcome};

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
            let counters = sport_counters(&sport, &cs.score, &player.player_id);

            // If a user somehow appears twice, take the best outcome
            // (won > drawn > lost) and sum counters across both appearances.
            let entry = desired.entry(user_id.clone()).or_default();
            entry.outcome = Some(match (entry.outcome, outcome) {
                (Some(MatchOutcome::Won), _) | (_, MatchOutcome::Won) => MatchOutcome::Won,
                (Some(MatchOutcome::Drawn), _) | (_, MatchOutcome::Drawn) => MatchOutcome::Drawn,
                _ => MatchOutcome::Lost,
            });
            for (k, v) in counters {
                *entry.counters.entry(k).or_insert(0) += v;
            }
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

/// A player's box-score contribution for one match's confirmed score, keyed
/// by sport-specific counter name. Empty for a sport with no per-player box
/// score to derive extras from, or when `player_id` didn't feature at all
/// (e.g. an accepted invitee who didn't bat/bowl/score).
fn sport_counters(sport: &str, score: &ScoreRecord, player_id: &str) -> HashMap<String, u64> {
    match sport {
        "cricket" => cricket_counters(score, player_id),
        "football" => football_counters(score, player_id),
        _ => HashMap::new(),
    }
}

/// Sums this player's runs/fours/sixes (batting) and wickets (bowling) across
/// every innings of a confirmed cricket score. A player can feature in more
/// than one innings (e.g. a two-innings match), so these accumulate rather
/// than take the last entry.
fn cricket_counters(score: &ScoreRecord, player_id: &str) -> HashMap<String, u64> {
    let mut counters = HashMap::new();
    let ScoreRecord::Cricket { innings, .. } = score else {
        return counters;
    };
    for inning in innings {
        for entry in inning.batting.iter().flatten() {
            if entry.player_id == player_id {
                *counters.entry("runs".to_string()).or_insert(0) += entry.runs as u64;
                *counters.entry("fours".to_string()).or_insert(0) += entry.fours as u64;
                *counters.entry("sixes".to_string()).or_insert(0) += entry.sixes as u64;
            }
        }
        for entry in inning.bowling.iter().flatten() {
            if entry.player_id == player_id {
                *counters.entry("wickets".to_string()).or_insert(0) += entry.wickets as u64;
            }
        }
    }
    counters
}

/// Counts this player's goals scored (own goals excluded) and assists across
/// a confirmed football score's goal log.
fn football_counters(score: &ScoreRecord, player_id: &str) -> HashMap<String, u64> {
    let mut counters = HashMap::new();
    let ScoreRecord::Football { goals, .. } = score else {
        return counters;
    };
    for goal in goals.iter().flatten() {
        if !goal.own_goal && goal.scorer_player_id.as_deref() == Some(player_id) {
            *counters.entry("goals".to_string()).or_insert(0) += 1;
        }
        if goal.assist_player_id.as_deref() == Some(player_id) {
            *counters.entry("assists".to_string()).or_insert(0) += 1;
        }
    }
    counters
}
