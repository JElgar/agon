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

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum FootballPeriod {
    HalfTime,
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
#[derive(Object)]
pub struct FootballLiveState {
    /// The most recent period marker seen, if any.
    pub period: Option<FootballPeriod>,
    pub score: Vec<FootballSideGoals>,
    pub events: Vec<FootballEvent>,
}

/// Folds an ordered list of events into the derived live state. Callers pass
/// whatever the DAO currently has on record — a deleted event is simply
/// absent from that list, and an amended one shows up with its corrected
/// content, so this never needs to know a correction happened at all.
pub fn derive_state(events: &[FootballLiveEvent]) -> FootballLiveState {
    let mut period = None;
    let mut score: Vec<FootballSideGoals> = Vec::new();
    let mut out = Vec::new();

    for event in events {
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
                period = Some(p.period.clone());
            }
        }
    }

    FootballLiveState {
        period,
        score,
        events: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_score_and_events_from_goals_cards_and_subs() {
        let events = vec![
            FootballLiveEvent::Goal(FootballGoalEvent {
                side_id: "riverside".into(),
                scorer_player_id: Some("alvarez".into()),
                assist_player_id: Some("diaz".into()),
                own_goal: false,
                penalty: false,
                minute: Some(41),
            }),
            FootballLiveEvent::Card(FootballCardEvent {
                side_id: "oak_park".into(),
                player_id: "khan".into(),
                color: FootballCardColor::Yellow,
                minute: Some(58),
            }),
            // An own goal counts for the side benefiting from it.
            FootballLiveEvent::Goal(FootballGoalEvent {
                side_id: "riverside".into(),
                scorer_player_id: None,
                assist_player_id: None,
                own_goal: true,
                penalty: false,
                minute: Some(63),
            }),
            FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                side_id: "oak_park".into(),
                player_in_id: "moreno".into(),
                player_out_id: "khan".into(),
                minute: Some(70),
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::FullTime,
            }),
        ];

        let state = derive_state(&events);

        assert_eq!(state.score.len(), 1);
        assert_eq!(state.score[0].side_id, "riverside");
        assert_eq!(state.score[0].goals, 2);
        assert_eq!(state.events.len(), 4);
        assert!(matches!(state.period, Some(FootballPeriod::FullTime)));
        assert!(matches!(state.events[2].kind, FootballEventKind::OwnGoal));
        assert!(matches!(
            state.events[3].kind,
            FootballEventKind::Substitution
        ));
    }
}
