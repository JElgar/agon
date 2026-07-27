use poem_openapi::{Enum, Object, Union};

use crate::detailed_score::football::{FootballEvent, FootballEventKind};

/// Football live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Football`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union)]
#[oai(one_of, discriminator_name = "kind")]
pub enum FootballLiveEvent {
    Goal(FootballGoalEvent),
    Card(FootballCardEvent),
    Substitution(FootballSubstitutionEvent),
    Period(FootballPeriodEvent),
}

#[derive(Object)]
pub struct FootballGoalEvent {
    /// The side this goal counts for.
    pub side_id: String,
    /// None for an own goal with no recorded scorer.
    pub scorer_player_id: Option<String>,
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    pub minute: Option<u32>,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum FootballCardColor {
    Yellow,
    Red,
}

#[derive(Object)]
pub struct FootballCardEvent {
    pub side_id: String,
    pub player_id: String,
    pub color: FootballCardColor,
    pub minute: Option<u32>,
}

#[derive(Object)]
pub struct FootballSubstitutionEvent {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    pub minute: Option<u32>,
}

#[derive(Enum, Clone, PartialEq)]
#[oai(rename_all = "snake_case")]
pub enum FootballPeriod {
    /// Kickoff — the moment the match clock actually starts. Recorded once,
    /// when the scorer starts live scoring; distinct from `Match.starts_at`
    /// (the *scheduled* time), which may not match when the whistle actually
    /// blew.
    KickOff,
    HalfTime,
    /// Kickoff of the second half — no clock gap is assumed between this and
    /// `HalfTime`, so added time in the first half is preserved automatically
    /// (the second half's clock continues from wherever the first half's
    /// left off, not from a fixed 45').
    SecondHalfKickOff,
    FullTime,
    ExtraTimeHalfTime,
    ExtraTimeFullTime,
    PenaltiesComplete,
}

#[derive(Object)]
pub struct FootballPeriodEvent {
    pub period: FootballPeriod,
}

/// A side's running goal tally, derived from the event log (a convenience for
/// cheap reads — e.g. a feed card — that don't want to re-derive it from
/// `events` themselves).
#[derive(Object)]
pub struct FootballSideGoals {
    pub side_id: String,
    pub goals: u32,
}

/// The live-scoring state derived by folding a match's football event log.
/// `events` reuses `detailed_score::football::FootballEvent` verbatim — this
/// *is* that timeline, just built up incrementally.
///
/// The `*_at` fields are the wall-clock moments each phase boundary was
/// actually recorded (each period marker's own `occurred_at`), not derived
/// from `Match.starts_at` — a scorer may start the clock later than the
/// scheduled kickoff. Together they let every client — the scorer's device
/// and any other viewer alike — compute the same running match minute
/// without needing a clock of their own: `now - kickoff_at` while in the
/// first half, `now - second_half_kickoff_at` plus the first half's length
/// once the second half is under way, frozen at the relevant boundary during
/// half-time/full-time.
#[derive(Object)]
pub struct FootballLiveState {
    /// The most recent period marker seen, if any.
    pub period: Option<FootballPeriod>,
    pub kickoff_at: Option<chrono::DateTime<chrono::Utc>>,
    pub half_time_at: Option<chrono::DateTime<chrono::Utc>>,
    pub second_half_kickoff_at: Option<chrono::DateTime<chrono::Utc>>,
    pub full_time_at: Option<chrono::DateTime<chrono::Utc>>,
    pub score: Vec<FootballSideGoals>,
    pub events: Vec<FootballEvent>,
}

/// Folds an ordered, timestamped list of events into the derived live state.
/// Callers pass whatever the DAO currently has on record — a deleted event is
/// simply absent from that list, and an amended one shows up with its
/// corrected content, so this never needs to know a correction happened at
/// all. Each event is paired with its own `occurred_at` (not `recorded_at`)
/// so a period marker's timestamp reflects when the half actually
/// started/ended on the pitch, not when the server received it.
pub fn derive_state(
    events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)],
) -> FootballLiveState {
    let mut period = None;
    let mut kickoff_at = None;
    let mut half_time_at = None;
    let mut second_half_kickoff_at = None;
    let mut full_time_at = None;
    let mut score: Vec<FootballSideGoals> = Vec::new();
    let mut out = Vec::new();

    for (occurred_at, event) in events {
        match event {
            FootballLiveEvent::Goal(g) => {
                if let Some(s) = score.iter_mut().find(|s| s.side_id == g.side_id) {
                    s.goals += 1;
                } else {
                    score.push(FootballSideGoals {
                        side_id: g.side_id.clone(),
                        goals: 1,
                    });
                }
                let kind = if g.own_goal {
                    FootballEventKind::OwnGoal
                } else if g.penalty {
                    FootballEventKind::Penalty
                } else {
                    FootballEventKind::Goal
                };
                out.push(FootballEvent {
                    kind,
                    side_id: g.side_id.clone(),
                    minute: g.minute,
                    player_id: g.scorer_player_id.clone(),
                    assist_player_id: g.assist_player_id.clone(),
                    substituted_player_id: None,
                });
            }
            FootballLiveEvent::Card(c) => {
                out.push(FootballEvent {
                    kind: match c.color {
                        FootballCardColor::Yellow => FootballEventKind::YellowCard,
                        FootballCardColor::Red => FootballEventKind::RedCard,
                    },
                    side_id: c.side_id.clone(),
                    minute: c.minute,
                    player_id: Some(c.player_id.clone()),
                    assist_player_id: None,
                    substituted_player_id: None,
                });
            }
            FootballLiveEvent::Substitution(sub) => {
                out.push(FootballEvent {
                    kind: FootballEventKind::Substitution,
                    side_id: sub.side_id.clone(),
                    minute: sub.minute,
                    player_id: Some(sub.player_in_id.clone()),
                    assist_player_id: None,
                    substituted_player_id: Some(sub.player_out_id.clone()),
                });
            }
            FootballLiveEvent::Period(p) => {
                match p.period {
                    FootballPeriod::KickOff => kickoff_at = Some(*occurred_at),
                    FootballPeriod::HalfTime => half_time_at = Some(*occurred_at),
                    FootballPeriod::SecondHalfKickOff => second_half_kickoff_at = Some(*occurred_at),
                    FootballPeriod::FullTime => full_time_at = Some(*occurred_at),
                    // Extra time / penalties aren't clocked yet — only the
                    // marker itself is tracked, same as before.
                    FootballPeriod::ExtraTimeHalfTime
                    | FootballPeriod::ExtraTimeFullTime
                    | FootballPeriod::PenaltiesComplete => {}
                }
                period = Some(p.period.clone());
            }
        }
    }

    FootballLiveState {
        period,
        kickoff_at,
        half_time_at,
        second_half_kickoff_at,
        full_time_at,
        score,
        events: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    #[test]
    fn derives_score_and_events_from_goals_cards_and_subs() {
        let events = vec![
            (
                ts(41),
                FootballLiveEvent::Goal(FootballGoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: Some("diaz".into()),
                    own_goal: false,
                    penalty: false,
                    minute: Some(41),
                }),
            ),
            (
                ts(58),
                FootballLiveEvent::Card(FootballCardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(58),
                }),
            ),
            // An own goal counts for the side benefiting from it.
            (
                ts(63),
                FootballLiveEvent::Goal(FootballGoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: None,
                    assist_player_id: None,
                    own_goal: true,
                    penalty: false,
                    minute: Some(63),
                }),
            ),
            (
                ts(70),
                FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                    side_id: "oak_park".into(),
                    player_in_id: "moreno".into(),
                    player_out_id: "khan".into(),
                    minute: Some(70),
                }),
            ),
            (
                ts(94),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::FullTime,
                }),
            ),
        ];

        let state = derive_state(&events);

        assert_eq!(state.score.len(), 1);
        assert_eq!(state.score[0].side_id, "riverside");
        assert_eq!(state.score[0].goals, 2);
        assert_eq!(state.events.len(), 4);
        assert!(matches!(state.period, Some(FootballPeriod::FullTime)));
        assert_eq!(state.full_time_at, Some(ts(94)));
        assert!(matches!(state.events[2].kind, FootballEventKind::OwnGoal));
        assert!(matches!(
            state.events[3].kind,
            FootballEventKind::Substitution
        ));
    }

    #[test]
    fn derives_phase_timestamps_from_period_markers() {
        let events = vec![
            (
                ts(0),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::KickOff,
                }),
            ),
            (
                ts(46),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::HalfTime,
                }),
            ),
            (
                ts(60),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::SecondHalfKickOff,
                }),
            ),
        ];

        let state = derive_state(&events);

        assert_eq!(state.kickoff_at, Some(ts(0)));
        assert_eq!(state.half_time_at, Some(ts(46)));
        assert_eq!(state.second_half_kickoff_at, Some(ts(60)));
        assert_eq!(state.full_time_at, None);
        assert!(matches!(state.period, Some(FootballPeriod::SecondHalfKickOff)));
    }
}
