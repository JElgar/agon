//! Cricket's `SportRecord` implementation.

use std::collections::HashMap;

use crate::dao::records::{CricketDismissalKindRecord, ScoreRecord};
use crate::dao::stats::BowlingSpell;
use crate::sport::{SportContribution, SportRecord};

/// Marker type for cricket's `SportRecord` impl — see
/// `agon_core::sports::netball::NetballRecord`'s doc comment for the pattern.
pub struct CricketRecord;

impl SportRecord for CricketRecord {
    const NAME: &'static str = "cricket";

    /// Sums this player's batting (runs/fours/sixes/balls faced/dismissals),
    /// fielding (catches), and bowling (wickets/runs conceded/balls bowled)
    /// across every innings of a confirmed cricket score. A player can
    /// feature in more than one innings (e.g. a two-innings match), so these
    /// accumulate rather than take the last entry.
    ///
    /// Only "runs" (high score) is a best-candidate here — "wickets" is
    /// tracked as a richer `bowling_spell` (best bowling figures need
    /// runs-conceded/balls-bowled alongside the wicket count, not just a bare
    /// scalar; see `Dao::update_best_bowling_figures`), and the rest (fours,
    /// sixes, balls faced, dismissals, catches, runs conceded, balls bowled)
    /// are lifetime totals only — nobody's asked to see "most balls faced in
    /// a game" as a record, so there's no reason to pay for tracking it.
    fn contribution(score: &ScoreRecord, player_id: &str, balls_per_over: u32) -> SportContribution {
        let mut counters = HashMap::new();
        let mut best_candidates = HashMap::new();
        let mut bowling_spell = BowlingSpell::default();
        let mut bowled = false;

        let ScoreRecord::Cricket(rec) = score else {
            return SportContribution::default();
        };

        for inning in &rec.innings {
            for entry in inning.batting.iter().flatten() {
                if entry.player_id == player_id {
                    *counters.entry("runs".to_string()).or_insert(0) += entry.runs as u64;
                    *counters.entry("fours".to_string()).or_insert(0) += entry.fours as u64;
                    *counters.entry("sixes".to_string()).or_insert(0) += entry.sixes as u64;
                    *counters.entry("balls_faced".to_string()).or_insert(0) +=
                        entry.balls_faced as u64;
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
}

/// Legal balls bowled, exact — uses this match's own `balls_per_over` (from
/// its `CricketFormatRecord`, or the standard 6 if unconfigured), not an
/// assumed one, so a 5-ball-over match (e.g. The Hundred) contributes its
/// true ball count rather than a slightly-off one.
fn overs_to_balls(overs: u32, balls: u32, balls_per_over: u32) -> u64 {
    overs as u64 * balls_per_over as u64 + balls as u64
}

/// The inverse of `overs_to_balls` — a raw ball count back to whole overs +
/// balls, in the same `balls_per_over`. Used by `agon_worker` after summing
/// `bowling_spell.balls_bowled` across every appearance (two `Overs` values
/// don't sum field-wise — balls can roll over into a whole extra over — so
/// the reconciler sums raw balls first and converts once via this).
pub fn balls_to_overs(balls: u64, balls_per_over: u32) -> crate::dao::records::OversRecord {
    crate::dao::records::OversRecord {
        overs: (balls / balls_per_over as u64) as u32,
        balls: (balls % balls_per_over as u64) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::records::OversRecord;

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
