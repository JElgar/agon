use std::collections::HashMap;

use poem_openapi::{Enum, Object};

/// A match's full football detail — live or finished, the same shape either
/// way. Every goal, card, and substitution recorded, each in its own
/// well-typed list rather than flattened into one generic "event" shape — a
/// goal's scorer/assist/own-goal/penalty fields mean the same thing here as
/// when they were recorded, no interpreting a shared field differently per
/// event kind. `period_times` are historical facts, not "current state" —
/// nothing here needs to go blank once the match is over the way cricket's
/// next-ball context does.
#[derive(Object)]
pub struct FootballDetail {
    pub score: Vec<FootballSideGoals>,
    pub goals: Vec<FootballGoalEvent>,
    pub cards: Vec<FootballCardEvent>,
    pub substitutions: Vec<FootballSubstitutionEvent>,
    /// The most recent period marker seen, if any.
    pub period: Option<FootballPeriod>,
    /// When each period marker was recorded, keyed by kind — one entry per
    /// `FootballPeriod` variant seen so far (a marker recorded twice
    /// overwrites, it doesn't append; see `FootballPeriod`'s doc comment on
    /// why every current variant is a once-per-match boundary). A map here
    /// rather than a named field per marker (the original
    /// `kickoff_at`/`half_time_at`/`second_half_kickoff_at`/`full_time_at`
    /// shape) so a new kind of marker — extra time, a drinks break — doesn't
    /// need a new field each time.
    pub period_times: HashMap<FootballPeriod, chrono::DateTime<chrono::Utc>>,
}

/// A side's running goal tally, derived from the event log (a convenience for
/// cheap reads — e.g. a feed card — that don't want to re-derive it from
/// `goals` themselves).
#[derive(Object)]
pub struct FootballSideGoals {
    pub side_id: String,
    pub goals: u32,
}

#[derive(Object, Clone)]
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

#[derive(Object, Clone)]
pub struct FootballCardEvent {
    pub side_id: String,
    pub player_id: String,
    pub color: FootballCardColor,
    pub minute: Option<u32>,
}

#[derive(Object, Clone)]
pub struct FootballSubstitutionEvent {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    pub minute: Option<u32>,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// `ToString`/`FromStr` (via `Display`) mirroring the `#[oai(rename_all =
/// "snake_case")]` wire form above — needed so `FootballPeriod` can be used
/// as a `HashMap` key (`period_times`), which poem-openapi represents as a
/// plain JSON object keyed by this string form.
impl std::fmt::Display for FootballPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FootballPeriod::KickOff => "kick_off",
            FootballPeriod::HalfTime => "half_time",
            FootballPeriod::SecondHalfKickOff => "second_half_kick_off",
            FootballPeriod::FullTime => "full_time",
            FootballPeriod::ExtraTimeHalfTime => "extra_time_half_time",
            FootballPeriod::ExtraTimeFullTime => "extra_time_full_time",
            FootballPeriod::PenaltiesComplete => "penalties_complete",
        })
    }
}

impl std::str::FromStr for FootballPeriod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "kick_off" => Ok(FootballPeriod::KickOff),
            "half_time" => Ok(FootballPeriod::HalfTime),
            "second_half_kick_off" => Ok(FootballPeriod::SecondHalfKickOff),
            "full_time" => Ok(FootballPeriod::FullTime),
            "extra_time_half_time" => Ok(FootballPeriod::ExtraTimeHalfTime),
            "extra_time_full_time" => Ok(FootballPeriod::ExtraTimeFullTime),
            "penalties_complete" => Ok(FootballPeriod::PenaltiesComplete),
            other => Err(format!("unknown football period: {other}")),
        }
    }
}
