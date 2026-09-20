//! Netball's DAO record types and `SportRecord` implementation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dao::records::ScoreRecord;
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

/// Marker type for netball's `SportRecord` impl — no fields, just a home for
/// the trait's associated const/methods (same pattern `agon_service::sports::
/// netball::NetballSport` uses on the API side).
pub struct NetballRecord;

impl SportRecord for NetballRecord {
    const NAME: &'static str = "netball";

    /// Netball has no dedicated stats record yet — a per-player goal/foul log
    /// exists on `ScoreRecord::Netball` (see `NetballGoalEventRecord`) that a
    /// future `NetballStatsRecord` (mirroring cricket/football) could derive
    /// best-figures from, same as cricket/football — just not built out yet.
    /// Matches today's behavior (netball falls through the worker's `_ =>
    /// empty` default) exactly.
    fn contribution(
        _score: &ScoreRecord,
        _player_id: &str,
        _balls_per_over: u32,
    ) -> SportContribution {
        SportContribution::default()
    }
}
