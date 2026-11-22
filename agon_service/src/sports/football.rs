//! Football: its `Sport` impl. `Score`/`Format`/`LiveEvent` are
//! `agon_core::football`'s own types, used directly as the API's wire types
//! (via `agon_core`'s `openapi` feature) rather than mirrored here — see
//! that crate for the fold logic and its tests.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use poem_openapi::Object;

use agon_core::football as scoring;
use agon_services_core::sports::football::Football;
use agon_services_core::sports::football::{
    FootballCardColorRecord, FootballCardEventRecord, FootballFormatRecord,
    FootballGoalEventRecord, FootballLiveEventRecord, FootballPenaltyShootoutKickRecord,
    FootballPeriodEventRecord, FootballPeriodRecord, FootballScoreRecord, FootballStatsRecord,
    FootballSubstitutionEventRecord,
};

use crate::mapping::{best_figure_from_record, generic_stats_from_record, parse_ts, parse_ts_opt};
use crate::sports::Sport;
use crate::{BestFigure, GenericPlayerStats};

impl Sport for Football {
    type Score = FootballScore;
    type Format = scoring::Format;
    type LiveEvent = scoring::LiveEvent;
    type PlayerStats = FootballPlayerStats;

    fn new_score() -> FootballScore {
        FootballScore {
            inner: scoring::Score::new(),
            players: HashMap::new(),
        }
    }

    fn apply_event(
        score: &mut FootballScore,
        occurred_at: DateTime<Utc>,
        event: &scoring::LiveEvent,
        format: Option<&scoring::Format>,
    ) {
        scoring::apply_event(&mut score.inner, occurred_at, event, format);
    }

    fn winner(score: &FootballScore, side_ids: &[String]) -> Option<String> {
        scoring::winner(&score.inner, side_ids)
    }

    fn side_ids(score: &FootballScore) -> Vec<&str> {
        score.inner.score.keys().map(|k| k.as_str()).collect()
    }

    /// Each goal's scorer and assist, each card's player, and each
    /// substitution's players in and out.
    fn player_ids(score: &FootballScore) -> Vec<String> {
        let mut ids = Vec::new();
        for goal in score.inner.goals.iter().flatten() {
            ids.extend(goal.scorer_player_id.clone());
            ids.extend(goal.assist_player_id.clone());
        }
        for card in score.inner.cards.iter().flatten() {
            ids.push(card.player_id.clone());
        }
        for sub in score.inner.substitutions.iter().flatten() {
            ids.push(sub.player_in_id.clone());
            ids.push(sub.player_out_id.clone());
        }
        ids
    }

    fn resolve_ids(
        s: &FootballScore,
        side_ids: &HashMap<String, String>,
        player_ids: &HashMap<String, String>,
    ) -> Option<FootballScore> {
        let s = &s.inner;
        let map = |client_id: &str| side_ids.get(client_id).cloned();
        let pmap = |client_id: &str| player_ids.get(client_id).cloned();

        let mut score = HashMap::with_capacity(s.score.len());
        for (side_id, goals) in &s.score {
            score.insert(map(side_id)?, *goals);
        }
        let goals = match &s.goals {
            Some(gs) => {
                let mut out = Vec::with_capacity(gs.len());
                for g in gs {
                    out.push(scoring::GoalEvent {
                        side_id: map(&g.side_id)?,
                        scorer_player_id: match &g.scorer_player_id {
                            Some(id) => Some(pmap(id)?),
                            None => None,
                        },
                        assist_player_id: match &g.assist_player_id {
                            Some(id) => Some(pmap(id)?),
                            None => None,
                        },
                        own_goal: g.own_goal,
                        penalty: g.penalty,
                        minute: g.minute,
                        occurred_at: g.occurred_at,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let cards = match &s.cards {
            Some(cs) => {
                let mut out = Vec::with_capacity(cs.len());
                for c in cs {
                    out.push(scoring::CardEvent {
                        side_id: map(&c.side_id)?,
                        player_id: pmap(&c.player_id)?,
                        color: c.color,
                        minute: c.minute,
                        occurred_at: c.occurred_at,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let substitutions = match &s.substitutions {
            Some(subs) => {
                let mut out = Vec::with_capacity(subs.len());
                for sub in subs {
                    out.push(scoring::SubstitutionEvent {
                        side_id: map(&sub.side_id)?,
                        player_in_id: pmap(&sub.player_in_id)?,
                        player_out_id: pmap(&sub.player_out_id)?,
                        minute: sub.minute,
                        occurred_at: sub.occurred_at,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let penalty_shootout = match &s.penalty_shootout {
            Some(ks) => {
                let mut out = Vec::with_capacity(ks.len());
                for k in ks {
                    out.push(scoring::PenaltyShootoutKick {
                        side_id: map(&k.side_id)?,
                        scored: k.scored,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let penalty_shootout_score = match &s.penalty_shootout_score {
            Some(pss) => {
                let mut out = HashMap::with_capacity(pss.len());
                for (side_id, kicks) in pss {
                    out.insert(map(side_id)?, *kicks);
                }
                Some(out)
            }
            None => None,
        };
        Some(FootballScore {
            inner: scoring::Score {
                score,
                goals,
                cards,
                substitutions,
                period: s.period,
                period_times: s.period_times.clone(),
                penalty_shootout,
                penalty_shootout_score,
            },
            players: HashMap::new(),
        })
    }

    fn set_players(
        score: &mut FootballScore,
        resolved: HashMap<String, crate::RosterPreviewPlayer>,
    ) {
        score.players = resolved;
    }

    fn score_to_record(s: &FootballScore) -> FootballScoreRecord {
        let s = &s.inner;
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
            substitutions: s
                .substitutions
                .as_ref()
                .map(|subs| subs.iter().map(substitution_event_to_record).collect()),
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

    fn score_from_record(rec: &FootballScoreRecord) -> FootballScore {
        FootballScore {
            inner: scoring::Score {
                score: rec.score.clone(),
                goals: rec
                    .goals
                    .as_ref()
                    .map(|gs| gs.iter().map(goal_event_from_record).collect()),
                cards: rec
                    .cards
                    .as_ref()
                    .map(|cs| cs.iter().map(card_event_from_record).collect()),
                substitutions: rec
                    .substitutions
                    .as_ref()
                    .map(|subs| subs.iter().map(substitution_event_from_record).collect()),
                period: rec.period.as_ref().map(period_from_record),
                period_times: rec.period_times.as_ref().map(|pts| {
                    pts.iter()
                        .map(|(p, t)| (period_from_record(p), parse_ts(t)))
                        .collect()
                }),
                penalty_shootout: rec.penalty_shootout.as_ref().map(|ks| {
                    ks.iter()
                        .map(|k| scoring::PenaltyShootoutKick {
                            side_id: k.side_id.clone(),
                            scored: k.scored,
                        })
                        .collect()
                }),
                penalty_shootout_score: rec.penalty_shootout_score.clone(),
            },
            // Not stored — `Api::hydrate_score_players` fills this afterward.
            players: HashMap::new(),
        }
    }

    fn format_to_record(f: &scoring::Format) -> FootballFormatRecord {
        FootballFormatRecord {
            half_length_minutes: f.half_length_minutes,
            num_halves: f.num_halves,
            extra_time: f.extra_time,
            extra_time_half_length_minutes: f.extra_time_half_length_minutes,
            penalties: f.penalties,
        }
    }

    fn format_from_record(f: &FootballFormatRecord) -> scoring::Format {
        scoring::Format {
            half_length_minutes: f.half_length_minutes,
            num_halves: f.num_halves,
            extra_time: f.extra_time,
            extra_time_half_length_minutes: f.extra_time_half_length_minutes,
            penalties: f.penalties,
        }
    }

    fn live_event_to_record(event: &scoring::LiveEvent) -> FootballLiveEventRecord {
        match event {
            scoring::LiveEvent::Goal(g) => FootballLiveEventRecord::Goal(goal_event_to_record(g)),
            scoring::LiveEvent::Card(c) => FootballLiveEventRecord::Card(card_event_to_record(c)),
            scoring::LiveEvent::Substitution(s) => {
                FootballLiveEventRecord::Substitution(substitution_event_to_record(s))
            }
            scoring::LiveEvent::Period(p) => {
                FootballLiveEventRecord::Period(FootballPeriodEventRecord {
                    period: period_to_record(&p.period),
                })
            }
            scoring::LiveEvent::PenaltyShootoutKick(k) => {
                FootballLiveEventRecord::PenaltyShootoutKick(FootballPenaltyShootoutKickRecord {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
            }
        }
    }

    fn live_event_from_record(rec: &FootballLiveEventRecord) -> scoring::LiveEvent {
        match rec {
            FootballLiveEventRecord::Goal(g) => scoring::LiveEvent::Goal(goal_event_from_record(g)),
            FootballLiveEventRecord::Card(c) => scoring::LiveEvent::Card(card_event_from_record(c)),
            FootballLiveEventRecord::Substitution(s) => {
                scoring::LiveEvent::Substitution(substitution_event_from_record(s))
            }
            FootballLiveEventRecord::Period(p) => {
                scoring::LiveEvent::Period(scoring::PeriodEvent {
                    period: period_from_record(&p.period),
                })
            }
            FootballLiveEventRecord::PenaltyShootoutKick(k) => {
                scoring::LiveEvent::PenaltyShootoutKick(scoring::PenaltyShootoutKick {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
            }
        }
    }

    fn stats_from_record(rec: &FootballStatsRecord) -> FootballPlayerStats {
        FootballPlayerStats {
            common: generic_stats_from_record(&rec.common),
            goals: rec.goals as i32,
            assists: rec.assists as i32,
            best_goals: rec.best_goals.as_ref().map(best_figure_from_record),
            best_goal_contributions: rec
                .best_goal_contributions
                .as_ref()
                .map(best_figure_from_record),
        }
    }
}

// ---- API types ----------------------------------------------------------

/// A football match's result: `agon_core`'s score, plus resolved player
/// display data — the one field the shared fold has no use for, so it's
/// flattened on here rather than carried in `agon_core` itself.
#[derive(Object)]
pub(crate) struct FootballScore {
    #[oai(flatten)]
    inner: scoring::Score,
    /// Live name/avatar for every player id referenced anywhere in `inner`
    /// — `goals`' scorer/assist, `cards`' player, `substitutions`' in/out —
    /// keyed by that same (match-scoped) player id. Same mechanism and
    /// rationale as `CricketScore.players`.
    players: HashMap<String, crate::RosterPreviewPlayer>,
}

/// Lifetime football stats: the common counters plus goals/assists derived
/// from every confirmed match's goal log.
#[derive(Object)]
pub struct FootballPlayerStats {
    #[oai(flatten)]
    pub common: GenericPlayerStats,
    pub goals: i32,
    pub assists: i32,
    /// Most goals scored in a single match.
    pub best_goals: Option<BestFigure>,
    /// Most goals + assists combined in a single match — a more complete
    /// "best game" than assists alone.
    pub best_goal_contributions: Option<BestFigure>,
}

// ---- Record mapping helpers ---------------------------------------------

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above — both carry the same `GoalEvent`/
/// `FootballGoalEventRecord` shape.
fn goal_event_to_record(g: &scoring::GoalEvent) -> FootballGoalEventRecord {
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

fn goal_event_from_record(rec: &FootballGoalEventRecord) -> scoring::GoalEvent {
    scoring::GoalEvent {
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
fn card_event_to_record(c: &scoring::CardEvent) -> FootballCardEventRecord {
    FootballCardEventRecord {
        side_id: c.side_id.clone(),
        player_id: c.player_id.clone(),
        color: match c.color {
            scoring::CardColor::Yellow => FootballCardColorRecord::Yellow,
            scoring::CardColor::Red => FootballCardColorRecord::Red,
        },
        minute: c.minute,
        occurred_at: c
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn card_event_from_record(rec: &FootballCardEventRecord) -> scoring::CardEvent {
    scoring::CardEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        color: match rec.color {
            FootballCardColorRecord::Yellow => scoring::CardColor::Yellow,
            FootballCardColorRecord::Red => scoring::CardColor::Red,
        },
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn substitution_event_to_record(s: &scoring::SubstitutionEvent) -> FootballSubstitutionEventRecord {
    FootballSubstitutionEventRecord {
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
    rec: &FootballSubstitutionEventRecord,
) -> scoring::SubstitutionEvent {
    scoring::SubstitutionEvent {
        side_id: rec.side_id.clone(),
        player_in_id: rec.player_in_id.clone(),
        player_out_id: rec.player_out_id.clone(),
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above (`period`/`period_times`' map keys) — both need
/// the same `Period`/`FootballPeriodRecord` correspondence.
fn period_to_record(period: &scoring::Period) -> FootballPeriodRecord {
    match period {
        scoring::Period::KickOff => FootballPeriodRecord::KickOff,
        scoring::Period::HalfTime => FootballPeriodRecord::HalfTime,
        scoring::Period::SecondHalfKickOff => FootballPeriodRecord::SecondHalfKickOff,
        scoring::Period::FullTime => FootballPeriodRecord::FullTime,
        scoring::Period::ExtraTimeKickOff => FootballPeriodRecord::ExtraTimeKickOff,
        scoring::Period::ExtraTimeHalfTime => FootballPeriodRecord::ExtraTimeHalfTime,
        scoring::Period::ExtraTimeSecondHalfKickOff => {
            FootballPeriodRecord::ExtraTimeSecondHalfKickOff
        }
        scoring::Period::ExtraTimeFullTime => FootballPeriodRecord::ExtraTimeFullTime,
        scoring::Period::PenaltiesComplete => FootballPeriodRecord::PenaltiesComplete,
    }
}

fn period_from_record(rec: &FootballPeriodRecord) -> scoring::Period {
    match rec {
        FootballPeriodRecord::KickOff => scoring::Period::KickOff,
        FootballPeriodRecord::HalfTime => scoring::Period::HalfTime,
        FootballPeriodRecord::SecondHalfKickOff => scoring::Period::SecondHalfKickOff,
        FootballPeriodRecord::FullTime => scoring::Period::FullTime,
        FootballPeriodRecord::ExtraTimeKickOff => scoring::Period::ExtraTimeKickOff,
        FootballPeriodRecord::ExtraTimeHalfTime => scoring::Period::ExtraTimeHalfTime,
        FootballPeriodRecord::ExtraTimeSecondHalfKickOff => {
            scoring::Period::ExtraTimeSecondHalfKickOff
        }
        FootballPeriodRecord::ExtraTimeFullTime => scoring::Period::ExtraTimeFullTime,
        FootballPeriodRecord::PenaltiesComplete => scoring::Period::PenaltiesComplete,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use poem_openapi::types::ToJSON;

    use super::*;
    use crate::live_score::LiveEventInput;
    use crate::mapping::{
        live_event_input_to_record, live_event_payload_from_record, match_format_from_record,
        match_format_to_record,
    };
    use crate::match_format::MatchFormat;

    fn ts(s: &str) -> DateTime<Utc> {
        parse_ts(s)
    }

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    /// The fold logic itself (`apply_event`/`winner`) is tested directly in
    /// `agon_core`, not re-tested here through this zero-logic delegation.
    #[test]
    fn football_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            scoring::LiveEvent::Goal(scoring::GoalEvent {
                side_id: "riverside".into(),
                scorer_player_id: Some("alvarez".into()),
                assist_player_id: None,
                own_goal: true,
                penalty: false,
                minute: Some(63),
                occurred_at: Some(ts("2024-05-01T20:03:00.000Z")),
            }),
            scoring::LiveEvent::Card(scoring::CardEvent {
                side_id: "oak_park".into(),
                player_id: "khan".into(),
                color: scoring::CardColor::Red,
                minute: None,
                occurred_at: None,
            }),
            scoring::LiveEvent::Substitution(scoring::SubstitutionEvent {
                side_id: "oak_park".into(),
                player_in_id: "moreno".into(),
                player_out_id: "khan".into(),
                minute: Some(70),
                occurred_at: Some(ts("2024-05-01T20:10:00.000Z")),
            }),
            scoring::LiveEvent::Period(scoring::PeriodEvent {
                period: scoring::Period::ExtraTimeKickOff,
            }),
            scoring::LiveEvent::Period(scoring::PeriodEvent {
                period: scoring::Period::ExtraTimeSecondHalfKickOff,
            }),
            scoring::LiveEvent::Period(scoring::PeriodEvent {
                period: scoring::Period::ExtraTimeFullTime,
            }),
            scoring::LiveEvent::PenaltyShootoutKick(scoring::PenaltyShootoutKick {
                side_id: "riverside".into(),
                scored: true,
            }),
        ];

        for event in events {
            let input = LiveEventInput::Football(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn football_format_round_trips_through_dao_mirror() {
        let format = MatchFormat::Football(scoring::Format {
            half_length_minutes: 40,
            num_halves: 2,
            extra_time: true,
            extra_time_half_length_minutes: Some(15),
            penalties: true,
        });
        let original_json = format.to_json();
        let round_tripped = match_format_from_record(&match_format_to_record(&format));
        assert_eq!(original_json, round_tripped.to_json());
    }

    #[test]
    fn football_score_round_trips_through_dao_mirror() {
        use crate::Score;

        let scores = vec![
            // A manually-entered football result: totals only, no detail.
            Score::Football(FootballScore {
                inner: scoring::Score {
                    score: HashMap::from([
                        ("side_red".to_string(), 3),
                        ("side_blue".to_string(), 1),
                    ]),
                    goals: None,
                    cards: None,
                    substitutions: None,
                    period: None,
                    period_times: None,
                    penalty_shootout: None,
                    penalty_shootout_score: None,
                },
                players: HashMap::new(),
            }),
            // A live-scored football result: full goal/card/sub detail.
            Score::Football(FootballScore {
                inner: scoring::Score {
                    score: HashMap::from([
                        ("side_red".to_string(), 2),
                        ("side_blue".to_string(), 1),
                    ]),
                    goals: Some(vec![
                        scoring::GoalEvent {
                            side_id: "side_red".into(),
                            scorer_player_id: Some("player_1".into()),
                            assist_player_id: Some("player_2".into()),
                            own_goal: false,
                            penalty: false,
                            minute: Some(23),
                            occurred_at: Some(ts("2024-05-01T20:23:00.000Z")),
                        },
                        scoring::GoalEvent {
                            side_id: "side_blue".into(),
                            scorer_player_id: None,
                            assist_player_id: None,
                            own_goal: true,
                            penalty: false,
                            minute: None,
                            occurred_at: None,
                        },
                    ]),
                    cards: Some(vec![scoring::CardEvent {
                        side_id: "side_blue".into(),
                        player_id: "player_3".into(),
                        color: scoring::CardColor::Yellow,
                        minute: Some(60),
                        occurred_at: Some(ts("2024-05-01T21:00:00.000Z")),
                    }]),
                    substitutions: Some(vec![scoring::SubstitutionEvent {
                        side_id: "side_red".into(),
                        player_in_id: "player_4".into(),
                        player_out_id: "player_1".into(),
                        minute: Some(75),
                        occurred_at: Some(ts("2024-05-01T21:15:00.000Z")),
                    }]),
                    period: Some(scoring::Period::FullTime),
                    period_times: Some(HashMap::from([
                        (scoring::Period::KickOff, ts("2024-05-01T20:00:00.000Z")),
                        (scoring::Period::FullTime, ts("2024-05-01T21:45:00.000Z")),
                    ])),
                    penalty_shootout: Some(vec![scoring::PenaltyShootoutKick {
                        side_id: "side_red".into(),
                        scored: true,
                    }]),
                    penalty_shootout_score: Some(HashMap::from([("side_red".to_string(), 1)])),
                },
                players: HashMap::new(),
            }),
        ];

        for score in scores {
            let original_json = score.to_json();
            let round_tripped =
                crate::mapping::score_from_record(&crate::mapping::score_to_record(&score));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }
}
