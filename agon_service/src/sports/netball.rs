//! Netball's API↔DAO mapping — colocated here instead of scattered across
//! `mapping.rs` (see the project's sport-setup refactor design notes). Plain
//! functions rather than a `SportApi` trait impl for now: nothing yet
//! dispatches over sports generically (that's the later `define_sports!`
//! macro cutover, once every sport has moved here), so a trait would have no
//! caller — these are called directly from `mapping.rs`, the same way the
//! not-yet-migrated cricket/football functions still are.

use std::collections::HashMap;

use agon_core::dao::records::{
    NetballFormatRecord, NetballFoulEventRecord, NetballFoulKindRecord, NetballGoalEventRecord,
    NetballLiveEventRecord, NetballPeriodEventRecord, NetballPeriodRecord, NetballPositionRecord,
    NetballScoreRecord,
};

use crate::detailed_score::netball::{
    NetballFoulEvent, NetballFoulKind, NetballGoalEvent, NetballPeriod, NetballPosition,
};
use crate::NetballScore;
use crate::live_score::netball::{NetballLiveEvent, NetballPeriodEvent};
use crate::mapping::{parse_ts, parse_ts_opt};
use crate::match_format::NetballFormat;

pub fn score_to_record(s: &NetballScore) -> NetballScoreRecord {
    NetballScoreRecord {
        score: s.score.clone(),
        goals: s
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_to_record).collect()),
        fouls: s
            .fouls
            .as_ref()
            .map(|fs| fs.iter().map(foul_event_to_record).collect()),
        period: s.period.as_ref().map(period_to_record),
        period_times: s.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| {
                    (
                        period_to_record(p),
                        t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    )
                })
                .collect()
        }),
        period_scores: s.period_scores.as_ref().map(|pss| {
            pss.iter()
                .map(|(p, entries)| (period_to_record(p), entries.clone()))
                .collect()
        }),
    }
}

pub fn score_from_record(rec: &NetballScoreRecord) -> NetballScore {
    NetballScore {
        score: rec.score.clone(),
        goals: rec
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_from_record).collect()),
        fouls: rec
            .fouls
            .as_ref()
            .map(|fs| fs.iter().map(foul_event_from_record).collect()),
        period: rec.period.as_ref().map(period_from_record),
        period_times: rec.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| (period_from_record(p), parse_ts(t)))
                .collect()
        }),
        period_scores: rec.period_scores.as_ref().map(|pss| {
            pss.iter()
                .map(|(p, entries)| (period_from_record(p), entries.clone()))
                .collect()
        }),
        // Not stored — `Api::hydrate_score_players` fills this afterward.
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &NetballFormat) -> NetballFormatRecord {
    NetballFormatRecord {
        num_quarters: f.num_quarters,
        quarter_length_minutes: f.quarter_length_minutes,
        two_point_zone: f.two_point_zone,
        extra_time: f.extra_time,
    }
}

pub fn format_from_record(f: &NetballFormatRecord) -> NetballFormat {
    NetballFormat {
        num_quarters: f.num_quarters,
        quarter_length_minutes: f.quarter_length_minutes,
        two_point_zone: f.two_point_zone,
        extra_time: f.extra_time,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above — both carry the same `NetballGoalEvent`/
/// `NetballGoalEventRecord` shape.
fn goal_event_to_record(g: &NetballGoalEvent) -> NetballGoalEventRecord {
    NetballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        scorer_position: g.scorer_position.as_ref().map(position_to_record),
        two_points: g.two_points,
        minute: g.minute,
        occurred_at: g
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn goal_event_from_record(rec: &NetballGoalEventRecord) -> NetballGoalEvent {
    NetballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        scorer_position: rec.scorer_position.as_ref().map(position_from_record),
        two_points: rec.two_points,
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn position_to_record(p: &NetballPosition) -> NetballPositionRecord {
    match p {
        NetballPosition::GoalShooter => NetballPositionRecord::GoalShooter,
        NetballPosition::GoalAttack => NetballPositionRecord::GoalAttack,
        NetballPosition::WingAttack => NetballPositionRecord::WingAttack,
        NetballPosition::Centre => NetballPositionRecord::Centre,
        NetballPosition::WingDefence => NetballPositionRecord::WingDefence,
        NetballPosition::GoalDefence => NetballPositionRecord::GoalDefence,
        NetballPosition::GoalKeeper => NetballPositionRecord::GoalKeeper,
    }
}

fn position_from_record(rec: &NetballPositionRecord) -> NetballPosition {
    match rec {
        NetballPositionRecord::GoalShooter => NetballPosition::GoalShooter,
        NetballPositionRecord::GoalAttack => NetballPosition::GoalAttack,
        NetballPositionRecord::WingAttack => NetballPosition::WingAttack,
        NetballPositionRecord::Centre => NetballPosition::Centre,
        NetballPositionRecord::WingDefence => NetballPosition::WingDefence,
        NetballPositionRecord::GoalDefence => NetballPosition::GoalDefence,
        NetballPositionRecord::GoalKeeper => NetballPosition::GoalKeeper,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above, same reasoning as `goal_event_to_record`.
fn foul_event_to_record(fo: &NetballFoulEvent) -> NetballFoulEventRecord {
    NetballFoulEventRecord {
        side_id: fo.side_id.clone(),
        player_id: fo.player_id.clone(),
        foul_kind: match fo.foul_kind {
            NetballFoulKind::Contact => NetballFoulKindRecord::Contact,
            NetballFoulKind::Obstruction => NetballFoulKindRecord::Obstruction,
            NetballFoulKind::Footwork => NetballFoulKindRecord::Footwork,
            NetballFoulKind::Offside => NetballFoulKindRecord::Offside,
            NetballFoulKind::HeldBall => NetballFoulKindRecord::HeldBall,
            NetballFoulKind::Other => NetballFoulKindRecord::Other,
        },
        minute: fo.minute,
        occurred_at: fo
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn foul_event_from_record(rec: &NetballFoulEventRecord) -> NetballFoulEvent {
    NetballFoulEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        foul_kind: match rec.foul_kind {
            NetballFoulKindRecord::Contact => NetballFoulKind::Contact,
            NetballFoulKindRecord::Obstruction => NetballFoulKind::Obstruction,
            NetballFoulKindRecord::Footwork => NetballFoulKind::Footwork,
            NetballFoulKindRecord::Offside => NetballFoulKind::Offside,
            NetballFoulKindRecord::HeldBall => NetballFoulKind::HeldBall,
            NetballFoulKindRecord::Other => NetballFoulKind::Other,
        },
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above (`period`/`period_times`/`period_scores`' map
/// keys) — both need the same `NetballPeriod`/`NetballPeriodRecord`
/// correspondence.
fn period_to_record(period: &NetballPeriod) -> NetballPeriodRecord {
    match period {
        NetballPeriod::Start => NetballPeriodRecord::Start,
        NetballPeriod::QuarterOneEnd => NetballPeriodRecord::QuarterOneEnd,
        NetballPeriod::QuarterTwoStart => NetballPeriodRecord::QuarterTwoStart,
        NetballPeriod::QuarterTwoEnd => NetballPeriodRecord::QuarterTwoEnd,
        NetballPeriod::QuarterThreeStart => NetballPeriodRecord::QuarterThreeStart,
        NetballPeriod::QuarterThreeEnd => NetballPeriodRecord::QuarterThreeEnd,
        NetballPeriod::QuarterFourStart => NetballPeriodRecord::QuarterFourStart,
        NetballPeriod::FullTime => NetballPeriodRecord::FullTime,
        NetballPeriod::ExtraTimeStart => NetballPeriodRecord::ExtraTimeStart,
        NetballPeriod::ExtraTimeEnd => NetballPeriodRecord::ExtraTimeEnd,
    }
}

fn period_from_record(rec: &NetballPeriodRecord) -> NetballPeriod {
    match rec {
        NetballPeriodRecord::Start => NetballPeriod::Start,
        NetballPeriodRecord::QuarterOneEnd => NetballPeriod::QuarterOneEnd,
        NetballPeriodRecord::QuarterTwoStart => NetballPeriod::QuarterTwoStart,
        NetballPeriodRecord::QuarterTwoEnd => NetballPeriod::QuarterTwoEnd,
        NetballPeriodRecord::QuarterThreeStart => NetballPeriod::QuarterThreeStart,
        NetballPeriodRecord::QuarterThreeEnd => NetballPeriod::QuarterThreeEnd,
        NetballPeriodRecord::QuarterFourStart => NetballPeriod::QuarterFourStart,
        NetballPeriodRecord::FullTime => NetballPeriod::FullTime,
        NetballPeriodRecord::ExtraTimeStart => NetballPeriod::ExtraTimeStart,
        NetballPeriodRecord::ExtraTimeEnd => NetballPeriod::ExtraTimeEnd,
    }
}

pub fn live_event_to_record(event: &NetballLiveEvent) -> NetballLiveEventRecord {
    match event {
        NetballLiveEvent::Goal(g) => NetballLiveEventRecord::Goal(goal_event_to_record(g)),
        NetballLiveEvent::Foul(fo) => NetballLiveEventRecord::Foul(foul_event_to_record(fo)),
        NetballLiveEvent::Period(p) => NetballLiveEventRecord::Period(NetballPeriodEventRecord {
            period: period_to_record(&p.period),
            score: p.score.clone(),
        }),
    }
}

pub fn live_event_from_record(rec: &NetballLiveEventRecord) -> NetballLiveEvent {
    match rec {
        NetballLiveEventRecord::Goal(g) => NetballLiveEvent::Goal(goal_event_from_record(g)),
        NetballLiveEventRecord::Foul(fo) => NetballLiveEvent::Foul(foul_event_from_record(fo)),
        NetballLiveEventRecord::Period(p) => NetballLiveEvent::Period(NetballPeriodEvent {
            period: period_from_record(&p.period),
            score: p.score.clone(),
        }),
    }
}
