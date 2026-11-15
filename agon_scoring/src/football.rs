//! Football: the score/event types and the live-event fold.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub score: HashMap<String, u32>,
    pub goals: Option<Vec<GoalEvent>>,
    pub cards: Option<Vec<CardEvent>>,
    pub substitutions: Option<Vec<SubstitutionEvent>>,
    pub period: Option<Period>,
    pub period_times: Option<HashMap<Period, DateTime<Utc>>>,
    pub penalty_shootout: Option<Vec<PenaltyShootoutKick>>,
    pub penalty_shootout_score: Option<HashMap<String, u32>>,
}

impl Score {
    pub fn new() -> Self {
        Self {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            cards: Some(Vec::new()),
            substitutions: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            penalty_shootout: Some(Vec::new()),
            penalty_shootout_score: Some(HashMap::new()),
        }
    }
}

impl Default for Score {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalEvent {
    pub side_id: String,
    pub scorer_player_id: Option<String>,
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    pub minute: Option<u32>,
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardColor {
    Yellow,
    Red,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardEvent {
    pub side_id: String,
    pub player_id: String,
    pub color: CardColor,
    pub minute: Option<u32>,
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubstitutionEvent {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    pub minute: Option<u32>,
    pub occurred_at: Option<DateTime<Utc>>,
}

/// One kick in a penalty shootout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PenaltyShootoutKick {
    pub side_id: String,
    pub scored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    KickOff,
    HalfTime,
    SecondHalfKickOff,
    FullTime,
    ExtraTimeKickOff,
    ExtraTimeHalfTime,
    ExtraTimeSecondHalfKickOff,
    ExtraTimeFullTime,
    PenaltiesComplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeriodEvent {
    pub period: Period,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LiveEvent {
    Goal(GoalEvent),
    Card(CardEvent),
    Substitution(SubstitutionEvent),
    Period(PeriodEvent),
    PenaltyShootoutKick(PenaltyShootoutKick),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Format {
    pub half_length_minutes: u32,
    pub num_halves: u32,
    pub extra_time: bool,
    pub extra_time_half_length_minutes: Option<u32>,
    pub penalties: bool,
}

/// Folds one live event into `score`. `occurred_at` is the recording
/// device's clock — the authoritative time for the event, regardless of
/// whatever (if anything) the event payload itself carries.
///
/// Every optional field is populated (`Some`, even if empty) once live
/// scoring starts — only a bare manual entry leaves them `None` — so this
/// always writes into an existing `Some`.
pub fn apply_event(
    score: &mut Score,
    occurred_at: DateTime<Utc>,
    event: &LiveEvent,
    _format: Option<&Format>,
) {
    match event {
        LiveEvent::Goal(g) => {
            *score.score.entry(g.side_id.clone()).or_insert(0) += 1;
            let mut g = g.clone();
            g.occurred_at = Some(occurred_at);
            score.goals.get_or_insert_with(Vec::new).push(g);
        }
        LiveEvent::Card(c) => {
            let mut c = c.clone();
            c.occurred_at = Some(occurred_at);
            score.cards.get_or_insert_with(Vec::new).push(c);
        }
        LiveEvent::Substitution(sub) => {
            let mut sub = sub.clone();
            sub.occurred_at = Some(occurred_at);
            score.substitutions.get_or_insert_with(Vec::new).push(sub);
        }
        LiveEvent::Period(p) => {
            score
                .period_times
                .get_or_insert_with(HashMap::new)
                .insert(p.period, occurred_at);
            score.period = Some(p.period);
        }
        LiveEvent::PenaltyShootoutKick(k) => {
            if k.scored {
                *score
                    .penalty_shootout_score
                    .get_or_insert_with(HashMap::new)
                    .entry(k.side_id.clone())
                    .or_insert(0) += 1;
            }
            score
                .penalty_shootout
                .get_or_insert_with(Vec::new)
                .push(k.clone());
        }
    }
}

/// Folds a whole ordered event log into a score. Appends use
/// [`apply_event`] directly, so both paths share one fold.
pub fn from_events(events: &[(DateTime<Utc>, LiveEvent)], format: Option<&Format>) -> Score {
    let mut score = Score::new();
    for (occurred_at, event) in events {
        apply_event(&mut score, *occurred_at, event, format);
    }
    score
}

/// The winning side, when the score decides one. `side_ids` is the match's
/// own two sides, since a side with nothing scored may be absent from the
/// score. Level on goals falls back to the penalty-shootout tally.
pub fn winner(score: &Score, side_ids: &[String]) -> Option<String> {
    two_side_winner(side_ids, |sid| *score.score.get(sid).unwrap_or(&0) as i64).or_else(|| {
        two_side_winner(side_ids, |sid| {
            score
                .penalty_shootout_score
                .as_ref()
                .and_then(|pss| pss.get(sid))
                .copied()
                .unwrap_or(0) as i64
        })
    })
}

fn two_side_winner(side_ids: &[String], score_for: impl Fn(&str) -> i64) -> Option<String> {
    let [a_id, b_id] = side_ids else { return None };
    let a = score_for(a_id);
    let b = score_for(b_id);
    if a == b {
        None
    } else if a > b {
        Some(a_id.clone())
    } else {
        Some(b_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    #[test]
    fn derives_score_goals_cards_and_substitutions() {
        let events = vec![
            (
                ts(41),
                LiveEvent::Goal(GoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: Some("diaz".into()),
                    own_goal: false,
                    penalty: false,
                    minute: Some(41),
                    occurred_at: None,
                }),
            ),
            (
                ts(58),
                LiveEvent::Card(CardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: CardColor::Yellow,
                    minute: Some(58),
                    occurred_at: None,
                }),
            ),
            (
                ts(63),
                LiveEvent::Goal(GoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: None,
                    assist_player_id: None,
                    own_goal: true,
                    penalty: false,
                    minute: Some(63),
                    occurred_at: None,
                }),
            ),
            (
                ts(70),
                LiveEvent::Substitution(SubstitutionEvent {
                    side_id: "oak_park".into(),
                    player_in_id: "moreno".into(),
                    player_out_id: "khan".into(),
                    minute: Some(70),
                    occurred_at: None,
                }),
            ),
            (
                ts(94),
                LiveEvent::Period(PeriodEvent {
                    period: Period::FullTime,
                }),
            ),
        ];

        let score = from_events(&events, None);

        assert_eq!(score.score.len(), 1);
        assert_eq!(score.score.get("riverside"), Some(&2));
        assert_eq!(score.goals.as_ref().unwrap().len(), 2);
        assert_eq!(score.cards.as_ref().unwrap().len(), 1);
        assert_eq!(score.substitutions.as_ref().unwrap().len(), 1);
        assert!(matches!(score.period, Some(Period::FullTime)));
        assert_eq!(
            score.period_times.as_ref().unwrap().get(&Period::FullTime),
            Some(&ts(94))
        );
        assert!(score.goals.as_ref().unwrap()[1].own_goal);
        assert_eq!(
            score.substitutions.as_ref().unwrap()[0].player_out_id,
            "khan"
        );
    }

    #[test]
    fn penalty_shootout_kicks_tally_scored_only() {
        let events = vec![
            (
                ts(160),
                LiveEvent::PenaltyShootoutKick(PenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(161),
                LiveEvent::PenaltyShootoutKick(PenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: false,
                }),
            ),
            (
                ts(162),
                LiveEvent::PenaltyShootoutKick(PenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
        ];

        let score = from_events(&events, None);
        let shootout_score = score.penalty_shootout_score.as_ref().unwrap();

        assert_eq!(score.penalty_shootout.as_ref().unwrap().len(), 3);
        assert_eq!(shootout_score.get("riverside"), Some(&2));
        assert!(score.score.is_empty());
    }

    #[test]
    fn winner_falls_back_to_penalty_shootout_when_level_on_goals() {
        let side_ids = vec!["riverside".to_string(), "oak_park".to_string()];
        let events = vec![
            (
                ts(0),
                LiveEvent::Goal(GoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: None,
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: None,
                    occurred_at: None,
                }),
            ),
            (
                ts(1),
                LiveEvent::Goal(GoalEvent {
                    side_id: "oak_park".into(),
                    scorer_player_id: None,
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: None,
                    occurred_at: None,
                }),
            ),
            (
                ts(160),
                LiveEvent::PenaltyShootoutKick(PenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(161),
                LiveEvent::PenaltyShootoutKick(PenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: false,
                }),
            ),
        ];

        let score = from_events(&events, None);

        assert_eq!(winner(&score, &side_ids), Some("riverside".to_string()));
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            (
                ts(0),
                LiveEvent::Period(PeriodEvent {
                    period: Period::KickOff,
                }),
            ),
            (
                ts(12),
                LiveEvent::Goal(GoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: Some(12),
                    occurred_at: None,
                }),
            ),
        ];

        let full = from_events(&events, None);

        let mut incremental = Score::new();
        for (occurred_at, event) in &events {
            apply_event(&mut incremental, *occurred_at, event, None);
        }

        assert_eq!(incremental, full);
    }
}
