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
    /// Free-text minutes-into-the-match, only ever set by a manually logged
    /// historical result (no live clock to timestamp against — see
    /// `agon_ui`'s `NetballScoreFields`). A live-scored goal leaves this
    /// unset; `occurred_at` is its source of truth instead.
    pub minute: Option<u32>,
    /// When this actually happened, wall-clock — always overwritten by the
    /// server from the live event envelope's own `occurred_at` for anything
    /// recorded through `/live/events` (see `NetballScore::apply_event`),
    /// ignoring whatever a client sends here. `None` for a manually logged
    /// historical result, same as `minute` above.
    ///
    /// This — not the quarter-relative `minute` — is what the events list
    /// sorts by: `minute` resets every quarter, so sorting on it alone
    /// scrambles a match's later quarters in with its earlier ones. Second
    /// precision (not just minutes) also means two goals recorded a few
    /// seconds apart no longer collide on the same displayed time.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
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
    /// Same convention as `NetballGoalEvent::minute` — manual entry only.
    pub minute: Option<u32>,
    /// Same convention as `NetballGoalEvent::occurred_at` — the source of
    /// truth for a live-scored foul's time and sort order.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A netball match's quarters, plus an optional golden-goal-style decider if
/// still level after full time — same shape/role as `FootballPeriod`.
///
/// Each quarter after the first has its own explicit `*Start` marker rather
/// than the *End* marker before it doubling as the next quarter's start —
/// unlike football's, netball's breaks (and half-time) are a real interval
/// of unknown length, not a token gap, so the scorer has to tap "start" once
/// play actually resumes. `ExtraTimeStart` already worked this way (the
/// existing precedent this follows); `Start` itself doesn't need a separate
/// pre-match marker since there's no "break" before the match has begun.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[oai(rename_all = "snake_case")]
pub enum NetballPeriod {
    /// First centre pass — the moment the match clock actually starts.
    Start,
    QuarterOneEnd,
    /// The 2nd quarter actually starting, after the break following
    /// `QuarterOneEnd` — see the enum's doc comment.
    QuarterTwoStart,
    /// = half time.
    QuarterTwoEnd,
    /// The 3rd quarter (2nd half) actually starting, after half-time.
    QuarterThreeStart,
    QuarterThreeEnd,
    /// The 4th quarter actually starting, after the break following
    /// `QuarterThreeEnd`.
    QuarterFourStart,
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
            NetballPeriod::QuarterTwoStart => "quarter_two_start",
            NetballPeriod::QuarterTwoEnd => "quarter_two_end",
            NetballPeriod::QuarterThreeStart => "quarter_three_start",
            NetballPeriod::QuarterThreeEnd => "quarter_three_end",
            NetballPeriod::QuarterFourStart => "quarter_four_start",
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
            "quarter_two_start" => Ok(NetballPeriod::QuarterTwoStart),
            "quarter_two_end" => Ok(NetballPeriod::QuarterTwoEnd),
            "quarter_three_start" => Ok(NetballPeriod::QuarterThreeStart),
            "quarter_three_end" => Ok(NetballPeriod::QuarterThreeEnd),
            "quarter_four_start" => Ok(NetballPeriod::QuarterFourStart),
            "full_time" => Ok(NetballPeriod::FullTime),
            "extra_time_start" => Ok(NetballPeriod::ExtraTimeStart),
            "extra_time_end" => Ok(NetballPeriod::ExtraTimeEnd),
            other => Err(format!("unknown netball period: {other}")),
        }
    }
}
