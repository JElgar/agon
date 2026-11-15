//! Football: the score/event types and the live-event fold.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A football match's result: the goal tally, plus optional richer detail.
/// Every optional field is populated (`Some`, even if empty) once live
/// scoring starts — only a bare manual entry leaves them `None`.
///
/// `agon_service` flattens this into its own `FootballScore` wrapper
/// (which adds `players`), so this type's own OpenAPI name must differ from
/// `"FootballScore"` — poem-openapi's `#[oai(flatten)]` silently drops the
/// flattened fields instead of erroring if the flattened type's registered
/// name collides with the containing struct's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballScoreFields"))]
pub struct Score {
    /// Goal tally, keyed by side id — exactly one entry per side, so a map
    /// rather than the `Vec<{side_id, ...}>` shape used where order or
    /// repeats matter (e.g. `goals`).
    pub score: HashMap<String, u32>,
    /// Every goal scored (normal + extra time; penalty-shootout kicks are
    /// tracked separately and never appear here), if there's a goal-by-goal
    /// breakdown to hand over.
    pub goals: Option<Vec<GoalEvent>>,
    pub cards: Option<Vec<CardEvent>>,
    pub substitutions: Option<Vec<SubstitutionEvent>>,
    /// The most recent period marker seen, if any. `None` for a result with
    /// no live detail behind it.
    pub period: Option<Period>,
    /// When each period marker was recorded, keyed by kind — one entry per
    /// [`Period`] variant seen so far (a marker recorded twice overwrites,
    /// it doesn't append). Historical facts, not "current state" — nothing
    /// here goes blank once the match is over.
    pub period_times: Option<HashMap<Period, DateTime<Utc>>>,
    /// Every penalty-shootout kick recorded, in order taken. Separate from
    /// `goals`/`score` — a shootout kick never counts as a match goal, only
    /// towards `penalty_shootout_score`.
    pub penalty_shootout: Option<Vec<PenaltyShootoutKick>>,
    /// Running shootout tally (kicks scored, not kicks taken) per side,
    /// derived from `penalty_shootout` the same way `score` is derived from
    /// `goals`. Keyed by side id, same reasoning as `score`.
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
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballGoalEvent"))]
pub struct GoalEvent {
    /// The side this goal counts for.
    pub side_id: String,
    /// `None` for an own goal with no recorded scorer.
    pub scorer_player_id: Option<String>,
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    /// Free-text minutes-into-the-match, only ever set by a manually logged
    /// historical result (no live clock to timestamp against). A
    /// live-scored goal leaves this unset; `occurred_at` is its source of
    /// truth instead.
    pub minute: Option<u32>,
    /// When this actually happened, wall-clock — always overwritten by the
    /// server from the live event envelope's own `occurred_at` for anything
    /// recorded through live scoring (see [`apply_event`]), ignoring
    /// whatever a client sends here. `None` for a manually logged
    /// historical result, same as `minute` above.
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Enum))]
#[cfg_attr(
    feature = "openapi",
    oai(rename = "FootballCardColor", rename_all = "snake_case")
)]
pub enum CardColor {
    Yellow,
    Red,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballCardEvent"))]
pub struct CardEvent {
    pub side_id: String,
    pub player_id: String,
    pub color: CardColor,
    /// Same convention as [`GoalEvent::minute`] — manual entry only.
    pub minute: Option<u32>,
    /// Same convention as [`GoalEvent::occurred_at`].
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballSubstitutionEvent"))]
pub struct SubstitutionEvent {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    /// Same convention as [`GoalEvent::minute`] — manual entry only.
    pub minute: Option<u32>,
    /// Same convention as [`GoalEvent::occurred_at`].
    pub occurred_at: Option<DateTime<Utc>>,
}

/// One kick in a penalty shootout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballPenaltyShootoutKick"))]
pub struct PenaltyShootoutKick {
    /// The side taking this kick.
    pub side_id: String,
    pub scored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Enum))]
#[cfg_attr(
    feature = "openapi",
    oai(rename = "FootballPeriod", rename_all = "snake_case")
)]
pub enum Period {
    /// Kickoff — the moment the match clock actually starts. Recorded once,
    /// when the scorer starts live scoring; distinct from the match's
    /// *scheduled* time, which may not match when the whistle actually blew.
    KickOff,
    HalfTime,
    /// Kickoff of the second half — no clock gap is assumed between this and
    /// `HalfTime`, so added time in the first half is preserved automatically.
    SecondHalfKickOff,
    FullTime,
    /// Kickoff of extra time's first half — same role as `KickOff`, one
    /// level up. Only recorded if the match is level at `FullTime` and the
    /// format plays extra time.
    ExtraTimeKickOff,
    ExtraTimeHalfTime,
    /// Kickoff of extra time's second half — same role as
    /// `SecondHalfKickOff`.
    ExtraTimeSecondHalfKickOff,
    ExtraTimeFullTime,
    PenaltiesComplete,
}

/// `ToString`/`FromStr` (via `Display`) mirroring the `snake_case` wire form
/// above — needed so [`Period`] can be used as a `HashMap` key
/// (`period_times`), which both `serde_json` and `poem-openapi` represent as
/// a plain JSON object keyed by this string form.
impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Period::KickOff => "kick_off",
            Period::HalfTime => "half_time",
            Period::SecondHalfKickOff => "second_half_kick_off",
            Period::FullTime => "full_time",
            Period::ExtraTimeKickOff => "extra_time_kick_off",
            Period::ExtraTimeHalfTime => "extra_time_half_time",
            Period::ExtraTimeSecondHalfKickOff => "extra_time_second_half_kick_off",
            Period::ExtraTimeFullTime => "extra_time_full_time",
            Period::PenaltiesComplete => "penalties_complete",
        })
    }
}

impl std::str::FromStr for Period {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "kick_off" => Ok(Period::KickOff),
            "half_time" => Ok(Period::HalfTime),
            "second_half_kick_off" => Ok(Period::SecondHalfKickOff),
            "full_time" => Ok(Period::FullTime),
            "extra_time_kick_off" => Ok(Period::ExtraTimeKickOff),
            "extra_time_half_time" => Ok(Period::ExtraTimeHalfTime),
            "extra_time_second_half_kick_off" => Ok(Period::ExtraTimeSecondHalfKickOff),
            "extra_time_full_time" => Ok(Period::ExtraTimeFullTime),
            "penalties_complete" => Ok(Period::PenaltiesComplete),
            other => Err(format!("unknown football period: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballPeriodEvent"))]
pub struct PeriodEvent {
    pub period: Period,
}

/// Football live-scoring events, discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event, not a variant
/// here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Union))]
#[cfg_attr(
    feature = "openapi",
    oai(rename = "FootballLiveEvent", one_of, discriminator_name = "kind")
)]
pub enum LiveEvent {
    Goal(GoalEvent),
    Card(CardEvent),
    Substitution(SubstitutionEvent),
    Period(PeriodEvent),
    PenaltyShootoutKick(PenaltyShootoutKick),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(poem_openapi::Object))]
#[cfg_attr(feature = "openapi", oai(rename = "FootballFormat"))]
pub struct Format {
    /// Minutes per half, e.g. 45.
    pub half_length_minutes: u32,
    /// Number of halves — normally 2.
    pub num_halves: u32,
    /// Whether extra time is played if the match is level after normal time.
    pub extra_time: bool,
    /// Minutes per extra-time half, if `extra_time` is set.
    pub extra_time_half_length_minutes: Option<u32>,
    /// Whether a penalty shootout follows if still level.
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
    fn derives_phase_timestamps_from_period_markers() {
        let events = vec![
            (
                ts(0),
                LiveEvent::Period(PeriodEvent {
                    period: Period::KickOff,
                }),
            ),
            (
                ts(46),
                LiveEvent::Period(PeriodEvent {
                    period: Period::HalfTime,
                }),
            ),
            (
                ts(60),
                LiveEvent::Period(PeriodEvent {
                    period: Period::SecondHalfKickOff,
                }),
            ),
        ];

        let score = from_events(&events, None);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(period_times.get(&Period::KickOff), Some(&ts(0)));
        assert_eq!(period_times.get(&Period::HalfTime), Some(&ts(46)));
        assert_eq!(period_times.get(&Period::SecondHalfKickOff), Some(&ts(60)));
        assert_eq!(period_times.get(&Period::FullTime), None);
        assert!(matches!(score.period, Some(Period::SecondHalfKickOff)));
    }

    #[test]
    fn extra_time_and_penalties_markers_get_timestamps_too() {
        let events = vec![
            (
                ts(90),
                LiveEvent::Period(PeriodEvent {
                    period: Period::ExtraTimeKickOff,
                }),
            ),
            (
                ts(105),
                LiveEvent::Period(PeriodEvent {
                    period: Period::ExtraTimeHalfTime,
                }),
            ),
            (
                ts(120),
                LiveEvent::Period(PeriodEvent {
                    period: Period::ExtraTimeSecondHalfKickOff,
                }),
            ),
            (
                ts(150),
                LiveEvent::Period(PeriodEvent {
                    period: Period::ExtraTimeFullTime,
                }),
            ),
            (
                ts(160),
                LiveEvent::Period(PeriodEvent {
                    period: Period::PenaltiesComplete,
                }),
            ),
        ];

        let score = from_events(&events, None);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(period_times.get(&Period::ExtraTimeKickOff), Some(&ts(90)));
        assert_eq!(period_times.get(&Period::ExtraTimeHalfTime), Some(&ts(105)));
        assert_eq!(
            period_times.get(&Period::ExtraTimeSecondHalfKickOff),
            Some(&ts(120))
        );
        assert_eq!(period_times.get(&Period::ExtraTimeFullTime), Some(&ts(150)));
        assert_eq!(period_times.get(&Period::PenaltiesComplete), Some(&ts(160)));
    }

    /// A live-scored goal/card/substitution is timestamped by the server's
    /// own clock, not whatever (if anything) a client sends — football's
    /// `minute` doesn't reset per period, so this isn't fixing a scrambling
    /// bug the way it would for a sport that resets per-period, but it
    /// still has to hold.
    #[test]
    fn goals_cards_and_subs_are_stamped_with_occurred_at_from_the_envelope() {
        let events = vec![
            (
                ts(0),
                LiveEvent::Goal(GoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: None,
                    occurred_at: None,
                }),
            ),
            (
                ts(1),
                LiveEvent::Card(CardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: CardColor::Yellow,
                    minute: None,
                    occurred_at: None,
                }),
            ),
        ];

        let score = from_events(&events, None);

        assert_eq!(score.goals.as_ref().unwrap()[0].occurred_at, Some(ts(0)));
        assert_eq!(score.cards.as_ref().unwrap()[0].occurred_at, Some(ts(1)));
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
