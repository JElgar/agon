//! Netball's whole API-side surface: its score/format/live-event/stats
//! types, the event-log fold (`NetballScore::from_events`/`apply_event`),
//! every per-score-shape helper the generic `Score` dispatchers call into
//! (`side_ids`/`resolve_ids`/`set_players`/`player_ids`/`winner`/
//! `apply_new_events`), and the API<->DAO mapping (see `crate::sports`'s doc
//! comment). Plain functions with the names `agon_sports!`'s generated
//! dispatchers expect every sport module to provide, rather than a shared
//! trait — there's no dynamic dispatch here (the macro expands to a direct
//! call through the module path for each sport), so a trait would add
//! ceremony without a caller that needs it.

use std::collections::HashMap;

use poem_openapi::{Enum, Object, Union};

use agon_core::sports::netball::{
    NetballFormatRecord, NetballFoulEventRecord, NetballFoulKindRecord, NetballGoalEventRecord,
    NetballLiveEventRecord, NetballPeriodEventRecord, NetballPeriodRecord, NetballPositionRecord,
    NetballScoreRecord, NetballStatsRecord,
};

use crate::GenericPlayerStats;
use crate::live_score::NewLiveEventInput;
use crate::mapping::{generic_stats_from_record, parse_ts, parse_ts_opt};

/// How this sport is named to people (e.g. share-card headings).
pub const LABEL: &str = "Netball";

// ===========================================================================
// API types (formerly `match_format::NetballFormat`,
// `detailed_score::netball`, `live_score::netball`).
// ===========================================================================

#[derive(Object, Clone)]
pub struct NetballFormat {
    /// Quarters per match — normally 4; some social leagues play 2 longer
    /// halves instead, so this isn't hardcoded to 4.
    pub num_quarters: u32,
    /// Minutes per quarter, e.g. 15 for a standard game or 12 for Fast5.
    pub quarter_length_minutes: u32,
    /// Whether a goal from the two-point zone counts double (Fast5/Netball
    /// Superleague Power Play rules). `false` for standard scoring, where
    /// every goal is worth one regardless of where it's shot from.
    pub two_point_zone: bool,
    /// Whether an extra period is played (typically "first to score" /
    /// golden goal) if the match is level after full time.
    pub extra_time: bool,
}

/// GS/GA are the only positions that can legally shoot, but (same stance as
/// `CricketFormat`'s doc comment) that isn't enforced here — purely
/// descriptive, for stat display.
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

/// A netball match's result: the goal tally, plus optional richer detail.
/// See the module doc comment for how the two live-scoring methods
/// (event-by-event, quarter-only) both fold into this one shape.
#[derive(Object)]
pub(crate) struct NetballScore {
    /// Goal tally, keyed by side id — exactly one entry per side, same
    /// "map, not a list" convention as `FootballScore.score`. In
    /// event-by-event mode this is folded from `goals`; in quarter-only mode
    /// it's just whatever the last `Period` marker said.
    score: HashMap<String, u32>,
    /// Every goal scored, if there's a goal-by-goal breakdown to hand over —
    /// `None` for a quarter-only-scored or manually-entered result, which
    /// has no such detail.
    goals: Option<Vec<NetballGoalEvent>>,
    /// Non-scoring infringements, for stat display — same role as
    /// `FootballScore.cards`. `None` for a quarter-only-scored or
    /// manually-entered result.
    fouls: Option<Vec<NetballFoulEvent>>,
    /// The most recent period marker seen, if any. `None` for a result with
    /// no live detail behind it.
    period: Option<NetballPeriod>,
    /// When each period marker was recorded, keyed by kind — same convention
    /// as `FootballScore.period_times`.
    period_times: Option<HashMap<NetballPeriod, chrono::DateTime<chrono::Utc>>>,
    /// The score *as of* each quarter-end marker — this is what lets a
    /// client render "Q1 12-9, Q2 22-18, ..." regardless of which
    /// live-scoring method produced it (see `NetballPeriodEvent::score`'s
    /// doc comment).
    period_scores: Option<HashMap<NetballPeriod, HashMap<String, u32>>>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — goals' scorer, fouls' player — keyed by that same
    /// (match-scoped) player id. Same mechanism and rationale as
    /// `FootballScore.players`.
    players: HashMap<String, crate::RosterPreviewPlayer>,
}

/// Netball live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Netball`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
///
/// The same three variants serve both of netball's live-scoring methods —
/// there's no separate vocabulary per method:
///
/// - **Event-by-event**: `Goal`/`Foul` as they happen, plus a `Period`
///   marker at each quarter's end for time-tracking. `NetballScore::score`
///   is folded from `Goal` events; the `Period` marker's own `score` should
///   just agree with that running total (see `NetballPeriodEvent::score`'s
///   doc comment for what happens if it doesn't).
/// - **Quarter-only**: no `Goal`/`Foul` events at all — just a `Period`
///   marker at each quarter's end, each one carrying the score directly.
///   `NetballScore::score` comes *only* from these markers.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum NetballLiveEvent {
    Goal(NetballGoalEvent),
    Foul(NetballFoulEvent),
    Period(NetballPeriodEvent),
}

#[derive(Object, Clone)]
pub struct NetballPeriodEvent {
    pub period: NetballPeriod,
    /// Cumulative score per side as of this marker — always present, not
    /// `Option`, unlike `FootballPeriodEvent` (which carries no score at
    /// all, since football's score is always derivable from `Goal` events
    /// alone). This is what lets both of netball's live-scoring methods
    /// share one event vocabulary: for event-by-event scoring this is a
    /// checkpoint that should already agree with the total folded from
    /// `Goal` events; for quarter-only scoring it's the *only* place the
    /// score ever comes from. Either way, `NetballScore::apply_event`
    /// simply overwrites `score` with this value — it doesn't need to know
    /// which method a given match is using.
    pub score: HashMap<String, u32>,
}

// ===========================================================================
// Event-log fold (formerly `live_score::netball`).
// ===========================================================================

impl NetballScore {
    /// Folds the whole event log into a `NetballScore` from scratch — the
    /// slow path, used to bootstrap a match's first score or recover from a
    /// missing/unparseable persisted record. Just `apply_event` run once per
    /// event in order; the fast (single-event) and slow (whole-log) paths
    /// share the exact same fold, so they can't disagree — same pattern as
    /// `crate::sports::football::FootballScore::from_events`.
    fn from_events(events: &[(chrono::DateTime<chrono::Utc>, NetballLiveEvent)]) -> Self {
        let mut score = NetballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            fouls: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            period_scores: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(*occurred_at, event);
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run on
    /// every append. `occurred_at` (not `recorded_at`) is threaded through
    /// separately from `event`, same reasoning as
    /// `crate::sports::football::FootballScore::apply_event`.
    fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &NetballLiveEvent,
    ) {
        match event {
            NetballLiveEvent::Goal(g) => {
                *self.score.entry(g.side_id.clone()).or_insert(0) +=
                    if g.two_points { 2 } else { 1 };
                // Stamp with the envelope's own `occurred_at` — the
                // recording device's clock — regardless of whatever (if
                // anything) the client sent in the payload itself. This is
                // what makes `occurred_at`, not the quarter-relative
                // `minute`, the authoritative order for a live-scored goal
                // (see `NetballGoalEvent::occurred_at`'s doc comment).
                let mut g = g.clone();
                g.occurred_at = Some(occurred_at);
                self.goals.get_or_insert_with(Vec::new).push(g);
            }
            NetballLiveEvent::Foul(fo) => {
                let mut fo = fo.clone();
                fo.occurred_at = Some(occurred_at);
                self.fouls.get_or_insert_with(Vec::new).push(fo);
            }
            NetballLiveEvent::Period(p) => {
                self.period_times
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, occurred_at);
                self.period_scores
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, p.score.clone());
                // The one line that unifies both live-scoring methods — see
                // `NetballPeriodEvent::score`'s doc comment.
                self.score = p.score.clone();
                self.period = Some(p.period);
            }
        }
    }
}

/// Folds an ordered event log into a full `NetballScore` — see
/// `crate::sports::football::from_events`'s doc comment for the uniform
/// signature every sport's `from_events` shares.
pub fn from_events(
    events: &[(chrono::DateTime<chrono::Utc>, NetballLiveEvent)],
    _format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> NetballScore {
    NetballScore::from_events(events)
}

/// The incremental fast path for a live-scoring append — folds `new_events`
/// into `score` in place. `None` means a mismatched event sport slipped
/// through (unreachable in practice; the caller has already rejected that
/// earlier). `_format` is unused (netball's fold needs no format-derived
/// rules) — kept in the signature only so every sport's `apply_new_events`
/// has the same shape to call generically, same reasoning as `from_events`.
pub fn apply_new_events(
    score: &mut NetballScore,
    new_events: &[NewLiveEventInput],
    _format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> Option<()> {
    for e in new_events {
        let crate::live_score::LiveEventInput::Netball(event) = &e.event else {
            return None;
        };
        score.apply_event(e.occurred_at, event);
    }
    Some(())
}

// ===========================================================================
// Generic-`Score`-dispatch helpers (formerly per-sport arms scattered across
// `main.rs`).
// ===========================================================================

/// This score's side ids, for validating a submitted score references only
/// sides that actually exist on the match.
pub fn side_ids(score: &NetballScore) -> Vec<&str> {
    score.score.keys().map(|k| k.as_str()).collect()
}

/// Re-point a `NetballScore`'s request-scoped client ids to the real side/
/// player ids assigned at creation — see `main.rs`'s (removed)
/// `resolve_score_ids` for the original shared doc comment. `None` if any
/// referenced side or player is unknown.
pub fn resolve_ids(
    s: &NetballScore,
    side_ids: &HashMap<String, String>,
    player_ids: &HashMap<String, String>,
) -> Option<NetballScore> {
    let map = |client_id: &str| side_ids.get(client_id).cloned();
    let pmap_opt = |id: &Option<String>| -> Option<Option<String>> {
        match id {
            Some(id) => player_ids.get(id).cloned().map(Some),
            None => Some(None),
        }
    };

    let mut score = HashMap::with_capacity(s.score.len());
    for (side_id, goals) in &s.score {
        score.insert(map(side_id)?, *goals);
    }
    let goals = match &s.goals {
        Some(gs) => {
            let mut out = Vec::with_capacity(gs.len());
            for g in gs {
                out.push(NetballGoalEvent {
                    side_id: map(&g.side_id)?,
                    scorer_player_id: pmap_opt(&g.scorer_player_id)?,
                    scorer_position: g.scorer_position,
                    two_points: g.two_points,
                    minute: g.minute,
                    occurred_at: g.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let fouls = match &s.fouls {
        Some(fs) => {
            let mut out = Vec::with_capacity(fs.len());
            for fo in fs {
                out.push(NetballFoulEvent {
                    side_id: map(&fo.side_id)?,
                    player_id: pmap_opt(&fo.player_id)?,
                    foul_kind: fo.foul_kind,
                    minute: fo.minute,
                    occurred_at: fo.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let period_scores = match &s.period_scores {
        Some(pss) => {
            let mut out = HashMap::with_capacity(pss.len());
            for (period, entries) in pss {
                let mut mapped = HashMap::with_capacity(entries.len());
                for (side_id, goals) in entries {
                    mapped.insert(map(side_id)?, *goals);
                }
                out.insert(*period, mapped);
            }
            Some(out)
        }
        None => None,
    };
    Some(NetballScore {
        score,
        goals,
        fouls,
        period: s.period,
        period_times: s.period_times.clone(),
        period_scores,
        players: HashMap::new(),
    })
}

/// Set this score's resolved-players map (see `Api::hydrate_score_players`).
pub fn set_players(
    score: &mut NetballScore,
    resolved: HashMap<String, crate::RosterPreviewPlayer>,
) {
    score.players = resolved;
}

/// Every player id referenced in a `NetballScore`: each goal's scorer, each
/// foul's player. May repeat the same id many times over — deduped
/// downstream by `Dao::batch_get_match_players`.
pub fn player_ids(score: &NetballScore) -> Vec<String> {
    let mut ids = Vec::new();
    for goal in score.goals.iter().flatten() {
        ids.extend(goal.scorer_player_id.clone());
    }
    for foul in score.fouls.iter().flatten() {
        ids.extend(foul.player_id.clone());
    }
    ids
}

/// Derives the winner (when decidable) from a live-scored netball match's
/// persisted score. `None` if the two sides are tied.
pub fn winner(score: &NetballScore, side_ids: &[String]) -> Option<String> {
    crate::two_side_winner(side_ids, |sid| *score.score.get(sid).unwrap_or(&0) as i64)
}

// ===========================================================================
// Stats (a user's lifetime netball totals, `UserStats::netball`).
// ===========================================================================

/// Lifetime netball stats: the common counters plus goals scored, derived
/// from every confirmed match's goal log. A quarter-only-scored or
/// manually-entered match has no goal-by-goal log, so it still counts
/// towards matches played/won/drawn/lost but adds no goals.
#[derive(Object)]
pub struct NetballPlayerStats {
    #[oai(flatten)]
    pub common: GenericPlayerStats,
    /// Goals scored (career total) — successful shots, so a two-point-zone
    /// goal counts once here even though it's worth two on the scoreboard.
    pub goals: i32,
}

pub fn stats_from_record(rec: &NetballStatsRecord) -> NetballPlayerStats {
    NetballPlayerStats {
        common: generic_stats_from_record(&rec.common),
        goals: rec.goals as i32,
    }
}

// ===========================================================================
// API<->DAO mapping.
// ===========================================================================

pub fn score_to_record(s: &NetballScore) -> NetballScoreRecord {
    NetballScoreRecord {
        score: s.score.clone(),
        goals: s
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_to_record).collect()),
        fouls: s
            .fouls
            .as_ref()
            .map(|fs| fs.iter().map(foul_event_to_record).collect()),
        period: s.period.as_ref().map(period_to_record),
        period_times: s.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| {
                    (
                        period_to_record(p),
                        t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    )
                })
                .collect()
        }),
        period_scores: s.period_scores.as_ref().map(|pss| {
            pss.iter()
                .map(|(p, entries)| (period_to_record(p), entries.clone()))
                .collect()
        }),
    }
}

pub fn score_from_record(rec: &NetballScoreRecord) -> NetballScore {
    NetballScore {
        score: rec.score.clone(),
        goals: rec
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_from_record).collect()),
        fouls: rec
            .fouls
            .as_ref()
            .map(|fs| fs.iter().map(foul_event_from_record).collect()),
        period: rec.period.as_ref().map(period_from_record),
        period_times: rec.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| (period_from_record(p), parse_ts(t)))
                .collect()
        }),
        period_scores: rec.period_scores.as_ref().map(|pss| {
            pss.iter()
                .map(|(p, entries)| (period_from_record(p), entries.clone()))
                .collect()
        }),
        // Not stored — `Api::hydrate_score_players` fills this afterward.
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &NetballFormat) -> NetballFormatRecord {
    NetballFormatRecord {
        num_quarters: f.num_quarters,
        quarter_length_minutes: f.quarter_length_minutes,
        two_point_zone: f.two_point_zone,
        extra_time: f.extra_time,
    }
}

pub fn format_from_record(f: &NetballFormatRecord) -> NetballFormat {
    NetballFormat {
        num_quarters: f.num_quarters,
        quarter_length_minutes: f.quarter_length_minutes,
        two_point_zone: f.two_point_zone,
        extra_time: f.extra_time,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above — both carry the same `NetballGoalEvent`/
/// `NetballGoalEventRecord` shape.
fn goal_event_to_record(g: &NetballGoalEvent) -> NetballGoalEventRecord {
    NetballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        scorer_position: g.scorer_position.as_ref().map(position_to_record),
        two_points: g.two_points,
        minute: g.minute,
        occurred_at: g
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn goal_event_from_record(rec: &NetballGoalEventRecord) -> NetballGoalEvent {
    NetballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        scorer_position: rec.scorer_position.as_ref().map(position_from_record),
        two_points: rec.two_points,
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn position_to_record(p: &NetballPosition) -> NetballPositionRecord {
    match p {
        NetballPosition::GoalShooter => NetballPositionRecord::GoalShooter,
        NetballPosition::GoalAttack => NetballPositionRecord::GoalAttack,
        NetballPosition::WingAttack => NetballPositionRecord::WingAttack,
        NetballPosition::Centre => NetballPositionRecord::Centre,
        NetballPosition::WingDefence => NetballPositionRecord::WingDefence,
        NetballPosition::GoalDefence => NetballPositionRecord::GoalDefence,
        NetballPosition::GoalKeeper => NetballPositionRecord::GoalKeeper,
    }
}

fn position_from_record(rec: &NetballPositionRecord) -> NetballPosition {
    match rec {
        NetballPositionRecord::GoalShooter => NetballPosition::GoalShooter,
        NetballPositionRecord::GoalAttack => NetballPosition::GoalAttack,
        NetballPositionRecord::WingAttack => NetballPosition::WingAttack,
        NetballPositionRecord::Centre => NetballPosition::Centre,
        NetballPositionRecord::WingDefence => NetballPosition::WingDefence,
        NetballPositionRecord::GoalDefence => NetballPosition::GoalDefence,
        NetballPositionRecord::GoalKeeper => NetballPosition::GoalKeeper,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above, same reasoning as `goal_event_to_record`.
fn foul_event_to_record(fo: &NetballFoulEvent) -> NetballFoulEventRecord {
    NetballFoulEventRecord {
        side_id: fo.side_id.clone(),
        player_id: fo.player_id.clone(),
        foul_kind: match fo.foul_kind {
            NetballFoulKind::Contact => NetballFoulKindRecord::Contact,
            NetballFoulKind::Obstruction => NetballFoulKindRecord::Obstruction,
            NetballFoulKind::Footwork => NetballFoulKindRecord::Footwork,
            NetballFoulKind::Offside => NetballFoulKindRecord::Offside,
            NetballFoulKind::HeldBall => NetballFoulKindRecord::HeldBall,
            NetballFoulKind::Other => NetballFoulKindRecord::Other,
        },
        minute: fo.minute,
        occurred_at: fo
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn foul_event_from_record(rec: &NetballFoulEventRecord) -> NetballFoulEvent {
    NetballFoulEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        foul_kind: match rec.foul_kind {
            NetballFoulKindRecord::Contact => NetballFoulKind::Contact,
            NetballFoulKindRecord::Obstruction => NetballFoulKind::Obstruction,
            NetballFoulKindRecord::Footwork => NetballFoulKind::Footwork,
            NetballFoulKindRecord::Offside => NetballFoulKind::Offside,
            NetballFoulKindRecord::HeldBall => NetballFoulKind::HeldBall,
            NetballFoulKindRecord::Other => NetballFoulKind::Other,
        },
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above (`period`/`period_times`/`period_scores`' map
/// keys) — both need the same `NetballPeriod`/`NetballPeriodRecord`
/// correspondence.
fn period_to_record(period: &NetballPeriod) -> NetballPeriodRecord {
    match period {
        NetballPeriod::Start => NetballPeriodRecord::Start,
        NetballPeriod::QuarterOneEnd => NetballPeriodRecord::QuarterOneEnd,
        NetballPeriod::QuarterTwoStart => NetballPeriodRecord::QuarterTwoStart,
        NetballPeriod::QuarterTwoEnd => NetballPeriodRecord::QuarterTwoEnd,
        NetballPeriod::QuarterThreeStart => NetballPeriodRecord::QuarterThreeStart,
        NetballPeriod::QuarterThreeEnd => NetballPeriodRecord::QuarterThreeEnd,
        NetballPeriod::QuarterFourStart => NetballPeriodRecord::QuarterFourStart,
        NetballPeriod::FullTime => NetballPeriodRecord::FullTime,
        NetballPeriod::ExtraTimeStart => NetballPeriodRecord::ExtraTimeStart,
        NetballPeriod::ExtraTimeEnd => NetballPeriodRecord::ExtraTimeEnd,
    }
}

fn period_from_record(rec: &NetballPeriodRecord) -> NetballPeriod {
    match rec {
        NetballPeriodRecord::Start => NetballPeriod::Start,
        NetballPeriodRecord::QuarterOneEnd => NetballPeriod::QuarterOneEnd,
        NetballPeriodRecord::QuarterTwoStart => NetballPeriod::QuarterTwoStart,
        NetballPeriodRecord::QuarterTwoEnd => NetballPeriod::QuarterTwoEnd,
        NetballPeriodRecord::QuarterThreeStart => NetballPeriod::QuarterThreeStart,
        NetballPeriodRecord::QuarterThreeEnd => NetballPeriod::QuarterThreeEnd,
        NetballPeriodRecord::QuarterFourStart => NetballPeriod::QuarterFourStart,
        NetballPeriodRecord::FullTime => NetballPeriod::FullTime,
        NetballPeriodRecord::ExtraTimeStart => NetballPeriod::ExtraTimeStart,
        NetballPeriodRecord::ExtraTimeEnd => NetballPeriod::ExtraTimeEnd,
    }
}

pub fn live_event_to_record(event: &NetballLiveEvent) -> NetballLiveEventRecord {
    match event {
        NetballLiveEvent::Goal(g) => NetballLiveEventRecord::Goal(goal_event_to_record(g)),
        NetballLiveEvent::Foul(fo) => NetballLiveEventRecord::Foul(foul_event_to_record(fo)),
        NetballLiveEvent::Period(p) => NetballLiveEventRecord::Period(NetballPeriodEventRecord {
            period: period_to_record(&p.period),
            score: p.score.clone(),
        }),
    }
}

pub fn live_event_from_record(rec: &NetballLiveEventRecord) -> NetballLiveEvent {
    match rec {
        NetballLiveEventRecord::Goal(g) => NetballLiveEvent::Goal(goal_event_from_record(g)),
        NetballLiveEventRecord::Foul(fo) => NetballLiveEvent::Foul(foul_event_from_record(fo)),
        NetballLiveEventRecord::Period(p) => NetballLiveEvent::Period(NetballPeriodEvent {
            period: period_from_record(&p.period),
            score: p.score.clone(),
        }),
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
        match_format_to_record,
    };
    use crate::match_format::MatchFormat;

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    fn goal(side_id: &str, minute: u32) -> NetballLiveEvent {
        NetballLiveEvent::Goal(NetballGoalEvent {
            side_id: side_id.into(),
            scorer_player_id: Some("shooter_1".into()),
            scorer_position: Some(NetballPosition::GoalShooter),
            two_points: false,
            minute: Some(minute),
            occurred_at: None,
        })
    }

    #[test]
    fn event_by_event_mode_derives_score_from_goals() {
        let events = vec![
            (ts(0), goal("kestrels", 2)),
            (ts(1), goal("kestrels", 5)),
            (ts(2), goal("harriers", 9)),
            (
                ts(3),
                NetballLiveEvent::Foul(NetballFoulEvent {
                    side_id: "harriers".into(),
                    player_id: Some("defender_1".into()),
                    foul_kind: NetballFoulKind::Contact,
                    minute: Some(10),
                    occurred_at: None,
                }),
            ),
            (
                ts(4),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 2),
                        ("harriers".to_string(), 1),
                    ]),
                }),
            ),
        ];

        let score = NetballScore::from_events(&events);

        assert_eq!(score.score.get("kestrels"), Some(&2));
        assert_eq!(score.score.get("harriers"), Some(&1));
        assert_eq!(score.goals.as_ref().unwrap().len(), 3);
        assert_eq!(score.fouls.as_ref().unwrap().len(), 1);
        assert!(matches!(score.period, Some(NetballPeriod::QuarterOneEnd)));
        assert_eq!(
            score
                .period_scores
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&HashMap::from([
                ("kestrels".to_string(), 2),
                ("harriers".to_string(), 1)
            ]))
        );
        assert_eq!(
            score
                .period_times
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&ts(4))
        );
    }

    #[test]
    fn quarter_only_mode_derives_score_purely_from_period_markers() {
        // No Goal/Foul events at all — just quarter-end markers, each
        // carrying the running score directly.
        let events = vec![
            (
                ts(15),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 12),
                        ("harriers".to_string(), 9),
                    ]),
                }),
            ),
            (
                ts(30),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterTwoEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 22),
                        ("harriers".to_string(), 18),
                    ]),
                }),
            ),
        ];

        let score = NetballScore::from_events(&events);

        assert_eq!(score.score.get("kestrels"), Some(&22));
        assert_eq!(score.score.get("harriers"), Some(&18));
        assert!(score.goals.as_ref().unwrap().is_empty());
        assert!(matches!(score.period, Some(NetballPeriod::QuarterTwoEnd)));
        assert_eq!(score.period_scores.as_ref().unwrap().len(), 2);
        // Q1's snapshot survives untouched alongside Q2's.
        assert_eq!(
            score
                .period_scores
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&HashMap::from([
                ("kestrels".to_string(), 12),
                ("harriers".to_string(), 9)
            ]))
        );
    }

    #[test]
    fn two_point_goals_count_double() {
        let events = vec![(
            ts(0),
            NetballLiveEvent::Goal(NetballGoalEvent {
                side_id: "kestrels".into(),
                scorer_player_id: Some("shooter_1".into()),
                scorer_position: Some(NetballPosition::GoalAttack),
                two_points: true,
                minute: Some(4),
                occurred_at: None,
            }),
        )];

        let score = NetballScore::from_events(&events);
        assert_eq!(score.score.get("kestrels"), Some(&2));
    }

    /// The bug this guards against: `minute` is quarter-relative and reset
    /// every quarter, so two goals in different quarters can carry the same
    /// (or an out-of-order) `minute` — sorting the events list on that alone
    /// scrambles later quarters in with earlier ones. `occurred_at` is
    /// absolute, so folding always produces goals/fouls in true chronological
    /// (recorded) order regardless of what `minute` says, and it's stamped
    /// from the envelope's own clock even when the client sends none.
    #[test]
    fn goals_and_fouls_are_stamped_with_occurred_at_from_the_envelope_in_recorded_order() {
        let events = vec![
            (ts(0), goal("kestrels", 3)),
            (
                ts(1),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([("kestrels".to_string(), 1)]),
                }),
            ),
            // A new quarter's clock resets, so this goal's `minute` (2) is
            // numerically *less* than the first goal's (3) despite happening
            // later in the match — exactly the scrambling `minute`-only
            // sorting produced.
            (ts(2), goal("kestrels", 2)),
        ];

        let score = NetballScore::from_events(&events);
        let goals = score.goals.as_ref().unwrap();
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].occurred_at, Some(ts(0)));
        assert_eq!(goals[1].occurred_at, Some(ts(2)));
        // Recorded (array) order already reflects true chronological order,
        // regardless of the quarter-relative `minute` values above.
        assert!(goals[0].occurred_at < goals[1].occurred_at);
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            (ts(0), goal("kestrels", 2)),
            (ts(1), goal("harriers", 5)),
            (
                ts(2),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 1),
                        ("harriers".to_string(), 1),
                    ]),
                }),
            ),
        ];

        let full = NetballScore::from_events(&events);

        let mut incremental = NetballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            fouls: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            period_scores: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event);
        }

        assert_eq!(incremental.score, full.score);
        assert_eq!(
            incremental.goals.as_ref().unwrap().len(),
            full.goals.as_ref().unwrap().len()
        );
        assert_eq!(incremental.period_times, full.period_times);
        assert_eq!(incremental.period_scores, full.period_scores);
    }

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    #[test]
    fn netball_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            NetballLiveEvent::Goal(NetballGoalEvent {
                side_id: "kestrels".into(),
                scorer_player_id: Some("shooter_1".into()),
                scorer_position: Some(NetballPosition::GoalShooter),
                two_points: false,
                minute: Some(4),
                occurred_at: Some(parse_ts("2024-05-01T20:04:00.000Z")),
            }),
            NetballLiveEvent::Foul(NetballFoulEvent {
                side_id: "harriers".into(),
                player_id: Some("defender_1".into()),
                foul_kind: NetballFoulKind::Contact,
                minute: Some(6),
                occurred_at: Some(parse_ts("2024-05-01T20:06:00.000Z")),
            }),
            NetballLiveEvent::Period(NetballPeriodEvent {
                period: NetballPeriod::QuarterOneEnd,
                score: HashMap::from([("kestrels".to_string(), 12), ("harriers".to_string(), 9)]),
            }),
        ];

        for event in events {
            let input = LiveEventInput::Netball(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn netball_format_round_trips_through_dao_mirror() {
        let format = MatchFormat::Netball(NetballFormat {
            num_quarters: 4,
            quarter_length_minutes: 15,
            two_point_zone: false,
            extra_time: true,
        });
        let original_json = format.to_json();
        let round_tripped = match_format_from_record(&match_format_to_record(&format));
        assert_eq!(original_json, round_tripped.to_json());
    }

    #[test]
    fn netball_score_round_trips_through_dao_mirror() {
        use crate::Score;

        let scores = vec![
            // A manually-entered / quarter-only-scored netball result: goal
            // tally only, no goal-by-goal detail.
            Score::Netball(NetballScore {
                score: HashMap::from([("kestrels".to_string(), 45), ("harriers".to_string(), 38)]),
                goals: None,
                fouls: None,
                period: Some(NetballPeriod::FullTime),
                period_times: None,
                period_scores: Some(HashMap::from([
                    (
                        NetballPeriod::QuarterOneEnd,
                        HashMap::from([("kestrels".to_string(), 12), ("harriers".to_string(), 9)]),
                    ),
                    (
                        NetballPeriod::FullTime,
                        HashMap::from([("kestrels".to_string(), 45), ("harriers".to_string(), 38)]),
                    ),
                ])),
                players: HashMap::new(),
            }),
            // A live-scored (event-by-event) netball result: full goal/foul
            // detail.
            Score::Netball(NetballScore {
                score: HashMap::from([("kestrels".to_string(), 2), ("harriers".to_string(), 1)]),
                goals: Some(vec![
                    NetballGoalEvent {
                        side_id: "kestrels".into(),
                        scorer_player_id: Some("player_1".into()),
                        scorer_position: Some(NetballPosition::GoalShooter),
                        two_points: false,
                        minute: Some(3),
                        occurred_at: Some(parse_ts("2024-05-01T20:03:00.000Z")),
                    },
                    NetballGoalEvent {
                        side_id: "kestrels".into(),
                        scorer_player_id: Some("player_2".into()),
                        scorer_position: Some(NetballPosition::GoalAttack),
                        two_points: true,
                        minute: Some(7),
                        occurred_at: Some(parse_ts("2024-05-01T20:07:00.000Z")),
                    },
                ]),
                fouls: Some(vec![NetballFoulEvent {
                    side_id: "harriers".into(),
                    player_id: Some("player_3".into()),
                    foul_kind: NetballFoulKind::Obstruction,
                    minute: Some(5),
                    occurred_at: Some(parse_ts("2024-05-01T20:05:00.000Z")),
                }]),
                period: Some(NetballPeriod::QuarterOneEnd),
                period_times: Some(HashMap::from([(
                    NetballPeriod::QuarterOneEnd,
                    parse_ts("2024-05-01T20:15:00.000Z"),
                )])),
                period_scores: Some(HashMap::from([(
                    NetballPeriod::QuarterOneEnd,
                    HashMap::from([("kestrels".to_string(), 2), ("harriers".to_string(), 1)]),
                )])),
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

    /// `UserStats.netball` carries the common counters flattened in next to
    /// `goals` (the same shape as cricket's/football's stats), so a client
    /// reading only `matches_played`/`wins`/... keeps working unchanged.
    #[test]
    fn netball_stats_serialize_flat_with_goals() {
        use agon_core::dao::records::{GenericSportStatsRecord, UserStatsRecord};

        let stats = crate::mapping::user_stats_from_record(&UserStatsRecord {
            netball: Some(NetballStatsRecord {
                common: GenericSportStatsRecord {
                    matches_played: 4,
                    wins: 3,
                    draws: 0,
                    losses: 1,
                },
                goals: 17,
            }),
            ..Default::default()
        });
        let json = stats.to_json().expect("serializes");
        let netball = json["netball"].as_object().expect("netball stats present");
        assert_eq!(netball["matches_played"], 4);
        assert_eq!(netball["wins"], 3);
        assert_eq!(netball["losses"], 1);
        assert_eq!(netball["win_percentage"], 75.0);
        assert_eq!(netball["goals"], 17);
        assert!(json["football"].is_null(), "no stats for an unplayed sport");
    }
}
