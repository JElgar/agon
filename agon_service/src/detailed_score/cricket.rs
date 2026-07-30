use poem_openapi::{Enum, Object};

/// A match's full cricket detail — live or finished, the same shape either
/// way. `innings` covers the whole match so far (batting/bowling cards,
/// extras, fall-of-wickets — bounded by roster size, not ball count, so it
/// stays small regardless of match length). `recent_deliveries` and
/// `next_ball_context` describe only the *current* innings, and are `None`
/// once there isn't one (between innings, or the match is over) — there's
/// nothing "next" to give context for.
///
/// A match scored ball-by-ball (live, in-app) builds this incrementally, one
/// delivery at a time (see `live_score::cricket::apply_delivery`), from the
/// event log — which is *not* part of this record. It's already stored one
/// item per ball in the match's live event log, which has no practical size
/// ceiling; duplicating it here would mean a second, much larger copy
/// embedded in this single record, risking DynamoDB's per-item size cap on a
/// long match. A finished match's complete ball-by-ball history reads that
/// log directly (paginated — `GET /matches/:id/live/events`) instead of
/// this record.
#[derive(Object)]
pub struct CricketDetail {
    pub innings: Vec<CricketInnings>,
    pub recent_deliveries: Option<Vec<CricketDelivery>>,
    pub next_ball_context: Option<NextBallContext>,
    /// True once the log's last innings has ended and no following one has
    /// started yet (i.e. between innings, or nothing's been recorded).
    pub awaiting_next_innings: bool,
}

/// How many balls back `CricketDetail.recent_deliveries` keeps for the "this
/// over"/recent-balls row — enough to show a bit more than the current over,
/// nowhere near a whole innings.
pub const RECENT_DELIVERIES_LIMIT: usize = 18;

/// What's known about who's at the crease/bowling for the *next* delivery,
/// folded incrementally as events are recorded. A `None` field means the
/// scorer needs to pick someone before the next ball can be recorded —
/// either a fresh innings (nothing bowled yet), a wicket just fell (the
/// dismissed batter's slot is open), or an over just completed (a new
/// bowler is required; the same bowler can't bowl consecutive overs).
///
/// Mirrors `agon_ui/src/lib/cricketScore.ts`'s `NextBallContext` field-for-
/// field and step-for-step — kept in sync by hand. The frontend runs the
/// identical fold offline, against deliveries a device has recorded locally
/// but not yet synced, which is exactly the case this incremental (one
/// delivery at a time) shape is built for: apply one step to the last-known
/// context, don't replay history.
#[derive(Object, Clone)]
pub struct NextBallContext {
    pub striker_player_id: Option<String>,
    pub non_striker_player_id: Option<String>,
    pub bowler_player_id: Option<String>,
    /// 0-based over index the next delivery belongs to.
    pub over: u32,
    /// 1-based ball number within that over.
    pub ball: u32,
    /// The bowler who just finished an over — excluded from the next-bowler
    /// picker. `None` unless a bowler pick is actually needed.
    pub previous_over_bowler_player_id: Option<String>,
    /// Runs conceded so far in the *current* over (resets to 0 at each over
    /// boundary) — what decides whether the over that just completed was a
    /// maiden, without needing to look back at the deliveries that made it
    /// up. Also just useful on its own for a live "this over: 4 runs" read.
    pub runs_conceded_this_over: u32,
}

impl NextBallContext {
    /// The state before any delivery has been recorded in an innings.
    pub fn opening() -> Self {
        NextBallContext {
            striker_player_id: None,
            non_striker_player_id: None,
            bowler_player_id: None,
            over: 0,
            ball: 1,
            previous_over_bowler_player_id: None,
            runs_conceded_this_over: 0,
        }
    }
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// (not yet complete) over — not a single float like `19.4`, which only
/// works by coincidence for a standard 6-ball over. A float can't safely
/// represent a ball count that doesn't fit in one decimal digit (a
/// hypothetical over of 10+ balls collides with itself: 10 balls and 1 ball
/// both read as `.10`/`.1`), and an exact count has no business being a
/// float in the first place.
#[derive(Object, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overs {
    /// Completed overs.
    pub overs: u32,
    /// Balls into the current over (0..balls_per_over).
    pub balls: u32,
}

/// One innings' full detail: totals, batting/bowling cards, extras, and
/// fall-of-wickets. Bounded by roster size (how many players batted/bowled),
/// not ball count, so it stays small regardless of how long the innings runs
/// — safe to keep in `CricketDetail` and update on every single delivery.
#[derive(Object)]
pub struct CricketInnings {
    /// The batting side for this innings (references MatchSide.id).
    pub batting_side_id: String,
    /// The bowling/fielding side for this innings.
    pub bowling_side_id: String,
    /// Total runs scored in the innings.
    pub runs: u32,
    /// Wickets lost (0-10).
    pub wickets: u32,
    /// Overs bowled, e.g. 19 overs + 4 balls into the 20th.
    pub overs: Overs,
    /// Whether the innings has been declared closed.
    pub declared: bool,
    pub batting: Vec<CricketBattingEntry>,
    pub bowling: Vec<CricketBowlingEntry>,
    pub extras: CricketExtras,
    pub fall_of_wickets: Vec<CricketFallOfWicket>,
}

impl CricketInnings {
    pub fn opening(batting_side_id: String, bowling_side_id: String) -> Self {
        CricketInnings {
            batting_side_id,
            bowling_side_id,
            runs: 0,
            wickets: 0,
            overs: Overs { overs: 0, balls: 0 },
            declared: false,
            batting: Vec::new(),
            bowling: Vec::new(),
            extras: CricketExtras {
                byes: 0,
                leg_byes: 0,
                wides: 0,
                no_balls: 0,
                penalty: 0,
            },
            fall_of_wickets: Vec::new(),
        }
    }
}

/// A single delivery (ball) in an innings. The atomic unit of in-app scoring.
/// Also the live-scoring "delivery" event payload verbatim (see
/// `crate::live_score::cricket::CricketLiveEvent::Delivery`) — a live match's
/// ball-by-ball log *is* a sequence of these, folded incrementally instead of
/// submitted whole.
#[derive(Object, Clone)]
pub struct CricketDelivery {
    /// Over number, 0-based.
    pub over: u32,
    /// Legal-ball number within the over (1-6). Extras may add deliveries that
    /// do not advance this count (wides, no-balls).
    pub ball: u32,
    pub bowler_player_id: String,
    /// Batter on strike for this delivery.
    pub striker_player_id: String,
    /// Batter at the non-striker's end.
    pub non_striker_player_id: String,
    /// Runs scored off the bat (excludes extras).
    pub runs_off_bat: u32,
    /// Extra (wide / no-ball / bye / leg-bye / penalty), if this was one.
    pub extra: Option<CricketDeliveryExtra>,
    /// Wicket that fell on this delivery, if any.
    pub wicket: Option<CricketDeliveryWicket>,
}

#[derive(Object, Clone)]
pub struct CricketDeliveryExtra {
    pub kind: CricketExtraKind,
    /// Extra runs awarded for this delivery.
    pub runs: u32,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum CricketExtraKind {
    Wide,
    NoBall,
    Bye,
    LegBye,
    Penalty,
}

#[derive(Object, Clone)]
pub struct CricketDeliveryWicket {
    pub kind: CricketDismissalKind,
    /// The batter dismissed (usually the striker, but run-outs can be either).
    pub dismissed_player_id: String,
    /// Bowler credited (none for run outs / retired).
    pub bowler_player_id: Option<String>,
    /// Fielder involved: catcher, stumper, run-out thrower.
    pub fielder_player_id: Option<String>,
}

#[derive(Object)]
pub struct CricketBattingEntry {
    pub player_id: String,
    pub runs: u32,
    pub balls_faced: u32,
    pub fours: u32,
    pub sixes: u32,
    /// How the batter was dismissed. None if not out.
    pub dismissal: Option<CricketDismissal>,
    /// Batting order position (1 = opener).
    pub batting_position: Option<u32>,
}

#[derive(Object)]
pub struct CricketDismissal {
    pub kind: CricketDismissalKind,
    /// Bowler credited with the wicket (none for run outs / retired).
    pub bowler_player_id: Option<String>,
    /// Fielder involved: catcher, stumper, run-out thrower.
    pub fielder_player_id: Option<String>,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum CricketDismissalKind {
    Bowled,
    Caught,
    LegBeforeWicket,
    RunOut,
    Stumped,
    HitWicket,
    /// Left the field (injury/illness) and did not return. Counts as a
    /// wicket, unlike `RetiredHurt`. Recorded via a live `Retire` event with
    /// `retired_out: true` (see `crate::live_score::cricket`).
    RetiredOut,
    /// Left the field (injury/illness) but may return — the same
    /// `player_id` simply reappears on a later delivery. Does not count as a
    /// wicket. Recorded via a live `Retire` event with `retired_out: false`.
    RetiredHurt,
}

#[derive(Object)]
pub struct CricketBowlingEntry {
    pub player_id: String,
    /// Overs bowled, e.g. 4 overs + 0 balls, or 3 overs + 2 balls.
    pub overs: Overs,
    pub maidens: u32,
    pub runs_conceded: u32,
    pub wickets: u32,
    pub wides: u32,
    pub no_balls: u32,
}

#[derive(Object)]
pub struct CricketExtras {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

#[derive(Object)]
pub struct CricketFallOfWicket {
    /// Wicket number (1 = first wicket to fall).
    pub wicket: u32,
    /// Team score when this wicket fell.
    pub runs: u32,
    /// Batter dismissed.
    pub player_id: String,
    /// Overs completed when the wicket fell, if recorded.
    pub overs: Option<Overs>,
}

/// Cricket scoring rules encoded by `live_score::cricket::apply_delivery`:
/// - A delivery is *legal* (counts toward overs and balls faced) unless it is a
///   wide or no-ball.
/// - `runs_off_bat` is credited to the striker and the team total.
/// - Wides and no-balls are charged to the bowler; byes, leg-byes and penalties
///   are added to the team total but NOT charged to the bowler.
/// - A bowler is credited with a wicket only for bowled, caught, LBW, stumped,
///   or hit-wicket; run-outs and retirements are not.
/// - A maiden is an over in which the bowler concedes no runs (byes/leg-byes do
///   not count against the bowler, so they do not break a maiden).
pub(crate) fn dismissal_credited_to_bowler(kind: &CricketDismissalKind) -> bool {
    matches!(
        kind,
        CricketDismissalKind::Bowled
            | CricketDismissalKind::Caught
            | CricketDismissalKind::LegBeforeWicket
            | CricketDismissalKind::Stumped
            | CricketDismissalKind::HitWicket
    )
}

/// True if the delivery counts as a legal ball (advances the over). Wides and
/// no-balls are illegal (re-bowled as an extra delivery) under the standard
/// rules, but a match format can configure either one to just count as one of
/// the over's legal balls instead (see `CricketFormat::wide_is_extra_ball` /
/// `no_ball_is_extra_ball`).
pub(crate) fn is_legal_delivery(
    delivery: &CricketDelivery,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> bool {
    match delivery.extra.as_ref().map(|e| &e.kind) {
        Some(CricketExtraKind::Wide) => !wide_is_extra_ball,
        Some(CricketExtraKind::NoBall) => !no_ball_is_extra_ball,
        _ => true,
    }
}

/// Runs charged to the bowler for a delivery: runs off the bat plus wides and
/// no-balls. Byes, leg-byes and penalties are not the bowler's responsibility.
pub(crate) fn runs_charged_to_bowler(delivery: &CricketDelivery) -> u32 {
    let extra = delivery
        .extra
        .as_ref()
        .filter(|e| matches!(e.kind, CricketExtraKind::Wide | CricketExtraKind::NoBall))
        .map(|e| e.runs)
        .unwrap_or(0);
    delivery.runs_off_bat + extra
}

/// Converts a count of legal balls into whole overs + balls, e.g. 13 balls
/// -> 2 overs + 1 ball, given how many legal deliveries make an over (6 for
/// almost everything, 5 for The Hundred).
pub(crate) fn balls_to_overs(balls: u32, balls_per_over: u32) -> Overs {
    Overs {
        overs: balls / balls_per_over,
        balls: balls % balls_per_over,
    }
}
