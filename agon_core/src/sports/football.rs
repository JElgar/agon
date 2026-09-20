//! Football's DAO record types and `SportRecord` implementation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dao::records::ScoreRecord;
use crate::sport::{SportContribution, SportRecord};

/// Football's `ScoreRecord` shape — see
/// `crate::sports::netball::NetballScoreRecord`'s doc comment for why this is
/// a standalone type rather than an inline enum-variant struct: wire-identical
/// to the inline shape it replaced, guarded by
/// `agon_core/tests/football_score_record_roundtrip.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballScoreRecord {
    /// Goal tally, keyed by side id. `#[serde(default)]` because this
    /// field didn't exist before the tally was embedded here — a record
    /// written in that gap has no `score` to fall back to, so it
    /// deserializes as an empty tally rather than 500ing.
    #[serde(default)]
    pub score: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goals: Option<Vec<FootballGoalEventRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<FootballCardEventRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitutions: Option<Vec<FootballSubstitutionEventRecord>>,
    /// The most recent period marker seen, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<FootballPeriodRecord>,
    /// When each period marker was recorded, keyed by kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_times: Option<HashMap<FootballPeriodRecord, String>>,
    /// Every penalty-shootout kick recorded, in order taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub penalty_shootout: Option<Vec<FootballPenaltyShootoutKickRecord>>,
    /// Running shootout tally (kicks scored, not taken), keyed by side id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub penalty_shootout_score: Option<HashMap<String, u32>>,
}

/// Mirrors `agon_service::match_format::FootballFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballFormatRecord {
    pub half_length_minutes: u32,
    pub num_halves: u32,
    pub extra_time: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_time_half_length_minutes: Option<u32>,
    pub penalties: bool,
}

// ---- Football live events --------------------------------------------------

/// Mirrors `agon_service::live_score::football::FootballLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FootballLiveEventRecord {
    Goal(FootballGoalEventRecord),
    Card(FootballCardEventRecord),
    Substitution(FootballSubstitutionEventRecord),
    Period(FootballPeriodEventRecord),
    PenaltyShootoutKick(FootballPenaltyShootoutKickRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballGoalEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `agon_service::detailed_score::football::FootballGoalEvent::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballCardEventRecord {
    pub side_id: String,
    pub player_id: String,
    pub color: FootballCardColorRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `FootballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FootballCardColorRecord {
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballSubstitutionEventRecord {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `FootballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballPeriodEventRecord {
    pub period: FootballPeriodRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FootballPeriodRecord {
    KickOff,
    HalfTime,
    SecondHalfKickOff,
    FullTime,
    ExtraTimeKickOff,
    ExtraTimeHalfTime,
    ExtraTimeSecondHalfKickOff,
    ExtraTimeFullTime,
    PenaltiesComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballPenaltyShootoutKickRecord {
    pub side_id: String,
    pub scored: bool,
}

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
