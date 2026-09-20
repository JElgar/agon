//! Football's `SportRecord` implementation.

use std::collections::HashMap;

use crate::dao::records::ScoreRecord;
use crate::sport::{SportContribution, SportRecord};

/// Marker type for football's `SportRecord` impl — see
/// `agon_core::sports::netball::NetballRecord`'s doc comment for the pattern.
pub struct FootballRecord;

impl SportRecord for FootballRecord {
    const NAME: &'static str = "football";

    /// Counts this player's goals scored (own goals excluded) and assists
    /// across a confirmed football score's goal log. Best-candidates are
    /// "goals" and "goal_contributions" (goals + assists in this match) —
    /// not "assists" alone, since a single assist doesn't make as complete a
    /// "best game" record as the combined tally. No `bowling_spell` — that's
    /// cricket-only.
    fn contribution(
        score: &ScoreRecord,
        player_id: &str,
        _balls_per_over: u32,
    ) -> SportContribution {
        let mut counters = HashMap::new();
        let ScoreRecord::Football(rec) = score else {
            return SportContribution::default();
        };
        for goal in rec.goals.iter().flatten() {
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
        let contributions =
            counters.get("goals").unwrap_or(&0) + counters.get("assists").unwrap_or(&0);
        if contributions > 0 {
            best_candidates.insert("goal_contributions".to_string(), contributions);
        }

        SportContribution {
            counters,
            best_candidates,
            bowling_spell: None,
        }
    }
}
