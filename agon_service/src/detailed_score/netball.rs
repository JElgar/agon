use poem_openapi::{Enum, Object};

/// GS/GA are the only positions that can legally shoot, but (same stance as
/// `match_format::CricketFormat`'s doc comment) that isn't enforced here —
/// purely descriptive, for stat display.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum NetballPosition {
    GoalShooter,
    GoalAttack,
    WingAttack,
    Centre,
    WingDefence,
    GoalDefence,
    GoalKeeper,
}

#[derive(Object, Clone)]
pub struct NetballGoalEvent {
    /// The side this goal counts for.
    pub side_id: String,
    pub scorer_player_id: Option<String>,
    pub scorer_position: Option<NetballPosition>,
    /// Fast5/Power-Play-style two-point zone. `false` for standard scoring,
    /// where every goal is worth one.
    pub two_points: bool,
    /// Minutes elapsed in the *current quarter*, not match-wide — netball's
    /// clock resets each quarter, unlike football's continuous minute.
    pub minute: Option<u32>,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum NetballFoulKind {
    Contact,
    Obstruction,
    Footwork,
    Offside,
    HeldBall,
    Other,
}

/// A non-scoring infringement — recorded for stats only, same role as
/// `FootballCardEvent`. Doesn't touch `score`.
#[derive(Object, Clone)]
pub struct NetballFoulEvent {
    /// The side penalised (the conceding side, not the side benefiting).
    pub side_id: String,
    /// The offending player, when tracked — a casual scorer may log fouls
    /// against the side only.
    pub player_id: Option<String>,
    /// Named `foul_kind`, not `kind` — `NetballLiveEvent`'s own
    /// discriminator (`Goal`/`Foul`/`Period`) is itself called `kind`, and
    /// nesting a *second*, differently-typed `kind` field inside the `Foul`
    /// variant would collide with it once flattened onto the same JSON
    /// object (the two would fight over one wire key). `NetballGoalEvent`
    /// has no such field, so it doesn't need the same care.
    pub foul_kind: NetballFoulKind,
    pub minute: Option<u32>,
}

/// A netball match's quarters, plus an optional golden-goal-style decider if
/// still level after full time — same shape/role as `FootballPeriod`.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[oai(rename_all = "snake_case")]
pub enum NetballPeriod {
    /// First centre pass — the moment the match clock actually starts.
    Start,
    QuarterOneEnd,
    /// = half time.
    QuarterTwoEnd,
    QuarterThreeEnd,
    FullTime,
    ExtraTimeStart,
    ExtraTimeEnd,
}

/// `ToString`/`FromStr` (via `Display`) mirroring the `#[oai(rename_all =
/// "snake_case")]` wire form above — needed so `NetballPeriod` can be used as
/// a `HashMap` key (`period_times`/`period_scores`), which poem-openapi
/// represents as a plain JSON object keyed by this string form. Same
/// convention as `FootballPeriod`.
impl std::fmt::Display for NetballPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NetballPeriod::Start => "start",
            NetballPeriod::QuarterOneEnd => "quarter_one_end",
            NetballPeriod::QuarterTwoEnd => "quarter_two_end",
            NetballPeriod::QuarterThreeEnd => "quarter_three_end",
            NetballPeriod::FullTime => "full_time",
            NetballPeriod::ExtraTimeStart => "extra_time_start",
            NetballPeriod::ExtraTimeEnd => "extra_time_end",
        })
    }
}

impl std::str::FromStr for NetballPeriod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "start" => Ok(NetballPeriod::Start),
            "quarter_one_end" => Ok(NetballPeriod::QuarterOneEnd),
            "quarter_two_end" => Ok(NetballPeriod::QuarterTwoEnd),
            "quarter_three_end" => Ok(NetballPeriod::QuarterThreeEnd),
            "full_time" => Ok(NetballPeriod::FullTime),
            "extra_time_start" => Ok(NetballPeriod::ExtraTimeStart),
            "extra_time_end" => Ok(NetballPeriod::ExtraTimeEnd),
            other => Err(format!("unknown netball period: {other}")),
        }
    }
}
