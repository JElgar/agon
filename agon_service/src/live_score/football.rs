use std::collections::HashMap;

use poem_openapi::{Object, Union};

use crate::detailed_score::football::{
    FootballCardEvent, FootballDetail, FootballGoalEvent, FootballPeriod, FootballSideGoals,
    FootballSubstitutionEvent,
};

/// Football live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Football`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum FootballLiveEvent {
    /// Reuses `detailed_score::football::FootballGoalEvent` verbatim — see
    /// `CricketLiveEvent::Delivery`'s doc comment for why.
    Goal(FootballGoalEvent),
    Card(FootballCardEvent),
    Substitution(FootballSubstitutionEvent),
    Period(FootballPeriodEvent),
}

#[derive(Object, Clone)]
pub struct FootballPeriodEvent {
    pub period: FootballPeriod,
}

impl FootballDetail {
    /// Folds the whole event log into a `FootballDetail` from scratch — the
    /// slow path, used to bootstrap a match's first detail or recover from a
    /// missing/unparseable persisted record. Just `apply_event` run once per
    /// event in order; the fast (single-event) and slow (whole-log) paths
    /// share the exact same fold, so they can't disagree — same pattern as
    /// `CricketDetail::from_events`.
    pub fn from_events(events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)]) -> Self {
        let mut detail = FootballDetail {
            score: Vec::new(),
            goals: Vec::new(),
            cards: Vec::new(),
            substitutions: Vec::new(),
            period: None,
            period_times: HashMap::new(),
        };
        for (occurred_at, event) in events {
            detail.apply_event(*occurred_at, event);
        }
        detail
    }

    /// Folds one new event into this detail in place — the fast path, run on
    /// every append. `occurred_at` (not `recorded_at`) is threaded through
    /// separately from `event` so a period marker's timestamp reflects when
    /// the half actually started/ended on the pitch, not when the server
    /// received it.
    pub fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &FootballLiveEvent,
    ) {
        match event {
            FootballLiveEvent::Goal(g) => {
                if let Some(s) = self.score.iter_mut().find(|s| s.side_id == g.side_id) {
                    s.goals += 1;
                } else {
                    self.score.push(FootballSideGoals {
                        side_id: g.side_id.clone(),
                        goals: 1,
                    });
                }
                self.goals.push(g.clone());
            }
            FootballLiveEvent::Card(c) => {
                self.cards.push(c.clone());
            }
            FootballLiveEvent::Substitution(sub) => {
                self.substitutions.push(sub.clone());
            }
            FootballLiveEvent::Period(p) => {
                self.period_times.insert(p.period, occurred_at);
                self.period = Some(p.period);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detailed_score::football::FootballCardColor;
    use chrono::{DateTime, TimeZone, Utc};

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    #[test]
    fn derives_score_goals_cards_and_substitutions() {
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

        let detail = FootballDetail::from_events(&events);

        assert_eq!(detail.score.len(), 1);
        assert_eq!(detail.score[0].side_id, "riverside");
        assert_eq!(detail.score[0].goals, 2);
        assert_eq!(detail.goals.len(), 2);
        assert_eq!(detail.cards.len(), 1);
        assert_eq!(detail.substitutions.len(), 1);
        assert!(matches!(detail.period, Some(FootballPeriod::FullTime)));
        assert_eq!(
            detail.period_times.get(&FootballPeriod::FullTime),
            Some(&ts(94))
        );
        assert!(detail.goals[1].own_goal);
        assert_eq!(detail.substitutions[0].player_out_id, "khan");
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

        let detail = FootballDetail::from_events(&events);

        assert_eq!(
            detail.period_times.get(&FootballPeriod::KickOff),
            Some(&ts(0))
        );
        assert_eq!(
            detail.period_times.get(&FootballPeriod::HalfTime),
            Some(&ts(46))
        );
        assert_eq!(
            detail.period_times.get(&FootballPeriod::SecondHalfKickOff),
            Some(&ts(60))
        );
        assert_eq!(detail.period_times.get(&FootballPeriod::FullTime), None);
        assert!(matches!(
            detail.period,
            Some(FootballPeriod::SecondHalfKickOff)
        ));
    }

    #[test]
    fn extra_time_and_penalties_markers_get_timestamps_too() {
        let events = vec![
            (
                ts(120),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeHalfTime,
                }),
            ),
            (
                ts(150),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeFullTime,
                }),
            ),
            (
                ts(160),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::PenaltiesComplete,
                }),
            ),
        ];

        let detail = FootballDetail::from_events(&events);

        assert_eq!(
            detail.period_times.get(&FootballPeriod::ExtraTimeHalfTime),
            Some(&ts(120))
        );
        assert_eq!(
            detail.period_times.get(&FootballPeriod::ExtraTimeFullTime),
            Some(&ts(150))
        );
        assert_eq!(
            detail.period_times.get(&FootballPeriod::PenaltiesComplete),
            Some(&ts(160))
        );
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            (
                ts(0),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::KickOff,
                }),
            ),
            (
                ts(12),
                FootballLiveEvent::Goal(FootballGoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: Some(12),
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
        ];

        let full = FootballDetail::from_events(&events);

        // Apply the same events one at a time, incrementally, and check the
        // final state matches the full fold exactly.
        let mut incremental = FootballDetail {
            score: Vec::new(),
            goals: Vec::new(),
            cards: Vec::new(),
            substitutions: Vec::new(),
            period: None,
            period_times: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event);
        }

        assert_eq!(incremental.score.len(), full.score.len());
        assert_eq!(incremental.goals.len(), full.goals.len());
        assert_eq!(incremental.cards.len(), full.cards.len());
        assert_eq!(incremental.period_times, full.period_times);
    }
}
