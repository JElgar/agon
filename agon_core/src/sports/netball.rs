//! Netball's whole DAO-side surface — see `crate::sports::cricket`'s doc
//! comment.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dao::records::{GenericSportStatsRecord, MatchFormatRecord, ScoreRecord};
use crate::sport::{SportContribution, SportRecord};

/// Netball's `ScoreRecord` shape — pulled out to its own type (rather than an
/// inline enum-variant struct, like `Cricket`/`Football` originally were) as
/// the first sport migrated onto the `agon_core::sport::SportRecord` trait
/// (see the project's sport-setup refactor design notes). Wire-identical to
/// the inline shape it replaced — serde's internally-tagged representation
/// serializes a newtype-around-a-struct the same as a struct-variant with the
/// same fields — guarded by `agon_core/tests/netball_score_record_roundtrip.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballScoreRecord {
    /// Goal tally, keyed by side id.
    #[serde(default)]
    pub score: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goals: Option<Vec<NetballGoalEventRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fouls: Option<Vec<NetballFoulEventRecord>>,
    /// The most recent period marker seen, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<NetballPeriodRecord>,
    /// When each period marker was recorded, keyed by kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_times: Option<HashMap<NetballPeriodRecord, String>>,
    /// The score as of each quarter-end marker, keyed by kind — the
    /// *only* source of the score for a quarter-only-scored match. See
    /// `agon_service::live_score::netball::NetballPeriodEvent::score`'s
    /// doc comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_scores: Option<HashMap<NetballPeriodRecord, HashMap<String, u32>>>,
}

/// Mirrors `agon_service::match_format::NetballFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballFormatRecord {
    pub num_quarters: u32,
    pub quarter_length_minutes: u32,
    pub two_point_zone: bool,
    pub extra_time: bool,
}

// ---- Netball live events ----------------------------------------------------

/// Mirrors `agon_service::live_score::netball::NetballLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetballLiveEventRecord {
    Goal(NetballGoalEventRecord),
    Foul(NetballFoulEventRecord),
    Period(NetballPeriodEventRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballGoalEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_position: Option<NetballPositionRecord>,
    pub two_points: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `agon_service::detailed_score::netball::NetballGoalEvent::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetballPositionRecord {
    GoalShooter,
    GoalAttack,
    WingAttack,
    Centre,
    WingDefence,
    GoalDefence,
    GoalKeeper,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballFoulEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>,
    /// Named `foul_kind`, not `kind` — same collision-avoidance as the API's
    /// `NetballFoulEvent::foul_kind` (`NetballLiveEventRecord`'s own
    /// `#[serde(tag = "kind")]` would otherwise fight this field over the
    /// same wire key once serde flattens the variant's fields in).
    pub foul_kind: NetballFoulKindRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `NetballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetballFoulKindRecord {
    Contact,
    Obstruction,
    Footwork,
    Offside,
    HeldBall,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballPeriodEventRecord {
    pub period: NetballPeriodRecord,
    /// Cumulative score per side as of this marker — always present, same
    /// reasoning as `agon_service::live_score::netball::NetballPeriodEvent`.
    pub score: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NetballPeriodRecord {
    Start,
    QuarterOneEnd,
    QuarterTwoStart,
    QuarterTwoEnd,
    QuarterThreeStart,
    QuarterThreeEnd,
    QuarterFourStart,
    FullTime,
    ExtraTimeStart,
    ExtraTimeEnd,
}

/// Lifetime netball stats (`stats.netball` on the user's profile): the
/// common counters plus goals scored, derived from every confirmed match's
/// goal log.
///
/// Wire-compatible with the `GenericSportStatsRecord` shape `stats.netball`
/// used before this type existed — the common counters are flattened in at
/// the same keys and `goals` defaults to `0` when absent — guarded by
/// `agon_core/tests/netball_stats_record_roundtrip.rs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NetballStatsRecord {
    #[serde(flatten)]
    pub common: GenericSportStatsRecord,
    /// Goals scored (career total) — a count of successful shots, so a
    /// two-point-zone goal counts once here even though it's worth two on
    /// the scoreboard. Sparse like every other counter: absent until the
    /// player's first goal.
    #[serde(default)]
    pub goals: u64,
}

/// Marker type for netball's `SportRecord` impl — no fields, just a home for
/// the trait's methods (the same per-sport marker pattern cricket/football
/// use).
pub struct NetballRecord;

impl SportRecord for NetballRecord {
    /// Counts the goals this player scored across a confirmed netball
    /// score's goal log. Nothing to count for a quarter-only-scored or
    /// manually-entered result with no goal-by-goal detail — the player still
    /// gets their played/won/drawn/lost from the outcome, just no goals.
    fn contribution(
        score: &ScoreRecord,
        player_id: &str,
        _format: Option<&MatchFormatRecord>,
    ) -> SportContribution {
        let ScoreRecord::Netball(rec) = score else {
            return SportContribution::default();
        };
        let goals = rec
            .goals
            .iter()
            .flatten()
            .filter(|g| g.scorer_player_id.as_deref() == Some(player_id))
            .count() as u64;

        let mut counters = HashMap::new();
        if goals > 0 {
            counters.insert("goals".to_string(), goals);
        }
        SportContribution {
            counters,
            ..SportContribution::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal(scorer: Option<&str>, two_points: bool) -> NetballGoalEventRecord {
        NetballGoalEventRecord {
            side_id: "kestrels".into(),
            scorer_player_id: scorer.map(Into::into),
            scorer_position: None,
            two_points,
            minute: None,
            occurred_at: None,
        }
    }

    fn score(goals: Option<Vec<NetballGoalEventRecord>>) -> ScoreRecord {
        ScoreRecord::Netball(NetballScoreRecord {
            score: HashMap::new(),
            goals,
            fouls: None,
            period: None,
            period_times: None,
            period_scores: None,
        })
    }

    #[test]
    fn counts_only_this_players_goals_with_a_two_pointer_counting_once() {
        let s = score(Some(vec![
            goal(Some("p1"), false),
            goal(Some("p1"), true),
            goal(Some("p2"), false),
            goal(None, false),
        ]));
        let c = NetballRecord::contribution(&s, "p1", None);
        assert_eq!(c.counters.get("goals"), Some(&2));
        assert!(c.best_candidates.is_empty());
        assert!(c.bowling_spell.is_none());
    }

    #[test]
    fn a_player_with_no_goals_contributes_no_goals_counter() {
        let c =
            NetballRecord::contribution(&score(Some(vec![goal(Some("p2"), false)])), "p1", None);
        assert!(c.counters.is_empty());
        // Quarter-only / manual result: no goal log at all.
        let c = NetballRecord::contribution(&score(None), "p1", None);
        assert!(c.counters.is_empty());
    }
}
