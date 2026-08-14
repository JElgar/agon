use poem_openapi::{Enum, Object};

#[derive(Object, Clone)]
pub struct FootballGoalEvent {
    /// The side this goal counts for.
    pub side_id: String,
    /// None for an own goal with no recorded scorer.
    pub scorer_player_id: Option<String>,
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    /// Free-text minutes-into-the-match, only ever set by a manually logged
    /// historical result (no live clock to timestamp against — see
    /// `agon_ui`'s `FootballScoreFields`). A live-scored goal leaves this
    /// unset; `occurred_at` is its source of truth instead (see that field's
    /// doc comment).
    pub minute: Option<u32>,
    /// When this actually happened, wall-clock — always overwritten by the
    /// server from the live event envelope's own `occurred_at` for anything
    /// recorded through `/live/events` (see `FootballScore::apply_event`),
    /// ignoring whatever a client sends here. `None` for a manually logged
    /// historical result, same as `minute` above.
    ///
    /// This — not the free-text `minute` — is what a live-scored event's
    /// displayed minute is derived from, bracketed against
    /// `FootballScore.period_times` the same way the live clock itself is
    /// (see `agon_ui`'s `lib/liveScore.ts`): added time carries over between
    /// halves and extra time layers on top, all computed from period
    /// boundaries rather than trusted from what was typed in. Second
    /// precision also means two events recorded a few seconds apart no
    /// longer collide on the same displayed minute.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
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
    /// Same convention as `FootballGoalEvent::minute` — manual entry only.
    pub minute: Option<u32>,
    /// Same convention as `FootballGoalEvent::occurred_at`.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Object, Clone)]
pub struct FootballSubstitutionEvent {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    /// Same convention as `FootballGoalEvent::minute` — manual entry only.
    pub minute: Option<u32>,
    /// Same convention as `FootballGoalEvent::occurred_at`.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One kick in a penalty shootout.
#[derive(Object, Clone)]
pub struct FootballPenaltyShootoutKick {
    /// The side taking this kick.
    pub side_id: String,
    pub scored: bool,
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
    /// Kickoff of extra time's first half — same role as `KickOff`, one level
    /// up. Only recorded if the match is level at `FullTime` and the format
    /// plays extra time.
    ExtraTimeKickOff,
    ExtraTimeHalfTime,
    /// Kickoff of extra time's second half — same role as
    /// `SecondHalfKickOff`.
    ExtraTimeSecondHalfKickOff,
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
            FootballPeriod::ExtraTimeKickOff => "extra_time_kick_off",
            FootballPeriod::ExtraTimeHalfTime => "extra_time_half_time",
            FootballPeriod::ExtraTimeSecondHalfKickOff => "extra_time_second_half_kick_off",
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
            "extra_time_kick_off" => Ok(FootballPeriod::ExtraTimeKickOff),
            "extra_time_half_time" => Ok(FootballPeriod::ExtraTimeHalfTime),
            "extra_time_second_half_kick_off" => Ok(FootballPeriod::ExtraTimeSecondHalfKickOff),
            "extra_time_full_time" => Ok(FootballPeriod::ExtraTimeFullTime),
            "penalties_complete" => Ok(FootballPeriod::PenaltiesComplete),
            other => Err(format!("unknown football period: {other}")),
        }
    }
}
