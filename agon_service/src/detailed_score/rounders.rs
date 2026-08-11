use poem_openapi::{Enum, Object};

/// How many balls back `RoundersScore.recent_deliveries` keeps for a
/// "recent balls" read — same role as cricket's `RECENT_DELIVERIES_LIMIT`.
pub const RECENT_DELIVERIES_LIMIT: usize = 12;

/// The four posts a batter runs around, in order — reaching the fourth
/// (touching home/the batting square) completes the circuit.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum RoundersPost {
    First,
    Second,
    Third,
    Fourth,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum RoundersOutKind {
    /// A struck ball caught before it touches the ground.
    Caught,
    /// A fielder gets the ball to the post a runner is running to before
    /// they arrive.
    StumpedAtPost,
    /// Runner passes/overtakes a teammate on the track.
    OvertookRunner,
    /// Deliberately obstructs a fielder going for the ball or a post.
    Obstruction,
    /// Struck the ball but made no attempt to run.
    FailedToRun,
}

/// Doesn't carry which player was dismissed — always nested somewhere that
/// already identifies them (`RoundersRunnerMovement.player_id`,
/// `RoundersBattingEntry.player_id`), same convention as
/// `detailed_score::cricket::CricketDismissal`.
#[derive(Object, Clone)]
pub struct RoundersOut {
    pub kind: RoundersOutKind,
    pub fielder_player_id: Option<String>,
}

/// One runner's outcome on a single delivery. A delivery carries a *list*
/// of these — every runner already on a post can move independently of
/// whatever the new striker does, since rounders (unlike cricket's
/// striker/non-striker pair) can have several runners on the track from
/// different batters' earlier turns at once.
#[derive(Object, Clone)]
pub struct RoundersRunnerMovement {
    pub player_id: String,
    /// `None` = stayed on their previous post (or the striker didn't run).
    /// Whether reaching the fourth post here is a full or half rounder is
    /// not stored — it's derived while folding, from whether `player_id`
    /// is this delivery's `striker_player_id` (see
    /// `RoundersLiveEvent::apply_delivery`'s doc comment).
    pub post_reached: Option<RoundersPost>,
    pub out: Option<RoundersOut>,
}

/// A single delivery (ball) in an innings — the live-scoring "delivery"
/// event payload verbatim (see
/// `crate::live_score::rounders::RoundersLiveEvent::Delivery`), same
/// convention as `CricketDelivery`.
#[derive(Object, Clone)]
pub struct RoundersDelivery {
    pub bowler_player_id: String,
    /// The batter newly facing this ball. A batter's turn ends after one
    /// good ball (the last-remaining batter in the order gets more — see
    /// `crate::match_format::RoundersFormat::last_batter_bonus_balls`), then
    /// the next name in the order comes in; the order loops back around for
    /// anyone not yet out once everyone's had a turn, so the same
    /// `player_id` can appear here again later in the innings.
    pub striker_player_id: String,
    /// Bowled unplayably (too high/wide/low/short). Doesn't count toward
    /// the innings' good-ball cap and can't dismiss the striker at 1st
    /// post — any reward for it is still expressed as an ordinary
    /// `RoundersRunnerMovement`, not implied by this flag.
    pub no_ball: bool,
    /// Backstop missed/fumbled a ball the striker didn't hit, rather than
    /// any runner advancing off a struck ball. Purely categorical — the
    /// actual advancement (if any) is in `movements`. Kept as an explicit
    /// flag rather than inferred, because "backstop's fault vs. a clean
    /// take that just didn't get back in time" is the scorer's judgment
    /// call, not an arithmetic fact — same reason
    /// `CricketDeliveryWicket::kind` is stored explicitly rather than
    /// derived.
    pub byes: bool,
    /// Every runner (including the striker, if they ran) who moved, stayed,
    /// or got out on this ball.
    pub movements: Vec<RoundersRunnerMovement>,
}

/// What's known about who's where for the *next* delivery, folded
/// incrementally as events are recorded — the rounders analogue of
/// cricket's `NextBallContext`, but tracking a variable number of
/// concurrent runners instead of a fixed striker/non-striker pair (see
/// `RoundersDelivery`'s doc comment on why more than one runner can be on
/// the track at once).
#[derive(Object, Clone)]
pub struct RoundersNextBallContext {
    /// Every runner still out on the track (not yet home, not yet out),
    /// keyed by player id.
    pub runners_on_base: std::collections::HashMap<String, RoundersPost>,
    pub bowler_player_id: Option<String>,
    /// Balls this bowler has sent down in their current spell — resets to 1
    /// whenever the bowler changes. Format may cap this (see
    /// `crate::match_format::RoundersFormat::balls_per_bowling_spell`),
    /// same display role as cricket's over/ball counters.
    pub balls_in_spell: u32,
}

impl RoundersNextBallContext {
    /// The state before any delivery has been recorded in an innings.
    pub fn opening() -> Self {
        RoundersNextBallContext {
            runners_on_base: std::collections::HashMap::new(),
            bowler_player_id: None,
            balls_in_spell: 0,
        }
    }
}
