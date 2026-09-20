//! Cricket's whole API-side surface — see `crate::sports::netball`'s doc
//! comment for the pattern and its rationale. Cricket is the most involved
//! of the three: its event-log fold is genuinely stateful (an innings'
//! batting/bowling cards, extras, fall-of-wickets, and the next-ball
//! context all update together on every delivery), and its format
//! (`balls_per_over`, wide/no-ball rules) actually drives that fold's math,
//! not just client display.

use std::collections::HashMap;

use poem_openapi::{Enum, Object, Union};

use agon_core::sports::cricket::{
    CricketBattingEntryRecord, CricketBowlingEntryRecord, CricketDeliveryExtraRecord,
    CricketDeliveryRecord, CricketDeliveryWicketRecord, CricketDismissalKindRecord,
    CricketDismissalRecord, CricketExtraKindRecord, CricketExtrasRecord, CricketFallOfWicketRecord,
    CricketFormatRecord, CricketInningsEndEventRecord, CricketInningsStartEventRecord,
    CricketLiveEventRecord, CricketRetireEventRecord, CricketScoreInningsRecord,
    CricketScoreRecord, InningsEndReasonRecord, NextBallContextRecord, OversRecord,
};

use crate::live_score::NewLiveEventInput;
use crate::mapping::parse_ts_opt;

// ===========================================================================
// API types (formerly `match_format::CricketFormat`, `detailed_score::cricket`,
// `live_score::cricket`).
// ===========================================================================

#[derive(Object, Clone)]
pub struct CricketFormat {
    /// Overs per innings; `None` = unlimited (e.g. a declaration format).
    pub overs_per_innings: Option<u32>,
    /// Innings per side — 1 (limited-overs) or 2 (first-class/test-style).
    pub innings_per_side: u32,
    /// Legal deliveries per over — 6 for almost everything, 5 for The
    /// Hundred. Unlike the rest of this struct, this one *is* load-bearing
    /// on the server: it drives the overs-bowled math in this module's event
    /// fold, not just client display.
    pub balls_per_over: u32,
    /// Runs awarded for a no-ball's mandatory penalty (excludes any runs off
    /// the bat, which are recorded separately).
    pub no_ball_penalty_runs: u32,
    /// Runs awarded for a wide.
    pub wide_penalty_runs: u32,
    /// Whether a wide is re-bowled as an extra delivery (the standard rule —
    /// `true`) or simply counts as one of the over's legal balls alongside its
    /// penalty runs (a casual/social variant some sides play to keep overs
    /// moving).
    pub wide_is_extra_ball: bool,
    /// Whether a no-ball is re-bowled as an extra delivery (`true`, the
    /// standard rule) or counts as a legal ball, same trade-off as
    /// `wide_is_extra_ball`. Independent of `free_hit_after_no_ball`: Test
    /// cricket, for instance, plays no-balls as extra balls with no free hit
    /// at all (free hits are a modern limited-overs addition).
    pub no_ball_is_extra_ball: bool,
    /// Whether the delivery after a no-ball is a free hit.
    pub free_hit_after_no_ball: bool,
}

/// How many balls back `CricketScore.recent_deliveries` keeps for the "this
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
    fn opening() -> Self {
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

/// A single delivery (ball) in an innings. The atomic unit of in-app scoring.
/// Also the live-scoring "delivery" event payload verbatim (see
/// `CricketLiveEvent::Delivery`) — a live match's ball-by-ball log *is* a
/// sequence of these, folded incrementally instead of submitted whole.
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
    /// When this actually happened, wall-clock — always overwritten by the
    /// server from the live event envelope's own `occurred_at`, ignoring
    /// whatever a client sends here. `None` for a manually logged result.
    /// Cricket orders deliveries by over/ball rather than a clock, so unlike
    /// `NetballGoalEvent::occurred_at`/`FootballGoalEvent::occurred_at` this
    /// doesn't fix an ordering bug — it's stored for consistency with the
    /// other sports and for any future pace-of-play-style stat, not read by
    /// anything today.
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
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

#[derive(Object, Clone)]
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

#[derive(Object, Clone)]
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
    /// `retired_out: true`.
    RetiredOut,
    /// Left the field (injury/illness) but may return — the same
    /// `player_id` simply reappears on a later delivery. Does not count as a
    /// wicket. Recorded via a live `Retire` event with `retired_out: false`.
    RetiredHurt,
}

#[derive(Object, Clone)]
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

#[derive(Object, Clone, Default)]
pub struct CricketExtras {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

#[derive(Object, Clone)]
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

/// Cricket scoring rules encoded by `apply_delivery`:
/// - A delivery is *legal* (counts toward overs and balls faced) unless it is a
///   wide or no-ball.
/// - `runs_off_bat` is credited to the striker and the team total.
/// - Wides and no-balls are charged to the bowler; byes, leg-byes and penalties
///   are added to the team total but NOT charged to the bowler.
/// - A bowler is credited with a wicket only for bowled, caught, LBW, stumped,
///   or hit-wicket; run-outs and retirements are not.
/// - A maiden is an over in which the bowler concedes no runs (byes/leg-byes do
///   not count against the bowler, so they do not break a maiden).
fn dismissal_credited_to_bowler(kind: &CricketDismissalKind) -> bool {
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
fn is_legal_delivery(
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
fn runs_charged_to_bowler(delivery: &CricketDelivery) -> u32 {
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

/// One innings' totals, plus optional per-player detail. The totals
/// (`runs`/`wickets`/`overs`/`declared`) are always present — enough for a
/// completed-match tile — while `batting`/`bowling`/`fall_of_wickets`/
/// `extras` are `None` for a result with no per-player detail to hand over
/// (a manually-entered result with no card, or one that doesn't include
/// fall-of-wickets) and populated for a live-scored (or backfilled) match.
/// Never includes ball-by-ball deliveries — that stays in the live event log
/// for a match that wants it.
#[derive(Object)]
pub(crate) struct CricketScoreInnings {
    /// The batting side for this innings (references MatchSide.id).
    batting_side_id: String,
    /// The bowling/fielding side for this innings.
    bowling_side_id: String,
    /// Total runs scored in the innings.
    runs: u32,
    /// Wickets lost (0-10).
    wickets: u32,
    /// Overs bowled, e.g. 19 overs + 4 balls into the 20th.
    overs: Overs,
    /// Whether the innings was declared closed rather than bowled/timed out.
    declared: bool,
    batting: Option<Vec<CricketBattingEntry>>,
    bowling: Option<Vec<CricketBowlingEntry>>,
    fall_of_wickets: Option<Vec<CricketFallOfWicket>>,
    extras: Option<CricketExtras>,
}

impl CricketScoreInnings {
    /// The state before any delivery has been recorded in this innings —
    /// only ever constructed on a live-scoring path (`InningsStart`), so the
    /// optional card fields start populated (`Some`, empty) rather than
    /// `None`: there's a live innings behind this from the moment it exists,
    /// even before its first ball.
    fn opening(batting_side_id: String, bowling_side_id: String) -> Self {
        CricketScoreInnings {
            batting_side_id,
            bowling_side_id,
            runs: 0,
            wickets: 0,
            overs: Overs { overs: 0, balls: 0 },
            declared: false,
            batting: Some(Vec::new()),
            bowling: Some(Vec::new()),
            fall_of_wickets: Some(Vec::new()),
            extras: Some(CricketExtras::default()),
        }
    }
}

/// One entry per innings played, in the order they were played;
/// `recent_deliveries`/`next_ball_context`/`awaiting_next_innings`, plus
/// live name/avatar for every referenced player id.
#[derive(Object)]
pub(crate) struct CricketScore {
    innings: Vec<CricketScoreInnings>,
    /// The current/most recent innings' recent-ball window, for a "this
    /// over"/recent-balls read. `None` once there isn't a current innings
    /// (between innings, or the match is over) or for a result with no
    /// ball-by-ball detail behind it — there's nothing to show. Bounded
    /// (`RECENT_DELIVERIES_LIMIT`), not the whole innings — a finished
    /// match's complete ball-by-ball history reads the live event log
    /// directly (paginated — `GET /matches/:id/live/events`) instead of this
    /// field.
    recent_deliveries: Option<Vec<CricketDelivery>>,
    /// What's known about who's at the crease/bowling for the *next*
    /// delivery. `None` once there isn't a next delivery to give context for
    /// (between innings, match over, or no live detail at all).
    next_ball_context: Option<NextBallContext>,
    /// True once the log's last innings has ended and no following one has
    /// started yet (i.e. between innings, or nothing's been recorded).
    /// `None` for a result with no live log behind it.
    awaiting_next_innings: Option<bool>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — `next_ball_context`'s striker/non-striker/bowler, each
    /// innings' batting/bowling/fall-of-wicket entries, `recent_deliveries` —
    /// keyed by that same (match-scoped) player id. Look a player up here
    /// instead of scanning `Match.players`, which a feed/search card's
    /// trimmed match type doesn't carry at all. Resolved separately from
    /// everything else on this type: `score_from_record`/
    /// `CricketScore::from_events` (the DAO-only paths) always leave this
    /// empty, since neither has access to player records;
    /// `Api::hydrate_score_players` fills it afterward with one targeted
    /// `Dao::batch_get_match_players` lookup for exactly the ids this score
    /// references, not a full roster query. Not persisted (no counterpart on
    /// `ScoreRecord`).
    players: HashMap<String, crate::RosterPreviewPlayer>,
}

/// Cricket live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Cricket`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum CricketLiveEvent {
    /// One ball. Reuses `CricketDelivery` verbatim — a live innings'
    /// ball-by-ball log *is* that log, built up one delivery at a time
    /// instead of submitted whole at the end.
    Delivery(CricketDelivery),
    Retire(CricketRetireEvent),
    InningsStart(CricketInningsStartEvent),
    InningsEnd(CricketInningsEndEvent),
}

#[derive(Object, Clone)]
pub struct CricketRetireEvent {
    pub batter_player_id: String,
    /// True: counts as a wicket (fall-of-wickets, team tally) and the batter
    /// does not return. False: "retired hurt" — doesn't touch the wicket
    /// count, and the same `player_id` can simply reappear on a later
    /// delivery to resume batting (no separate "resume" event needed).
    pub retired_out: bool,
}

#[derive(Object, Clone)]
pub struct CricketInningsStartEvent {
    pub batting_side_id: String,
    pub bowling_side_id: String,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum InningsEndReason {
    AllOut,
    OversComplete,
    Declared,
    TargetReached,
}

#[derive(Object, Clone)]
pub struct CricketInningsEndEvent {
    pub reason: InningsEndReason,
}

// ===========================================================================
// Event-log fold (formerly `live_score::cricket`).
// ===========================================================================

fn batter<'a>(
    batting: &'a mut Vec<CricketBattingEntry>,
    player_id: &str,
) -> &'a mut CricketBattingEntry {
    if let Some(pos) = batting.iter().position(|b| b.player_id == player_id) {
        &mut batting[pos]
    } else {
        batting.push(CricketBattingEntry {
            player_id: player_id.to_string(),
            runs: 0,
            balls_faced: 0,
            fours: 0,
            sixes: 0,
            dismissal: None,
            batting_position: Some(batting.len() as u32 + 1),
        });
        batting.last_mut().unwrap()
    }
}

fn bowler<'a>(
    bowling: &'a mut Vec<CricketBowlingEntry>,
    player_id: &str,
) -> &'a mut CricketBowlingEntry {
    if let Some(pos) = bowling.iter().position(|b| b.player_id == player_id) {
        &mut bowling[pos]
    } else {
        bowling.push(CricketBowlingEntry {
            player_id: player_id.to_string(),
            overs: Overs { overs: 0, balls: 0 },
            maidens: 0,
            runs_conceded: 0,
            wickets: 0,
            wides: 0,
            no_balls: 0,
        });
        bowling.last_mut().unwrap()
    }
}

/// Folds one delivery into an innings already in progress — updates totals,
/// the batting/bowling cards, extras, and fall-of-wickets in place — and
/// returns the next-ball context that follows it. The whole incremental
/// step: called once per new delivery on the append path, and repeatedly
/// from `CricketScoreInnings::opening`/`NextBallContext::opening` to
/// bootstrap or recover a `CricketScore` from the full event log (see
/// `CricketScore::from_events`) — one implementation either way, so the two
/// paths can't disagree. The card fields (`batting`/`bowling`/`extras`/
/// `fall_of_wickets`) are always `Some` by the time this runs — `opening()`
/// populates them empty rather than `None` — but every access still goes
/// through `get_or_insert_with` defensively, since the same fields are
/// `None` for a bare manually-entered result and it costs nothing here to
/// not assume otherwise.
///
/// Strike rotates on an odd number of the ball's rotating runs (off the bat,
/// or byes/leg-byes — wides/no-balls don't rotate strike) and always at the
/// end of an over — including when one slot is already vacant from a wicket
/// on that same ball (e.g. dismissed on the over's final ball): the swap
/// still applies to whichever slot the survivor occupies, so the vacancy
/// lands in the correct slot for the next ball rather than always defaulting
/// to "striker". A maiden is credited to whoever bowled the over that just
/// completed, using `runs_conceded_this_over` — tracked alongside the
/// context rather than recomputed by scanning deliveries.
fn apply_delivery(
    innings: &mut CricketScoreInnings,
    context: &NextBallContext,
    d: &CricketDelivery,
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> NextBallContext {
    let legal = is_legal_delivery(d, wide_is_extra_ball, no_ball_is_extra_ball);
    let charged = runs_charged_to_bowler(d);
    let extra_runs = d.extra.as_ref().map(|e| e.runs).unwrap_or(0);

    innings.runs += d.runs_off_bat + extra_runs;
    if legal {
        let legal_balls = innings.overs.overs * balls_per_over + innings.overs.balls + 1;
        innings.overs = balls_to_overs(legal_balls, balls_per_over);
    }

    // Batting: striker is credited runs off the bat and faces legal balls
    // and no-balls (but not wides).
    {
        let b = batter(
            innings.batting.get_or_insert_with(Vec::new),
            &d.striker_player_id,
        );
        b.runs += d.runs_off_bat;
        let faced_ball = !matches!(
            d.extra.as_ref().map(|e| &e.kind),
            Some(CricketExtraKind::Wide)
        );
        if faced_ball {
            b.balls_faced += 1;
        }
        match d.runs_off_bat {
            4 => b.fours += 1,
            6 => b.sixes += 1,
            _ => {}
        }
    }

    // Bowling figures.
    {
        let bw = bowler(
            innings.bowling.get_or_insert_with(Vec::new),
            &d.bowler_player_id,
        );
        bw.runs_conceded += charged;
        if let Some(extra) = &d.extra {
            match extra.kind {
                CricketExtraKind::Wide => bw.wides += 1,
                CricketExtraKind::NoBall => bw.no_balls += 1,
                _ => {}
            }
        }
        if legal {
            let legal_balls = bw.overs.overs * balls_per_over + bw.overs.balls + 1;
            bw.overs = balls_to_overs(legal_balls, balls_per_over);
        }
    }

    // Extras breakdown.
    if let Some(extra) = &d.extra {
        let extras = innings.extras.get_or_insert_with(CricketExtras::default);
        match extra.kind {
            CricketExtraKind::Bye => extras.byes += extra.runs,
            CricketExtraKind::LegBye => extras.leg_byes += extra.runs,
            CricketExtraKind::Wide => extras.wides += extra.runs,
            CricketExtraKind::NoBall => extras.no_balls += extra.runs,
            CricketExtraKind::Penalty => extras.penalty += extra.runs,
        }
    }

    // Wicket.
    if let Some(wicket) = &d.wicket {
        innings.wickets += 1;
        innings
            .fall_of_wickets
            .get_or_insert_with(Vec::new)
            .push(CricketFallOfWicket {
                wicket: innings.wickets,
                runs: innings.runs,
                player_id: wicket.dismissed_player_id.clone(),
                overs: Some(innings.overs),
            });
        if dismissal_credited_to_bowler(&wicket.kind) {
            bowler(
                innings.bowling.get_or_insert_with(Vec::new),
                &d.bowler_player_id,
            )
            .wickets += 1;
        }
        let b = batter(
            innings.batting.get_or_insert_with(Vec::new),
            &wicket.dismissed_player_id,
        );
        b.dismissal = Some(CricketDismissal {
            kind: wicket.kind.clone(),
            bowler_player_id: wicket.bowler_player_id.clone(),
            fielder_player_id: wicket.fielder_player_id.clone(),
        });
    }

    // Next-ball context, folded from the previous one.
    let mut striker = Some(d.striker_player_id.clone());
    let mut non_striker = Some(d.non_striker_player_id.clone());
    let mut bowler_id = Some(d.bowler_player_id.clone());
    let mut previous_over_bowler: Option<String> = None;
    let mut legal_in_over = context.ball.saturating_sub(1);
    let mut over = context.over;
    let mut runs_conceded_this_over = context.runs_conceded_this_over + charged;

    if let Some(wicket) = &d.wicket {
        if striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            striker = None;
        } else if non_striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            non_striker = None;
        }
    }

    if legal {
        legal_in_over += 1;
        let rotating_runs = d.runs_off_bat
            + match d.extra.as_ref().map(|e| &e.kind) {
                Some(CricketExtraKind::Bye) | Some(CricketExtraKind::LegBye) => extra_runs,
                _ => 0,
            };
        if rotating_runs % 2 == 1 {
            std::mem::swap(&mut striker, &mut non_striker);
        }
        if legal_in_over == balls_per_over {
            std::mem::swap(&mut striker, &mut non_striker);
            if runs_conceded_this_over == 0
                && let Some(over_bowler_id) = &bowler_id
            {
                bowler(innings.bowling.get_or_insert_with(Vec::new), over_bowler_id).maidens += 1;
            }
            previous_over_bowler = bowler_id.take();
            over += 1;
            legal_in_over = 0;
            runs_conceded_this_over = 0;
        }
    }

    NextBallContext {
        striker_player_id: striker,
        non_striker_player_id: non_striker,
        bowler_player_id: bowler_id,
        over,
        ball: legal_in_over + 1,
        previous_over_bowler_player_id: previous_over_bowler,
        runs_conceded_this_over,
    }
}

impl CricketScore {
    /// Folds the whole event log into a `CricketScore` from scratch — the
    /// slow path, used to bootstrap a match's first score, recover from a
    /// missing/unparseable cache, or rebuild after undoing the last event.
    /// Just `apply_event` run once per event in order; the fast
    /// (single-event) and slow (whole-log) paths share the exact same fold,
    /// so they can't disagree.
    fn from_events(
        events: &[(chrono::DateTime<chrono::Utc>, CricketLiveEvent)],
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) -> Self {
        let mut score = CricketScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(
                *occurred_at,
                event,
                balls_per_over,
                wide_is_extra_ball,
                no_ball_is_extra_ball,
            );
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run
    /// on every append. `occurred_at` (not `recorded_at`) is threaded
    /// through separately from `event`, same reasoning as
    /// `crate::sports::football::FootballScore::apply_event` — only
    /// `Delivery` reads it (see `CricketDelivery::occurred_at`'s doc
    /// comment); every other variant ignores it, same as before this
    /// parameter existed.
    fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &CricketLiveEvent,
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) {
        match event {
            CricketLiveEvent::InningsStart(start) => {
                self.innings.push(CricketScoreInnings::opening(
                    start.batting_side_id.clone(),
                    start.bowling_side_id.clone(),
                ));
                self.recent_deliveries = Some(Vec::new());
                self.next_ball_context = Some(NextBallContext::opening());
                self.awaiting_next_innings = Some(false);
            }
            CricketLiveEvent::Delivery(d) => {
                let Some(current) = self.innings.last_mut() else {
                    // A delivery with no open innings is malformed input —
                    // there's nothing to fold it into.
                    return;
                };
                let context = self
                    .next_ball_context
                    .clone()
                    .unwrap_or_else(NextBallContext::opening);
                let next_context = apply_delivery(
                    current,
                    &context,
                    d,
                    balls_per_over,
                    wide_is_extra_ball,
                    no_ball_is_extra_ball,
                );
                self.next_ball_context = Some(next_context);

                // Stamp with the envelope's own `occurred_at` before storing
                // — see `CricketDelivery::occurred_at`'s doc comment. Stats
                // are already folded above off the un-stamped `d`, so this
                // only affects what gets stored/returned.
                let mut d = d.clone();
                d.occurred_at = Some(occurred_at);
                let deliveries = self.recent_deliveries.get_or_insert_with(Vec::new);
                deliveries.push(d);
                if deliveries.len() > RECENT_DELIVERIES_LIMIT {
                    deliveries.remove(0);
                }
            }
            CricketLiveEvent::Retire(r) => {
                let Some(current) = self.innings.last_mut() else {
                    return;
                };
                let Some(entry) = current
                    .batting
                    .get_or_insert_with(Vec::new)
                    .iter_mut()
                    .find(|b| b.player_id == r.batter_player_id)
                else {
                    // Retired without ever facing a ball — no batting-card
                    // row exists yet to annotate. Rare enough not to
                    // synthesize one.
                    return;
                };
                if entry.dismissal.is_some() {
                    // A later real dismissal (from a delivery) always takes
                    // precedence over an earlier retirement note — and by
                    // the time we're processing events in order, that's
                    // already reflected here.
                    return;
                }
                entry.dismissal = Some(CricketDismissal {
                    kind: if r.retired_out {
                        CricketDismissalKind::RetiredOut
                    } else {
                        CricketDismissalKind::RetiredHurt
                    },
                    bowler_player_id: None,
                    fielder_player_id: None,
                });
                if r.retired_out {
                    current.wickets += 1;
                    current.fall_of_wickets.get_or_insert_with(Vec::new).push(
                        CricketFallOfWicket {
                            wicket: current.wickets,
                            runs: current.runs,
                            player_id: r.batter_player_id.clone(),
                            overs: Some(current.overs),
                        },
                    );
                }
                if let Some(ctx) = &mut self.next_ball_context {
                    if ctx.striker_player_id.as_deref() == Some(r.batter_player_id.as_str()) {
                        ctx.striker_player_id = None;
                    } else if ctx.non_striker_player_id.as_deref()
                        == Some(r.batter_player_id.as_str())
                    {
                        ctx.non_striker_player_id = None;
                    }
                }
            }
            CricketLiveEvent::InningsEnd(end) => {
                if let Some(current) = self.innings.last_mut() {
                    current.declared = matches!(end.reason, InningsEndReason::Declared);
                }
                self.recent_deliveries = None;
                self.next_ball_context = None;
                self.awaiting_next_innings = Some(true);
            }
        }
    }
}

/// A cricket match's configured over length and extra-ball rules — standard
/// rules (a 6-ball over, wides/no-balls re-bowled as extras) unless the match
/// configured something else (e.g. The Hundred's 5-ball over). The only
/// pieces of match format the DAO-agnostic scoring math actually needs, so
/// callers thread these three through as plain arguments rather than the
/// whole format.
pub fn format_args(
    format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> (u32, bool, bool) {
    match format {
        Some(agon_core::dao::records::MatchFormatRecord::Cricket(f)) => (
            f.balls_per_over,
            f.wide_is_extra_ball,
            f.no_ball_is_extra_ball,
        ),
        _ => (6, true, true),
    }
}

/// Folds an ordered event log into a full `CricketScore` — see
/// `crate::sports::football::from_events`'s doc comment for the uniform
/// signature every sport's `from_events` shares. Cricket is the one sport
/// that actually needs `format` here, for `format_args`' over-length/
/// extra-ball rules.
pub fn from_events(
    events: &[(chrono::DateTime<chrono::Utc>, CricketLiveEvent)],
    format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> CricketScore {
    let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) = format_args(format);
    CricketScore::from_events(
        events,
        balls_per_over,
        wide_is_extra_ball,
        no_ball_is_extra_ball,
    )
}

/// The incremental fast path for a live-scoring append — see
/// `crate::sports::netball::apply_new_events`'s doc comment. Unlike
/// football/netball, cricket's fold genuinely needs `format` (via
/// `format_args`) to thread `balls_per_over`/the extra-ball rules through.
pub fn apply_new_events(
    score: &mut CricketScore,
    new_events: &[NewLiveEventInput],
    format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> Option<()> {
    let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) = format_args(format);
    for e in new_events {
        let crate::live_score::LiveEventInput::Cricket(event) = &e.event else {
            return None;
        };
        score.apply_event(
            e.occurred_at,
            event,
            balls_per_over,
            wide_is_extra_ball,
            no_ball_is_extra_ball,
        );
    }
    Some(())
}

// ===========================================================================
// Generic-`Score`-dispatch helpers (formerly per-sport arms scattered across
// `main.rs`).
// ===========================================================================

/// This score's side ids, for validating a submitted score references only
/// sides that actually exist on the match.
pub fn side_ids(score: &CricketScore) -> Vec<&str> {
    score
        .innings
        .iter()
        .flat_map(|i| [i.batting_side_id.as_str(), i.bowling_side_id.as_str()])
        .collect()
}

/// Re-point a `CricketScore`'s request-scoped client ids to the real side/
/// player ids assigned at creation — see `crate::sports::netball::resolve_ids`'s
/// doc comment. `None` if any referenced side or player is unknown.
pub fn resolve_ids(
    s: &CricketScore,
    side_ids: &HashMap<String, String>,
    player_ids: &HashMap<String, String>,
) -> Option<CricketScore> {
    let map = |client_id: &str| side_ids.get(client_id).cloned();
    let pmap = |client_id: &str| player_ids.get(client_id).cloned();
    let pmap_opt = |id: &Option<String>| -> Option<Option<String>> {
        match id {
            Some(id) => pmap(id).map(Some),
            None => Some(None),
        }
    };

    let mut innings = Vec::with_capacity(s.innings.len());
    for i in &s.innings {
        let batting = match &i.batting {
            Some(bs) => {
                let mut out = Vec::with_capacity(bs.len());
                for b in bs {
                    let dismissal = match &b.dismissal {
                        Some(d) => Some(CricketDismissal {
                            kind: d.kind.clone(),
                            bowler_player_id: pmap_opt(&d.bowler_player_id)?,
                            fielder_player_id: pmap_opt(&d.fielder_player_id)?,
                        }),
                        None => None,
                    };
                    out.push(CricketBattingEntry {
                        player_id: pmap(&b.player_id)?,
                        runs: b.runs,
                        balls_faced: b.balls_faced,
                        fours: b.fours,
                        sixes: b.sixes,
                        dismissal,
                        batting_position: b.batting_position,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let bowling = match &i.bowling {
            Some(bs) => {
                let mut out = Vec::with_capacity(bs.len());
                for b in bs {
                    out.push(CricketBowlingEntry {
                        player_id: pmap(&b.player_id)?,
                        overs: b.overs,
                        maidens: b.maidens,
                        runs_conceded: b.runs_conceded,
                        wickets: b.wickets,
                        wides: b.wides,
                        no_balls: b.no_balls,
                    });
                }
                Some(out)
            }
            None => None,
        };
        let fall_of_wickets = match &i.fall_of_wickets {
            Some(fs) => {
                let mut out = Vec::with_capacity(fs.len());
                for f in fs {
                    out.push(CricketFallOfWicket {
                        wicket: f.wicket,
                        runs: f.runs,
                        player_id: pmap(&f.player_id)?,
                        overs: f.overs,
                    });
                }
                Some(out)
            }
            None => None,
        };
        innings.push(CricketScoreInnings {
            batting_side_id: map(&i.batting_side_id)?,
            bowling_side_id: map(&i.bowling_side_id)?,
            runs: i.runs,
            wickets: i.wickets,
            overs: i.overs,
            declared: i.declared,
            batting,
            bowling,
            fall_of_wickets,
            extras: i.extras.clone(),
        });
    }
    let recent_deliveries = match &s.recent_deliveries {
        Some(ds) => {
            let mut out = Vec::with_capacity(ds.len());
            for d in ds {
                out.push(CricketDelivery {
                    over: d.over,
                    ball: d.ball,
                    bowler_player_id: pmap(&d.bowler_player_id)?,
                    striker_player_id: pmap(&d.striker_player_id)?,
                    non_striker_player_id: pmap(&d.non_striker_player_id)?,
                    runs_off_bat: d.runs_off_bat,
                    extra: d.extra.clone(),
                    wicket: match &d.wicket {
                        Some(w) => Some(CricketDeliveryWicket {
                            kind: w.kind.clone(),
                            dismissed_player_id: pmap(&w.dismissed_player_id)?,
                            bowler_player_id: pmap_opt(&w.bowler_player_id)?,
                            fielder_player_id: pmap_opt(&w.fielder_player_id)?,
                        }),
                        None => None,
                    },
                    occurred_at: d.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let next_ball_context = match &s.next_ball_context {
        Some(ctx) => Some(NextBallContext {
            striker_player_id: pmap_opt(&ctx.striker_player_id)?,
            non_striker_player_id: pmap_opt(&ctx.non_striker_player_id)?,
            bowler_player_id: pmap_opt(&ctx.bowler_player_id)?,
            over: ctx.over,
            ball: ctx.ball,
            previous_over_bowler_player_id: pmap_opt(&ctx.previous_over_bowler_player_id)?,
            runs_conceded_this_over: ctx.runs_conceded_this_over,
        }),
        None => None,
    };
    Some(CricketScore {
        innings,
        recent_deliveries,
        next_ball_context,
        awaiting_next_innings: s.awaiting_next_innings,
        players: HashMap::new(),
    })
}

/// Set this score's resolved-players map (see `Api::hydrate_score_players`).
pub fn set_players(
    score: &mut CricketScore,
    resolved: HashMap<String, crate::RosterPreviewPlayer>,
) {
    score.players = resolved;
}

/// Every player id referenced in a `CricketScore`: `next_ball_context`'s
/// striker/non-striker/bowler/previous-over-bowler, each innings' batting/
/// bowling/fall-of-wicket entries (plus a batting entry's dismissal bowler/
/// fielder), and `recent_deliveries` (plus each delivery's wicket). May
/// repeat the same id many times over (e.g. a bowler across several
/// deliveries) — deduped downstream by `Dao::batch_get_match_players`.
pub fn player_ids(score: &CricketScore) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(ctx) = &score.next_ball_context {
        ids.extend(ctx.striker_player_id.clone());
        ids.extend(ctx.non_striker_player_id.clone());
        ids.extend(ctx.bowler_player_id.clone());
        ids.extend(ctx.previous_over_bowler_player_id.clone());
    }
    for innings in &score.innings {
        for entry in innings.batting.iter().flatten() {
            ids.push(entry.player_id.clone());
            if let Some(d) = &entry.dismissal {
                ids.extend(d.bowler_player_id.clone());
                ids.extend(d.fielder_player_id.clone());
            }
        }
        for entry in innings.bowling.iter().flatten() {
            ids.push(entry.player_id.clone());
        }
        for fow in innings.fall_of_wickets.iter().flatten() {
            ids.push(fow.player_id.clone());
        }
    }
    for delivery in score.recent_deliveries.iter().flatten() {
        ids.push(delivery.bowler_player_id.clone());
        ids.push(delivery.striker_player_id.clone());
        ids.push(delivery.non_striker_player_id.clone());
        if let Some(w) = &delivery.wicket {
            ids.push(w.dismissed_player_id.clone());
            ids.extend(w.bowler_player_id.clone());
            ids.extend(w.fielder_player_id.clone());
        }
    }
    ids
}

/// Derives the winner (when decidable) from a live-scored cricket match's
/// persisted score — the summed match totals (two-innings formats add up
/// both).
pub fn winner(score: &CricketScore, side_ids: &[String]) -> Option<String> {
    let mut totals: HashMap<&str, u32> = HashMap::new();
    for i in &score.innings {
        *totals.entry(i.batting_side_id.as_str()).or_insert(0) += i.runs;
    }
    crate::two_side_winner(side_ids, |sid| *totals.get(sid).unwrap_or(&0) as i64)
}

// ===========================================================================
// API<->DAO mapping.
// ===========================================================================

pub fn score_to_record(s: &CricketScore) -> CricketScoreRecord {
    CricketScoreRecord {
        innings: s.innings.iter().map(score_innings_to_record).collect(),
        recent_deliveries: s
            .recent_deliveries
            .as_ref()
            .map(|ds| ds.iter().map(delivery_to_record).collect()),
        next_ball_context: s
            .next_ball_context
            .as_ref()
            .map(next_ball_context_to_record),
        awaiting_next_innings: s.awaiting_next_innings,
    }
}

pub fn score_from_record(rec: &CricketScoreRecord) -> CricketScore {
    CricketScore {
        innings: rec.innings.iter().map(score_innings_from_record).collect(),
        recent_deliveries: rec
            .recent_deliveries
            .as_ref()
            .map(|ds| ds.iter().map(delivery_from_record).collect()),
        next_ball_context: rec
            .next_ball_context
            .as_ref()
            .map(next_ball_context_from_record),
        awaiting_next_innings: rec.awaiting_next_innings,
        // Not stored — `Api::hydrate_score_players` fills this afterward (see
        // `CricketScore::players`' doc comment).
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &CricketFormat) -> CricketFormatRecord {
    CricketFormatRecord {
        overs_per_innings: f.overs_per_innings,
        innings_per_side: f.innings_per_side,
        balls_per_over: f.balls_per_over,
        no_ball_penalty_runs: f.no_ball_penalty_runs,
        wide_penalty_runs: f.wide_penalty_runs,
        wide_is_extra_ball: f.wide_is_extra_ball,
        no_ball_is_extra_ball: f.no_ball_is_extra_ball,
        free_hit_after_no_ball: f.free_hit_after_no_ball,
    }
}

pub fn format_from_record(f: &CricketFormatRecord) -> CricketFormat {
    CricketFormat {
        overs_per_innings: f.overs_per_innings,
        innings_per_side: f.innings_per_side,
        balls_per_over: f.balls_per_over,
        no_ball_penalty_runs: f.no_ball_penalty_runs,
        wide_penalty_runs: f.wide_penalty_runs,
        wide_is_extra_ball: f.wide_is_extra_ball,
        no_ball_is_extra_ball: f.no_ball_is_extra_ball,
        free_hit_after_no_ball: f.free_hit_after_no_ball,
    }
}

fn overs_to_record(overs: &Overs) -> OversRecord {
    OversRecord {
        overs: overs.overs,
        balls: overs.balls,
    }
}

fn overs_from_record(rec: &OversRecord) -> Overs {
    Overs {
        overs: rec.overs,
        balls: rec.balls,
    }
}

fn dismissal_to_record(d: &CricketDismissal) -> CricketDismissalRecord {
    CricketDismissalRecord {
        kind: dismissal_kind_to_record(&d.kind),
        bowler_player_id: d.bowler_player_id.clone(),
        fielder_player_id: d.fielder_player_id.clone(),
    }
}

fn dismissal_from_record(rec: &CricketDismissalRecord) -> CricketDismissal {
    CricketDismissal {
        kind: dismissal_kind_from_record(&rec.kind),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn batting_entry_to_record(b: &CricketBattingEntry) -> CricketBattingEntryRecord {
    CricketBattingEntryRecord {
        player_id: b.player_id.clone(),
        runs: b.runs,
        balls_faced: b.balls_faced,
        fours: b.fours,
        sixes: b.sixes,
        dismissal: b.dismissal.as_ref().map(dismissal_to_record),
        batting_position: b.batting_position,
    }
}

fn batting_entry_from_record(rec: &CricketBattingEntryRecord) -> CricketBattingEntry {
    CricketBattingEntry {
        player_id: rec.player_id.clone(),
        runs: rec.runs,
        balls_faced: rec.balls_faced,
        fours: rec.fours,
        sixes: rec.sixes,
        dismissal: rec.dismissal.as_ref().map(dismissal_from_record),
        batting_position: rec.batting_position,
    }
}

fn bowling_entry_to_record(b: &CricketBowlingEntry) -> CricketBowlingEntryRecord {
    CricketBowlingEntryRecord {
        player_id: b.player_id.clone(),
        overs: overs_to_record(&b.overs),
        maidens: b.maidens,
        runs_conceded: b.runs_conceded,
        wickets: b.wickets,
        wides: b.wides,
        no_balls: b.no_balls,
    }
}

fn bowling_entry_from_record(rec: &CricketBowlingEntryRecord) -> CricketBowlingEntry {
    CricketBowlingEntry {
        player_id: rec.player_id.clone(),
        overs: overs_from_record(&rec.overs),
        maidens: rec.maidens,
        runs_conceded: rec.runs_conceded,
        wickets: rec.wickets,
        wides: rec.wides,
        no_balls: rec.no_balls,
    }
}

fn extras_to_record(e: &CricketExtras) -> CricketExtrasRecord {
    CricketExtrasRecord {
        byes: e.byes,
        leg_byes: e.leg_byes,
        wides: e.wides,
        no_balls: e.no_balls,
        penalty: e.penalty,
    }
}

fn extras_from_record(rec: &CricketExtrasRecord) -> CricketExtras {
    CricketExtras {
        byes: rec.byes,
        leg_byes: rec.leg_byes,
        wides: rec.wides,
        no_balls: rec.no_balls,
        penalty: rec.penalty,
    }
}

fn fall_of_wicket_to_record(f: &CricketFallOfWicket) -> CricketFallOfWicketRecord {
    CricketFallOfWicketRecord {
        wicket: f.wicket,
        runs: f.runs,
        player_id: f.player_id.clone(),
        overs: f.overs.map(|o| overs_to_record(&o)),
    }
}

fn fall_of_wicket_from_record(rec: &CricketFallOfWicketRecord) -> CricketFallOfWicket {
    CricketFallOfWicket {
        wicket: rec.wicket,
        runs: rec.runs,
        player_id: rec.player_id.clone(),
        overs: rec.overs.map(|o| overs_from_record(&o)),
    }
}

fn score_innings_to_record(i: &CricketScoreInnings) -> CricketScoreInningsRecord {
    CricketScoreInningsRecord {
        batting_side_id: i.batting_side_id.clone(),
        bowling_side_id: i.bowling_side_id.clone(),
        runs: i.runs,
        wickets: i.wickets,
        overs: overs_to_record(&i.overs),
        declared: i.declared,
        batting: i
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(batting_entry_to_record).collect()),
        bowling: i
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(bowling_entry_to_record).collect()),
        fall_of_wickets: i
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(fall_of_wicket_to_record).collect()),
        extras: i.extras.as_ref().map(extras_to_record),
    }
}

fn score_innings_from_record(rec: &CricketScoreInningsRecord) -> CricketScoreInnings {
    CricketScoreInnings {
        batting_side_id: rec.batting_side_id.clone(),
        bowling_side_id: rec.bowling_side_id.clone(),
        runs: rec.runs,
        wickets: rec.wickets,
        overs: overs_from_record(&rec.overs),
        declared: rec.declared,
        batting: rec
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(batting_entry_from_record).collect()),
        bowling: rec
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(bowling_entry_from_record).collect()),
        fall_of_wickets: rec
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(fall_of_wicket_from_record).collect()),
        extras: rec.extras.as_ref().map(extras_from_record),
    }
}

fn delivery_to_record(d: &CricketDelivery) -> CricketDeliveryRecord {
    CricketDeliveryRecord {
        over: d.over,
        ball: d.ball,
        bowler_player_id: d.bowler_player_id.clone(),
        striker_player_id: d.striker_player_id.clone(),
        non_striker_player_id: d.non_striker_player_id.clone(),
        runs_off_bat: d.runs_off_bat,
        extra: d.extra.as_ref().map(delivery_extra_to_record),
        wicket: d.wicket.as_ref().map(delivery_wicket_to_record),
        occurred_at: d
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn delivery_from_record(rec: &CricketDeliveryRecord) -> CricketDelivery {
    CricketDelivery {
        over: rec.over,
        ball: rec.ball,
        bowler_player_id: rec.bowler_player_id.clone(),
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        runs_off_bat: rec.runs_off_bat,
        extra: rec.extra.as_ref().map(delivery_extra_from_record),
        wicket: rec.wicket.as_ref().map(delivery_wicket_from_record),
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn delivery_extra_to_record(e: &CricketDeliveryExtra) -> CricketDeliveryExtraRecord {
    CricketDeliveryExtraRecord {
        kind: extra_kind_to_record(&e.kind),
        runs: e.runs,
    }
}

fn delivery_extra_from_record(rec: &CricketDeliveryExtraRecord) -> CricketDeliveryExtra {
    CricketDeliveryExtra {
        kind: extra_kind_from_record(&rec.kind),
        runs: rec.runs,
    }
}

fn extra_kind_to_record(kind: &CricketExtraKind) -> CricketExtraKindRecord {
    match kind {
        CricketExtraKind::Wide => CricketExtraKindRecord::Wide,
        CricketExtraKind::NoBall => CricketExtraKindRecord::NoBall,
        CricketExtraKind::Bye => CricketExtraKindRecord::Bye,
        CricketExtraKind::LegBye => CricketExtraKindRecord::LegBye,
        CricketExtraKind::Penalty => CricketExtraKindRecord::Penalty,
    }
}

fn extra_kind_from_record(rec: &CricketExtraKindRecord) -> CricketExtraKind {
    match rec {
        CricketExtraKindRecord::Wide => CricketExtraKind::Wide,
        CricketExtraKindRecord::NoBall => CricketExtraKind::NoBall,
        CricketExtraKindRecord::Bye => CricketExtraKind::Bye,
        CricketExtraKindRecord::LegBye => CricketExtraKind::LegBye,
        CricketExtraKindRecord::Penalty => CricketExtraKind::Penalty,
    }
}

fn delivery_wicket_to_record(w: &CricketDeliveryWicket) -> CricketDeliveryWicketRecord {
    CricketDeliveryWicketRecord {
        kind: dismissal_kind_to_record(&w.kind),
        dismissed_player_id: w.dismissed_player_id.clone(),
        bowler_player_id: w.bowler_player_id.clone(),
        fielder_player_id: w.fielder_player_id.clone(),
    }
}

fn delivery_wicket_from_record(rec: &CricketDeliveryWicketRecord) -> CricketDeliveryWicket {
    CricketDeliveryWicket {
        kind: dismissal_kind_from_record(&rec.kind),
        dismissed_player_id: rec.dismissed_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn dismissal_kind_to_record(kind: &CricketDismissalKind) -> CricketDismissalKindRecord {
    match kind {
        CricketDismissalKind::Bowled => CricketDismissalKindRecord::Bowled,
        CricketDismissalKind::Caught => CricketDismissalKindRecord::Caught,
        CricketDismissalKind::LegBeforeWicket => CricketDismissalKindRecord::LegBeforeWicket,
        CricketDismissalKind::RunOut => CricketDismissalKindRecord::RunOut,
        CricketDismissalKind::Stumped => CricketDismissalKindRecord::Stumped,
        CricketDismissalKind::HitWicket => CricketDismissalKindRecord::HitWicket,
        CricketDismissalKind::RetiredOut => CricketDismissalKindRecord::RetiredOut,
        CricketDismissalKind::RetiredHurt => CricketDismissalKindRecord::RetiredHurt,
    }
}

fn dismissal_kind_from_record(rec: &CricketDismissalKindRecord) -> CricketDismissalKind {
    match rec {
        CricketDismissalKindRecord::Bowled => CricketDismissalKind::Bowled,
        CricketDismissalKindRecord::Caught => CricketDismissalKind::Caught,
        CricketDismissalKindRecord::LegBeforeWicket => CricketDismissalKind::LegBeforeWicket,
        CricketDismissalKindRecord::RunOut => CricketDismissalKind::RunOut,
        CricketDismissalKindRecord::Stumped => CricketDismissalKind::Stumped,
        CricketDismissalKindRecord::HitWicket => CricketDismissalKind::HitWicket,
        CricketDismissalKindRecord::RetiredOut => CricketDismissalKind::RetiredOut,
        CricketDismissalKindRecord::RetiredHurt => CricketDismissalKind::RetiredHurt,
    }
}

fn next_ball_context_to_record(ctx: &NextBallContext) -> NextBallContextRecord {
    NextBallContextRecord {
        striker_player_id: ctx.striker_player_id.clone(),
        non_striker_player_id: ctx.non_striker_player_id.clone(),
        bowler_player_id: ctx.bowler_player_id.clone(),
        over: ctx.over,
        ball: ctx.ball,
        previous_over_bowler_player_id: ctx.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: ctx.runs_conceded_this_over,
    }
}

fn next_ball_context_from_record(rec: &NextBallContextRecord) -> NextBallContext {
    NextBallContext {
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        over: rec.over,
        ball: rec.ball,
        previous_over_bowler_player_id: rec.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: rec.runs_conceded_this_over,
    }
}

fn innings_end_reason_to_record(reason: &InningsEndReason) -> InningsEndReasonRecord {
    match reason {
        InningsEndReason::AllOut => InningsEndReasonRecord::AllOut,
        InningsEndReason::OversComplete => InningsEndReasonRecord::OversComplete,
        InningsEndReason::Declared => InningsEndReasonRecord::Declared,
        InningsEndReason::TargetReached => InningsEndReasonRecord::TargetReached,
    }
}

fn innings_end_reason_from_record(rec: &InningsEndReasonRecord) -> InningsEndReason {
    match rec {
        InningsEndReasonRecord::AllOut => InningsEndReason::AllOut,
        InningsEndReasonRecord::OversComplete => InningsEndReason::OversComplete,
        InningsEndReasonRecord::Declared => InningsEndReason::Declared,
        InningsEndReasonRecord::TargetReached => InningsEndReason::TargetReached,
    }
}

pub fn live_event_to_record(event: &CricketLiveEvent) -> CricketLiveEventRecord {
    match event {
        CricketLiveEvent::Delivery(d) => CricketLiveEventRecord::Delivery(delivery_to_record(d)),
        CricketLiveEvent::Retire(r) => CricketLiveEventRecord::Retire(CricketRetireEventRecord {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEvent::InningsStart(s) => {
            CricketLiveEventRecord::InningsStart(CricketInningsStartEventRecord {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEvent::InningsEnd(e) => {
            CricketLiveEventRecord::InningsEnd(CricketInningsEndEventRecord {
                reason: innings_end_reason_to_record(&e.reason),
            })
        }
    }
}

pub fn live_event_from_record(rec: &CricketLiveEventRecord) -> CricketLiveEvent {
    match rec {
        CricketLiveEventRecord::Delivery(d) => CricketLiveEvent::Delivery(delivery_from_record(d)),
        CricketLiveEventRecord::Retire(r) => CricketLiveEvent::Retire(CricketRetireEvent {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEventRecord::InningsStart(s) => {
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEventRecord::InningsEnd(e) => {
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: innings_end_reason_from_record(&e.reason),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use poem_openapi::types::ToJSON;

    use super::*;
    use crate::live_score::LiveEventInput;
    use crate::mapping::{
        live_event_input_to_record, live_event_payload_from_record, match_format_from_record,
        match_format_to_record, parse_ts,
    };
    use crate::match_format::MatchFormat;

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
    }

    fn ball(bowler: &str, striker: &str, non_striker: &str, runs: u32) -> CricketDelivery {
        CricketDelivery {
            over: 0,
            ball: 1,
            bowler_player_id: bowler.into(),
            striker_player_id: striker.into(),
            non_striker_player_id: non_striker.into(),
            runs_off_bat: runs,
            extra: None,
            wicket: None,
            occurred_at: None,
        }
    }

    /// Most tests only care about the fold's result, not real timestamps —
    /// this pairs each event with a synthetic, strictly increasing one so
    /// `from_events` (which now always wants a timestamp — see
    /// `CricketScore::apply_event`) has something to thread through.
    fn score(events: &[CricketLiveEvent]) -> CricketScore {
        let timed: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(i, e)| (ts(i as i64), e.clone()))
            .collect();
        CricketScore::from_events(&timed, 6, true, true)
    }

    #[test]
    fn deliveries_are_stamped_with_occurred_at_from_the_envelope() {
        let events = vec![
            (
                ts(0),
                CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                    batting_side_id: "warriors".into(),
                    bowling_side_id: "mill_lane".into(),
                }),
            ),
            (
                ts(1),
                CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            ),
        ];

        let d = CricketScore::from_events(&events, 6, true, true);

        assert_eq!(
            d.recent_deliveries.as_ref().unwrap()[0].occurred_at,
            Some(ts(1))
        );
    }

    #[test]
    fn tracks_runs_wickets_overs_and_cards_per_innings() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 1);
        assert_eq!(d.innings[0].runs, 4);
        assert_eq!(d.innings[0].wickets, 1);
        assert_eq!(d.innings[0].overs, Overs { overs: 0, balls: 2 });
        assert_eq!(d.awaiting_next_innings, Some(false));

        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(sharma.runs, 4);
        assert!(matches!(
            sharma.dismissal.as_ref().map(|w| &w.kind),
            Some(CricketDismissalKind::Bowled)
        ));

        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.runs_conceded, 4);
        assert_eq!(patel.wickets, 1);
    }

    #[test]
    fn a_wicketless_over_is_a_maiden() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        for _ in 0..6 {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "sharma", "verma", 0,
            )));
        }
        let d = score(&events);
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.maidens, 1);
        assert_eq!(patel.overs, Overs { overs: 1, balls: 0 });
    }

    #[test]
    fn a_single_run_breaks_the_maiden() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        events.push(CricketLiveEvent::Delivery(ball(
            "patel", "sharma", "verma", 1,
        )));
        for _ in 0..5 {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "verma", "sharma", 0,
            )));
        }
        let d = score(&events);
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.maidens, 0);
    }

    #[test]
    fn recent_deliveries_is_capped_and_cleared_between_innings() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        for _ in 0..(RECENT_DELIVERIES_LIMIT + 5) {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "sharma", "verma", 1,
            )));
        }
        let d = score(&events);
        assert_eq!(
            d.recent_deliveries.as_ref().map(Vec::len),
            Some(RECENT_DELIVERIES_LIMIT)
        );
        assert_eq!(d.innings[0].runs, RECENT_DELIVERIES_LIMIT as u32 + 5);

        events.push(CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
            reason: InningsEndReason::Declared,
        }));
        let d = score(&events);
        assert!(d.recent_deliveries.is_none());
        assert!(d.next_ball_context.is_none());
        assert_eq!(d.awaiting_next_innings, Some(true));
    }

    #[test]
    fn next_ball_context_rotates_strike_on_odd_runs_and_over_boundary() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
        ];
        let ctx = score(&events).next_ball_context.unwrap();
        assert_eq!(ctx.striker_player_id.as_deref(), Some("verma"));
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("sharma"));
        assert_eq!(ctx.bowler_player_id.as_deref(), Some("patel"));
        assert_eq!(ctx.ball, 2);
        assert_eq!(ctx.over, 0);

        let mut all_events = events;
        for _ in 0..5 {
            all_events.push(CricketLiveEvent::Delivery(ball(
                "patel", "verma", "sharma", 0,
            )));
        }
        let ctx = score(&all_events).next_ball_context.unwrap();
        assert_eq!(
            ctx.over, 1,
            "over completes after 6 legal balls (with the standard 6-ball-over default)"
        );
        assert_eq!(ctx.ball, 1);
        assert_eq!(
            ctx.previous_over_bowler_player_id.as_deref(),
            Some("patel"),
            "the over's bowler can't be picked again for the next one"
        );
        assert!(
            ctx.bowler_player_id.is_none(),
            "a new bowler must be picked for the next over"
        );
    }

    #[test]
    fn a_wicket_vacates_the_dismissed_batters_slot() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];
        let ctx = score(&events).next_ball_context.unwrap();
        assert!(ctx.striker_player_id.is_none());
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("verma"));
    }

    #[test]
    fn retiring_hurt_lets_the_same_batter_resume_without_a_wicket() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: false,
            }),
            // Sharma comes back and faces another ball — same player_id, no
            // extra event needed for the "resume" side of things.
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 1);
        assert_eq!(
            d.innings[0].wickets, 0,
            "retiring hurt must not count as a wicket"
        );
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(
            sharma.runs, 5,
            "runs from before and after the retirement both count"
        );
        assert!(matches!(
            sharma.dismissal.as_ref().map(|w| &w.kind),
            Some(CricketDismissalKind::RetiredHurt)
        ));
        assert_eq!(
            d.awaiting_next_innings,
            Some(false),
            "the innings is still open (no InningsEnd), so we're not \"awaiting\" the next one"
        );
    }

    #[test]
    fn retiring_out_counts_as_a_wicket() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 2)),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: true,
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings[0].wickets, 1);
        assert_eq!(d.innings[0].fall_of_wickets.as_ref().unwrap().len(), 1);
        assert_eq!(
            d.innings[0].fall_of_wickets.as_ref().unwrap()[0].player_id,
            "sharma"
        );
    }

    #[test]
    fn innings_end_and_start_split_totals_by_inferred_index_not_a_stored_field() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: InningsEndReason::Declared,
            }),
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "mill_lane".into(),
                bowling_side_id: "warriors".into(),
            }),
            CricketLiveEvent::Delivery(ball("sharma", "cole", "adeyemi", 1)),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 2);
        assert_eq!(d.innings[0].batting_side_id, "warriors");
        assert_eq!(d.innings[0].runs, 4);
        assert!(d.innings[0].declared);
        assert_eq!(d.innings[1].batting_side_id, "mill_lane");
        assert_eq!(d.innings[1].runs, 1);
        assert_eq!(d.awaiting_next_innings, Some(false));
    }

    #[test]
    fn deleting_a_wrongly_placed_innings_end_re_flows_events_into_the_earlier_innings() {
        // A scorer wrongly taps "end innings" after one ball and deletes that
        // event (DELETE /matches/:id/live/events/:seq) rather than voiding
        // it — from a full refold's point of view that's indistinguishable
        // from it never having been recorded: the deleted event is just
        // absent from the list it's given.
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            // The wrongly-placed InningsEnd has already been deleted, so it
            // never appears here at all.
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 2)),
        ];

        let d = score(&events);
        assert_eq!(
            d.innings.len(),
            1,
            "the deleted end must not have split the log into two innings"
        );
        assert_eq!(d.innings[0].runs, 6);
        assert_eq!(d.awaiting_next_innings, Some(false));
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Caught,
                    dismissed_player_id: "verma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: Some("khan".into()),
                }),
                ..ball("patel", "verma", "sharma", 0)
            }),
        ];
        let events: Vec<_> = events
            .into_iter()
            .enumerate()
            .map(|(i, e)| (ts(i as i64), e))
            .collect();

        let full = CricketScore::from_events(&events, 6, true, true);

        // Apply the same events one at a time, incrementally, and check the
        // final state matches the full fold exactly.
        let mut incremental = CricketScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event, 6, true, true);
        }

        assert_eq!(incremental.innings.len(), full.innings.len());
        assert_eq!(incremental.innings[0].runs, full.innings[0].runs);
        assert_eq!(incremental.innings[0].wickets, full.innings[0].wickets);
        assert_eq!(
            incremental.innings[0].batting.as_ref().unwrap().len(),
            full.innings[0].batting.as_ref().unwrap().len()
        );
        assert_eq!(
            incremental.innings[0].bowling.as_ref().unwrap().len(),
            full.innings[0].bowling.as_ref().unwrap().len()
        );
    }

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    #[test]
    fn cricket_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            CricketLiveEvent::Delivery(CricketDelivery {
                over: 4,
                ball: 3,
                bowler_player_id: "patel".into(),
                striker_player_id: "sharma".into(),
                non_striker_player_id: "verma".into(),
                runs_off_bat: 0,
                extra: Some(CricketDeliveryExtra {
                    kind: CricketExtraKind::Wide,
                    runs: 1,
                }),
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Caught,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: Some("cole".into()),
                }),
                occurred_at: Some(parse_ts("2024-05-01T20:04:03.000Z")),
            }),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: false,
            }),
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: InningsEndReason::Declared,
            }),
        ];

        for event in events {
            let input = LiveEventInput::Cricket(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn cricket_format_round_trips_through_dao_mirror() {
        let format = MatchFormat::Cricket(CricketFormat {
            overs_per_innings: None,
            innings_per_side: 2,
            balls_per_over: 6,
            no_ball_penalty_runs: 2,
            wide_penalty_runs: 1,
            wide_is_extra_ball: true,
            no_ball_is_extra_ball: false,
            free_hit_after_no_ball: false,
        });
        let original_json = format.to_json();
        let round_tripped = match_format_from_record(&match_format_to_record(&format));
        assert_eq!(original_json, round_tripped.to_json());
    }

    #[test]
    fn cricket_score_round_trips_through_dao_mirror() {
        use crate::Score;

        let scores = vec![
            // A manually-entered cricket result: totals only, no per-player detail.
            Score::Cricket(CricketScore {
                innings: vec![
                    CricketScoreInnings {
                        batting_side_id: "warriors".into(),
                        bowling_side_id: "mill_lane".into(),
                        runs: 180,
                        wickets: 6,
                        overs: Overs {
                            overs: 20,
                            balls: 0,
                        },
                        declared: false,
                        batting: None,
                        bowling: None,
                        fall_of_wickets: None,
                        extras: None,
                    },
                    CricketScoreInnings {
                        batting_side_id: "mill_lane".into(),
                        bowling_side_id: "warriors".into(),
                        runs: 165,
                        wickets: 10,
                        overs: Overs {
                            overs: 19,
                            balls: 3,
                        },
                        declared: false,
                        batting: None,
                        bowling: None,
                        fall_of_wickets: None,
                        extras: None,
                    },
                ],
                recent_deliveries: None,
                next_ball_context: None,
                awaiting_next_innings: None,
                players: HashMap::new(),
            }),
            // A live-scored cricket result: full per-player detail.
            Score::Cricket(CricketScore {
                innings: vec![CricketScoreInnings {
                    batting_side_id: "warriors".into(),
                    bowling_side_id: "mill_lane".into(),
                    runs: 45,
                    wickets: 1,
                    overs: Overs { overs: 8, balls: 2 },
                    declared: false,
                    batting: Some(vec![CricketBattingEntry {
                        player_id: "player_1".into(),
                        runs: 30,
                        balls_faced: 20,
                        fours: 4,
                        sixes: 1,
                        dismissal: Some(CricketDismissal {
                            kind: CricketDismissalKind::Caught,
                            bowler_player_id: Some("player_5".into()),
                            fielder_player_id: Some("player_6".into()),
                        }),
                        batting_position: Some(1),
                    }]),
                    bowling: Some(vec![CricketBowlingEntry {
                        player_id: "player_5".into(),
                        overs: Overs { overs: 4, balls: 0 },
                        maidens: 1,
                        runs_conceded: 20,
                        wickets: 1,
                        wides: 0,
                        no_balls: 0,
                    }]),
                    fall_of_wickets: Some(vec![CricketFallOfWicket {
                        wicket: 1,
                        runs: 30,
                        player_id: "player_1".into(),
                        overs: Some(Overs { overs: 7, balls: 4 }),
                    }]),
                    extras: Some(CricketExtras {
                        byes: 1,
                        leg_byes: 2,
                        wides: 3,
                        no_balls: 0,
                        penalty: 0,
                    }),
                }],
                recent_deliveries: Some(vec![CricketDelivery {
                    over: 8,
                    ball: 3,
                    bowler_player_id: "player_5".into(),
                    striker_player_id: "player_1".into(),
                    non_striker_player_id: "player_2".into(),
                    runs_off_bat: 1,
                    extra: None,
                    wicket: None,
                    occurred_at: Some(parse_ts("2024-05-01T20:08:03.000Z")),
                }]),
                next_ball_context: Some(NextBallContext {
                    striker_player_id: Some("player_2".into()),
                    non_striker_player_id: Some("player_1".into()),
                    bowler_player_id: Some("player_5".into()),
                    over: 8,
                    ball: 4,
                    previous_over_bowler_player_id: None,
                    runs_conceded_this_over: 1,
                }),
                awaiting_next_innings: Some(false),
                players: HashMap::new(),
            }),
        ];

        for score in scores {
            let original_json = score.to_json();
            let round_tripped =
                crate::mapping::score_from_record(&crate::mapping::score_to_record(&score));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }
}
