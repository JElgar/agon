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

/// Folds an ordered, timestamped list of events into the match's full
/// detail — cheap enough to do on every read; football's event volume
/// (goals, cards, subs) never approaches cricket's ball-by-ball scale, so
/// there's no bounding or incremental-caching concern here the way there is
/// for cricket. Each event is paired with its own `occurred_at` (not
/// `recorded_at`) so a period marker's timestamp reflects when the half
/// actually started/ended on the pitch, not when the server received it.
pub fn derive_detail(
    events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)],
) -> FootballDetail {
    let mut period = None;
    let mut kickoff_at = None;
    let mut half_time_at = None;
    let mut second_half_kickoff_at = None;
    let mut full_time_at = None;
    let mut score: Vec<FootballSideGoals> = Vec::new();
    let mut goals = Vec::new();
    let mut cards = Vec::new();
    let mut substitutions = Vec::new();

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
                goals.push(g.clone());
            }
            FootballLiveEvent::Card(c) => {
                cards.push(c.clone());
            }
            FootballLiveEvent::Substitution(sub) => {
                substitutions.push(sub.clone());
            }
            FootballLiveEvent::Period(p) => {
                match p.period {
                    FootballPeriod::KickOff => kickoff_at = Some(*occurred_at),
                    FootballPeriod::HalfTime => half_time_at = Some(*occurred_at),
                    FootballPeriod::SecondHalfKickOff => {
                        second_half_kickoff_at = Some(*occurred_at)
                    }
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

    FootballDetail {
        score,
        goals,
        cards,
        substitutions,
        period,
        kickoff_at,
        half_time_at,
        second_half_kickoff_at,
        full_time_at,
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

        let detail = derive_detail(&events);

        assert_eq!(detail.score.len(), 1);
        assert_eq!(detail.score[0].side_id, "riverside");
        assert_eq!(detail.score[0].goals, 2);
        assert_eq!(detail.goals.len(), 2);
        assert_eq!(detail.cards.len(), 1);
        assert_eq!(detail.substitutions.len(), 1);
        assert!(matches!(detail.period, Some(FootballPeriod::FullTime)));
        assert_eq!(detail.full_time_at, Some(ts(94)));
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

        let detail = derive_detail(&events);

        assert_eq!(detail.kickoff_at, Some(ts(0)));
        assert_eq!(detail.half_time_at, Some(ts(46)));
        assert_eq!(detail.second_half_kickoff_at, Some(ts(60)));
        assert_eq!(detail.full_time_at, None);
        assert!(matches!(
            detail.period,
            Some(FootballPeriod::SecondHalfKickOff)
        ));
    }
}
