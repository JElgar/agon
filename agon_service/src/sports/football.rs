//! Football's API↔DAO mapping — colocated here instead of scattered across
//! `mapping.rs` (see `crate::sports::netball`'s doc comment for the pattern
//! and its rationale). Plain functions rather than a `SportApi` trait impl,
//! same reasoning as netball's module.

use std::collections::HashMap;

use agon_core::dao::records::{
    FootballCardColorRecord, FootballCardEventRecord, FootballFormatRecord,
    FootballGoalEventRecord, FootballLiveEventRecord, FootballPenaltyShootoutKickRecord,
    FootballPeriodEventRecord, FootballPeriodRecord, FootballScoreRecord,
};

use crate::FootballScore;
use crate::detailed_score::football::{
    FootballCardColor, FootballCardEvent, FootballGoalEvent, FootballPenaltyShootoutKick,
    FootballPeriod, FootballSubstitutionEvent,
};
use crate::live_score::football::{FootballLiveEvent, FootballPeriodEvent};
use crate::mapping::{parse_ts, parse_ts_opt};
use crate::match_format::FootballFormat;

pub fn score_to_record(s: &FootballScore) -> FootballScoreRecord {
    FootballScoreRecord {
        score: s.score.clone(),
        goals: s
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_to_record).collect()),
        cards: s
            .cards
            .as_ref()
            .map(|cs| cs.iter().map(card_event_to_record).collect()),
        substitutions: s.substitutions.as_ref().map(|subs| {
            subs.iter()
                .map(substitution_event_to_record)
                .collect()
        }),
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
        penalty_shootout: s.penalty_shootout.as_ref().map(|ks| {
            ks.iter()
                .map(|k| FootballPenaltyShootoutKickRecord {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
                .collect()
        }),
        penalty_shootout_score: s.penalty_shootout_score.clone(),
    }
}

pub fn score_from_record(rec: &FootballScoreRecord) -> FootballScore {
    FootballScore {
        score: rec.score.clone(),
        goals: rec
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_from_record).collect()),
        cards: rec
            .cards
            .as_ref()
            .map(|cs| cs.iter().map(card_event_from_record).collect()),
        substitutions: rec.substitutions.as_ref().map(|subs| {
            subs.iter()
                .map(substitution_event_from_record)
                .collect()
        }),
        period: rec.period.as_ref().map(period_from_record),
        period_times: rec.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| (period_from_record(p), parse_ts(t)))
                .collect()
        }),
        penalty_shootout: rec.penalty_shootout.as_ref().map(|ks| {
            ks.iter()
                .map(|k| FootballPenaltyShootoutKick {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
                .collect()
        }),
        penalty_shootout_score: rec.penalty_shootout_score.clone(),
        // Not stored — `Api::hydrate_score_players` fills this afterward.
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &FootballFormat) -> FootballFormatRecord {
    FootballFormatRecord {
        half_length_minutes: f.half_length_minutes,
        num_halves: f.num_halves,
        extra_time: f.extra_time,
        extra_time_half_length_minutes: f.extra_time_half_length_minutes,
        penalties: f.penalties,
    }
}

pub fn format_from_record(f: &FootballFormatRecord) -> FootballFormat {
    FootballFormat {
        half_length_minutes: f.half_length_minutes,
        num_halves: f.num_halves,
        extra_time: f.extra_time,
        extra_time_half_length_minutes: f.extra_time_half_length_minutes,
        penalties: f.penalties,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above — both carry the same `FootballGoalEvent`/
/// `FootballGoalEventRecord` shape.
fn goal_event_to_record(g: &FootballGoalEvent) -> FootballGoalEventRecord {
    FootballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        assist_player_id: g.assist_player_id.clone(),
        own_goal: g.own_goal,
        penalty: g.penalty,
        minute: g.minute,
        occurred_at: g
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn goal_event_from_record(rec: &FootballGoalEventRecord) -> FootballGoalEvent {
    FootballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        assist_player_id: rec.assist_player_id.clone(),
        own_goal: rec.own_goal,
        penalty: rec.penalty,
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above, same reasoning as `goal_event_to_record`.
fn card_event_to_record(c: &FootballCardEvent) -> FootballCardEventRecord {
    FootballCardEventRecord {
        side_id: c.side_id.clone(),
        player_id: c.player_id.clone(),
        color: match c.color {
            FootballCardColor::Yellow => FootballCardColorRecord::Yellow,
            FootballCardColor::Red => FootballCardColorRecord::Red,
        },
        minute: c.minute,
        occurred_at: c
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn card_event_from_record(rec: &FootballCardEventRecord) -> FootballCardEvent {
    FootballCardEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        color: match rec.color {
            FootballCardColorRecord::Yellow => FootballCardColor::Yellow,
            FootballCardColorRecord::Red => FootballCardColor::Red,
        },
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn substitution_event_to_record(
    s: &FootballSubstitutionEvent,
) -> agon_core::dao::records::FootballSubstitutionEventRecord {
    agon_core::dao::records::FootballSubstitutionEventRecord {
        side_id: s.side_id.clone(),
        player_in_id: s.player_in_id.clone(),
        player_out_id: s.player_out_id.clone(),
        minute: s.minute,
        occurred_at: s
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn substitution_event_from_record(
    rec: &agon_core::dao::records::FootballSubstitutionEventRecord,
) -> FootballSubstitutionEvent {
    FootballSubstitutionEvent {
        side_id: rec.side_id.clone(),
        player_in_id: rec.player_in_id.clone(),
        player_out_id: rec.player_out_id.clone(),
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above (`period`/`period_times`' map keys) — both need
/// the same `FootballPeriod`/`FootballPeriodRecord` correspondence.
fn period_to_record(period: &FootballPeriod) -> FootballPeriodRecord {
    match period {
        FootballPeriod::KickOff => FootballPeriodRecord::KickOff,
        FootballPeriod::HalfTime => FootballPeriodRecord::HalfTime,
        FootballPeriod::SecondHalfKickOff => FootballPeriodRecord::SecondHalfKickOff,
        FootballPeriod::FullTime => FootballPeriodRecord::FullTime,
        FootballPeriod::ExtraTimeKickOff => FootballPeriodRecord::ExtraTimeKickOff,
        FootballPeriod::ExtraTimeHalfTime => FootballPeriodRecord::ExtraTimeHalfTime,
        FootballPeriod::ExtraTimeSecondHalfKickOff => {
            FootballPeriodRecord::ExtraTimeSecondHalfKickOff
        }
        FootballPeriod::ExtraTimeFullTime => FootballPeriodRecord::ExtraTimeFullTime,
        FootballPeriod::PenaltiesComplete => FootballPeriodRecord::PenaltiesComplete,
    }
}

fn period_from_record(rec: &FootballPeriodRecord) -> FootballPeriod {
    match rec {
        FootballPeriodRecord::KickOff => FootballPeriod::KickOff,
        FootballPeriodRecord::HalfTime => FootballPeriod::HalfTime,
        FootballPeriodRecord::SecondHalfKickOff => FootballPeriod::SecondHalfKickOff,
        FootballPeriodRecord::FullTime => FootballPeriod::FullTime,
        FootballPeriodRecord::ExtraTimeKickOff => FootballPeriod::ExtraTimeKickOff,
        FootballPeriodRecord::ExtraTimeHalfTime => FootballPeriod::ExtraTimeHalfTime,
        FootballPeriodRecord::ExtraTimeSecondHalfKickOff => {
            FootballPeriod::ExtraTimeSecondHalfKickOff
        }
        FootballPeriodRecord::ExtraTimeFullTime => FootballPeriod::ExtraTimeFullTime,
        FootballPeriodRecord::PenaltiesComplete => FootballPeriod::PenaltiesComplete,
    }
}

pub fn live_event_to_record(event: &FootballLiveEvent) -> FootballLiveEventRecord {
    match event {
        FootballLiveEvent::Goal(g) => FootballLiveEventRecord::Goal(goal_event_to_record(g)),
        FootballLiveEvent::Card(c) => FootballLiveEventRecord::Card(card_event_to_record(c)),
        FootballLiveEvent::Substitution(s) => {
            FootballLiveEventRecord::Substitution(substitution_event_to_record(s))
        }
        FootballLiveEvent::Period(p) => FootballLiveEventRecord::Period(FootballPeriodEventRecord {
            period: period_to_record(&p.period),
        }),
        FootballLiveEvent::PenaltyShootoutKick(k) => {
            FootballLiveEventRecord::PenaltyShootoutKick(FootballPenaltyShootoutKickRecord {
                side_id: k.side_id.clone(),
                scored: k.scored,
            })
        }
    }
}

pub fn live_event_from_record(rec: &FootballLiveEventRecord) -> FootballLiveEvent {
    match rec {
        FootballLiveEventRecord::Goal(g) => FootballLiveEvent::Goal(goal_event_from_record(g)),
        FootballLiveEventRecord::Card(c) => FootballLiveEvent::Card(card_event_from_record(c)),
        FootballLiveEventRecord::Substitution(s) => {
            FootballLiveEvent::Substitution(substitution_event_from_record(s))
        }
        FootballLiveEventRecord::Period(p) => FootballLiveEvent::Period(FootballPeriodEvent {
            period: period_from_record(&p.period),
        }),
        FootballLiveEventRecord::PenaltyShootoutKick(k) => {
            FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                side_id: k.side_id.clone(),
                scored: k.scored,
            })
        }
    }
}
