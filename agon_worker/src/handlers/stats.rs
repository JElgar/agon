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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{
    CricketDismissalKindRecord, MatchFormatRecord, OversRecord, ScoreRecord,
};
use agon_core::dao::stats::{BowlingSpell, MatchContribution, MatchOutcome};

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
            spell.overs = balls_to_overs(spell.balls_bowled, balls_per_over);
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

/// A player's box-score contribution for one match's confirmed score: every
/// counter that feeds their lifetime totals, the subset worth a personal-best
/// record, and (cricket only) their bowling figures in this match. Empty for
/// a sport with no per-player box score to derive any of this from, or when
/// `player_id` didn't feature at all (e.g. an accepted invitee who didn't
/// bat/bowl/score).
struct SportContribution {
    counters: HashMap<String, u64>,
    best_candidates: HashMap<String, u64>,
    bowling_spell: Option<BowlingSpell>,
}

fn sport_contribution(
    sport: &str,
    score: &ScoreRecord,
    player_id: &str,
    balls_per_over: u32,
) -> SportContribution {
    match sport {
        "cricket" => cricket_contribution(score, player_id, balls_per_over),
        "football" => football_contribution(score, player_id),
        _ => SportContribution {
            counters: HashMap::new(),
            best_candidates: HashMap::new(),
            bowling_spell: None,
        },
    }
}

/// Sums this player's batting (runs/fours/sixes/balls faced/dismissals),
/// fielding (catches), and bowling (wickets/runs conceded/balls bowled)
/// across every innings of a confirmed cricket score. A player can feature in
/// more than one innings (e.g. a two-innings match), so these accumulate
/// rather than take the last entry.
///
/// Only "runs" (high score) is a best-candidate here — "wickets" is tracked
/// as a richer `bowling_spell` (best bowling figures need runs-conceded/
/// balls-bowled alongside the wicket count, not just a bare scalar; see
/// `Dao::update_best_bowling_figures`), and the rest (fours, sixes, balls
/// faced, dismissals, catches, runs conceded, balls bowled) are lifetime
/// totals only — nobody's asked to see "most balls faced in a game" as a
/// record, so there's no reason to pay for tracking it.
fn cricket_contribution(
    score: &ScoreRecord,
    player_id: &str,
    balls_per_over: u32,
) -> SportContribution {
    let mut counters = HashMap::new();
    let mut best_candidates = HashMap::new();
    let mut bowling_spell = BowlingSpell::default();
    let mut bowled = false;

    let ScoreRecord::Cricket { innings, .. } = score else {
        return SportContribution {
            counters,
            best_candidates,
            bowling_spell: None,
        };
    };

    for inning in innings {
        for entry in inning.batting.iter().flatten() {
            if entry.player_id == player_id {
                *counters.entry("runs".to_string()).or_insert(0) += entry.runs as u64;
                *counters.entry("fours".to_string()).or_insert(0) += entry.fours as u64;
                *counters.entry("sixes".to_string()).or_insert(0) += entry.sixes as u64;
                *counters.entry("balls_faced".to_string()).or_insert(0) += entry.balls_faced as u64;
                if entry.dismissal.is_some() {
                    *counters.entry("dismissals".to_string()).or_insert(0) += 1;
                }
            }
            // Catches: this player credited as the fielder on *any* batter's
            // dismissal in the innings, regardless of which side they
            // batted for (a catch is a fielding contribution, not tied to
            // this player's own batting entry).
            if let Some(dismissal) = &entry.dismissal
                && matches!(dismissal.kind, CricketDismissalKindRecord::Caught)
                && dismissal.fielder_player_id.as_deref() == Some(player_id)
            {
                *counters.entry("catches".to_string()).or_insert(0) += 1;
            }
        }
        for entry in inning.bowling.iter().flatten() {
            if entry.player_id == player_id {
                let balls = overs_to_balls(entry.overs.overs, entry.overs.balls, balls_per_over);
                *counters.entry("wickets".to_string()).or_insert(0) += entry.wickets as u64;
                *counters.entry("runs_conceded".to_string()).or_insert(0) +=
                    entry.runs_conceded as u64;
                *counters.entry("balls_bowled".to_string()).or_insert(0) += balls;
                bowled = true;
                bowling_spell.wickets += entry.wickets as u64;
                bowling_spell.runs_conceded += entry.runs_conceded as u64;
                bowling_spell.balls_bowled += balls;
            }
        }
    }

    if let Some(runs) = counters.get("runs") {
        best_candidates.insert("runs".to_string(), *runs);
    }

    SportContribution {
        counters,
        best_candidates,
        bowling_spell: bowled.then_some(bowling_spell),
    }
}

/// Legal balls bowled, exact — uses this match's own `balls_per_over` (from
/// its `CricketFormatRecord`, or the standard 6 if unconfigured), not an
/// assumed one, so a 5-ball-over match (e.g. The Hundred) contributes its
/// true ball count rather than a slightly-off one.
fn overs_to_balls(overs: u32, balls: u32, balls_per_over: u32) -> u64 {
    overs as u64 * balls_per_over as u64 + balls as u64
}

/// The inverse of `overs_to_balls` — a raw ball count back to whole overs +
/// balls, in the same `balls_per_over`.
fn balls_to_overs(balls: u64, balls_per_over: u32) -> OversRecord {
    OversRecord {
        overs: (balls / balls_per_over as u64) as u32,
        balls: (balls % balls_per_over as u64) as u32,
    }
}

/// Counts this player's goals scored (own goals excluded) and assists across
/// a confirmed football score's goal log. Best-candidates are "goals" and
/// "goal_contributions" (goals + assists in this match) — not "assists"
/// alone, since a single assist doesn't make as complete a "best game" record
/// as the combined tally.
fn football_contribution(score: &ScoreRecord, player_id: &str) -> SportContribution {
    let mut counters = HashMap::new();
    let ScoreRecord::Football { goals, .. } = score else {
        return SportContribution {
            counters,
            best_candidates: HashMap::new(),
            bowling_spell: None,
        };
    };
    for goal in goals.iter().flatten() {
        if !goal.own_goal && goal.scorer_player_id.as_deref() == Some(player_id) {
            *counters.entry("goals".to_string()).or_insert(0) += 1;
        }
        if goal.assist_player_id.as_deref() == Some(player_id) {
            *counters.entry("assists".to_string()).or_insert(0) += 1;
        }
    }

    let mut best_candidates = HashMap::new();
    if let Some(goals) = counters.get("goals") {
        best_candidates.insert("goals".to_string(), *goals);
    }
    let contributions = counters.get("goals").unwrap_or(&0) + counters.get("assists").unwrap_or(&0);
    if contributions > 0 {
        best_candidates.insert("goal_contributions".to_string(), contributions);
    }

    SportContribution {
        counters,
        best_candidates,
        bowling_spell: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of threading a match's real `balls_per_over` through
    /// instead of assuming 6: a 5-ball-over spell (The Hundred) must convert
    /// to its true ball count, not one that's off by however many overs it
    /// ran.
    #[test]
    fn overs_to_balls_respects_a_non_standard_balls_per_over() {
        // 3 overs + 4 balls at 5 balls/over = 19 balls, not 3*6+4 = 22.
        assert_eq!(overs_to_balls(3, 4, 5), 19);
        // The standard case still works as before.
        assert_eq!(overs_to_balls(8, 4, 6), 52);
    }

    /// `balls_to_overs` is the exact inverse of `overs_to_balls` for any
    /// `balls_per_over`, including a partial (not-yet-complete) over.
    #[test]
    fn balls_to_overs_round_trips_overs_to_balls() {
        for balls_per_over in [5, 6] {
            for overs in 0..10u32 {
                for balls in 0..balls_per_over {
                    let total = overs_to_balls(overs, balls, balls_per_over);
                    let round_tripped = balls_to_overs(total, balls_per_over);
                    assert_eq!(
                        round_tripped,
                        OversRecord { overs, balls },
                        "{overs}.{balls} at {balls_per_over}/over -> {total} balls -> {round_tripped:?}"
                    );
                }
            }
        }
    }

    /// A rolled-over accumulation (more balls than fit in the format's
    /// notion of "one over") still reduces correctly — this is exactly the
    /// case the fully-accumulated `bowling_spell.balls_bowled` hits when a
    /// player bowls in two innings of the same match.
    #[test]
    fn balls_to_overs_reduces_a_ball_count_bigger_than_one_over() {
        // 19 balls at 5/over = 3 overs, 4 balls (not the same shape you'd
        // get summing two innings' `Overs` field-by-field, which is exactly
        // why the reconciler sums raw balls first and converts once).
        assert_eq!(balls_to_overs(19, 5), OversRecord { overs: 3, balls: 4 });
    }
}
