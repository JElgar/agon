use poem_openapi::{Enum, Object};

/// A match's full football detail — live or finished, the same shape either
/// way. Every goal, card, and substitution recorded, each in its own
/// well-typed list rather than flattened into one generic "event" shape — a
/// goal's scorer/assist/own-goal/penalty fields mean the same thing here as
/// when they were recorded, no interpreting a shared field differently per
/// event kind. The phase fields (`kickoff_at`, `half_time_at`, ...) are
/// historical facts, not "current state" — nothing here needs to go blank
/// once the match is over the way cricket's next-ball context does.
#[derive(Object)]
pub struct FootballDetail {
    pub score: Vec<FootballSideGoals>,
    pub goals: Vec<FootballGoalEvent>,
    pub cards: Vec<FootballCardEvent>,
    pub substitutions: Vec<FootballSubstitutionEvent>,
    /// The most recent period marker seen, if any.
    pub period: Option<FootballPeriod>,
    pub kickoff_at: Option<chrono::DateTime<chrono::Utc>>,
    pub half_time_at: Option<chrono::DateTime<chrono::Utc>>,
    pub second_half_kickoff_at: Option<chrono::DateTime<chrono::Utc>>,
    pub full_time_at: Option<chrono::DateTime<chrono::Utc>>,
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
