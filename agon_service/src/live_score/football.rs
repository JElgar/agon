use std::collections::HashMap;

use poem_openapi::{Object, Union};

use crate::FootballScore;
use crate::detailed_score::football::{
    FootballCardEvent, FootballGoalEvent, FootballPenaltyShootoutKick, FootballPeriod,
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
    /// Reuses `detailed_score::football::FootballPenaltyShootoutKick`
    /// verbatim, same as `Goal`.
    PenaltyShootoutKick(FootballPenaltyShootoutKick),
}

#[derive(Object, Clone)]
pub struct FootballPeriodEvent {
    pub period: FootballPeriod,
}

impl FootballScore {
    /// Folds the whole event log into a `FootballScore` from scratch — the
    /// slow path, used to bootstrap a match's first score or recover from a
    /// missing/unparseable persisted record. Just `apply_event` run once per
    /// event in order; the fast (single-event) and slow (whole-log) paths
    /// share the exact same fold, so they can't disagree — same pattern as
    /// `CricketScore::from_events`.
    pub fn from_events(events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)]) -> Self {
        let mut score = FootballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            cards: Some(Vec::new()),
            substitutions: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            penalty_shootout: Some(Vec::new()),
            penalty_shootout_score: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(*occurred_at, event);
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run on
    /// every append. `occurred_at` (not `recorded_at`) is threaded through
    /// separately from `event` so a period marker's timestamp reflects when
    /// the half actually started/ended on the pitch, not when the server
    /// received it. Every optional field is populated (`Some`, even if
    /// empty) by the time this is called from a live-scoring path — only a
    /// bare manual entry ever leaves them `None` — so this always writes
    /// into an existing `Some`, never leaves a field `None` behind.
    pub fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &FootballLiveEvent,
    ) {
        match event {
            FootballLiveEvent::Goal(g) => {
                *self.score.entry(g.side_id.clone()).or_insert(0) += 1;
                self.goals.get_or_insert_with(Vec::new).push(g.clone());
            }
            FootballLiveEvent::Card(c) => {
                self.cards.get_or_insert_with(Vec::new).push(c.clone());
            }
            FootballLiveEvent::Substitution(sub) => {
                self.substitutions
                    .get_or_insert_with(Vec::new)
                    .push(sub.clone());
            }
            FootballLiveEvent::Period(p) => {
                self.period_times
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, occurred_at);
                self.period = Some(p.period);
            }
            FootballLiveEvent::PenaltyShootoutKick(k) => {
                if k.scored {
                    *self
                        .penalty_shootout_score
                        .get_or_insert_with(HashMap::new)
                        .entry(k.side_id.clone())
                        .or_insert(0) += 1;
                }
                self.penalty_shootout
                    .get_or_insert_with(Vec::new)
                    .push(k.clone());
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

        let score = FootballScore::from_events(&events);

        assert_eq!(score.score.len(), 1);
        assert_eq!(score.score.get("riverside"), Some(&2));
        assert_eq!(score.goals.as_ref().unwrap().len(), 2);
        assert_eq!(score.cards.as_ref().unwrap().len(), 1);
        assert_eq!(score.substitutions.as_ref().unwrap().len(), 1);
        assert!(matches!(score.period, Some(FootballPeriod::FullTime)));
        assert_eq!(
            score
                .period_times
                .as_ref()
                .unwrap()
                .get(&FootballPeriod::FullTime),
            Some(&ts(94))
        );
        assert!(score.goals.as_ref().unwrap()[1].own_goal);
        assert_eq!(
            score.substitutions.as_ref().unwrap()[0].player_out_id,
            "khan"
        );
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

        let score = FootballScore::from_events(&events);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(period_times.get(&FootballPeriod::KickOff), Some(&ts(0)));
        assert_eq!(period_times.get(&FootballPeriod::HalfTime), Some(&ts(46)));
        assert_eq!(
            period_times.get(&FootballPeriod::SecondHalfKickOff),
            Some(&ts(60))
        );
        assert_eq!(period_times.get(&FootballPeriod::FullTime), None);
        assert!(matches!(
            score.period,
            Some(FootballPeriod::SecondHalfKickOff)
        ));
    }

    #[test]
    fn extra_time_and_penalties_markers_get_timestamps_too() {
        let events = vec![
            (
                ts(90),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeKickOff,
                }),
            ),
            (
                ts(105),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeHalfTime,
                }),
            ),
            (
                ts(120),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeSecondHalfKickOff,
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

        let score = FootballScore::from_events(&events);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeKickOff),
            Some(&ts(90))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeHalfTime),
            Some(&ts(105))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeSecondHalfKickOff),
            Some(&ts(120))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeFullTime),
            Some(&ts(150))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::PenaltiesComplete),
            Some(&ts(160))
        );
    }

    #[test]
    fn penalty_shootout_kicks_tally_scored_only() {
        let events = vec![
            (
                ts(160),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(161),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: false,
                }),
            ),
            (
                ts(162),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(163),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: true,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);
        let shootout_score = score.penalty_shootout_score.as_ref().unwrap();

        assert_eq!(score.penalty_shootout.as_ref().unwrap().len(), 4);
        // Only sides with at least one scored kick get a tally entry — same
        // "absence means zero" convention as `score`.
        assert_eq!(shootout_score.len(), 2);
        assert_eq!(shootout_score.get("riverside"), Some(&2));
        assert_eq!(shootout_score.get("oak_park"), Some(&1));
        // A shootout kick never counts as a match goal.
        assert!(score.score.is_empty());
        assert!(score.goals.as_ref().unwrap().is_empty());
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

        let full = FootballScore::from_events(&events);

        // Apply the same events one at a time, incrementally, and check the
        // final state matches the full fold exactly.
        let mut incremental = FootballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            cards: Some(Vec::new()),
            substitutions: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            penalty_shootout: Some(Vec::new()),
            penalty_shootout_score: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event);
        }

        assert_eq!(incremental.score.len(), full.score.len());
        assert_eq!(
            incremental.goals.as_ref().unwrap().len(),
            full.goals.as_ref().unwrap().len()
        );
        assert_eq!(
            incremental.cards.as_ref().unwrap().len(),
            full.cards.as_ref().unwrap().len()
        );
        assert_eq!(incremental.period_times, full.period_times);
    }
}
