//! Football's whole API-side surface — see `crate::sports::netball`'s doc
//! comment for the pattern and its rationale.

use std::collections::HashMap;

use poem_openapi::{Enum, Object, Union};

use agon_core::sports::football::{
    FootballCardColorRecord, FootballCardEventRecord, FootballFormatRecord,
    FootballGoalEventRecord, FootballLiveEventRecord, FootballPenaltyShootoutKickRecord,
    FootballPeriodEventRecord, FootballPeriodRecord, FootballScoreRecord,
    FootballSubstitutionEventRecord,
};

use crate::live_score::NewLiveEventInput;
use crate::mapping::{parse_ts, parse_ts_opt};

// ===========================================================================
// API types (formerly `match_format::FootballFormat`,
// `detailed_score::football`, `live_score::football`).
// ===========================================================================

#[derive(Object, Clone)]
pub struct FootballFormat {
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

/// A football match's result: the goal tally, plus optional richer detail.
#[derive(Object)]
pub(crate) struct FootballScore {
    /// Goal tally, keyed by side id — exactly one entry per side, so a map
    /// rather than the `Vec<{side_id, ...}>` shape used where order or
    /// repeats matter (e.g. `goals`).
    score: HashMap<String, u32>,
    /// Every goal scored (normal + extra time; penalty-shootout kicks are
    /// tracked separately and never appear here), if there's a goal-by-goal
    /// breakdown to hand over.
    goals: Option<Vec<FootballGoalEvent>>,
    cards: Option<Vec<FootballCardEvent>>,
    substitutions: Option<Vec<FootballSubstitutionEvent>>,
    /// The most recent period marker seen, if any. `None` for a result with
    /// no live detail behind it.
    period: Option<FootballPeriod>,
    /// When each period marker was recorded, keyed by kind — one entry per
    /// `FootballPeriod` variant seen so far (a marker recorded twice
    /// overwrites, it doesn't append). Historical facts, not "current
    /// state" — nothing here goes blank once the match is over.
    period_times: Option<HashMap<FootballPeriod, chrono::DateTime<chrono::Utc>>>,
    /// Every penalty-shootout kick recorded, in order taken. Separate from
    /// `goals`/`score` — a shootout kick never counts as a match goal, only
    /// towards `penalty_shootout_score` — since the scoreline it decides
    /// (e.g. "1-1, Riverside win 4-3 on penalties") keeps the 90/120-minute
    /// score and the shootout tally visually distinct, same as it's reported
    /// in the real world.
    penalty_shootout: Option<Vec<FootballPenaltyShootoutKick>>,
    /// Running shootout tally (kicks scored, not kicks taken) per side,
    /// derived from `penalty_shootout` the same way `score` is derived from
    /// `goals`. Keyed by side id, same reasoning as `score`.
    penalty_shootout_score: Option<HashMap<String, u32>>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — `goals`' scorer/assist, `cards`' player, `substitutions`' in/
    /// out — keyed by that same (match-scoped) player id. Same mechanism and
    /// rationale as `CricketScore.players`.
    players: HashMap<String, crate::RosterPreviewPlayer>,
}

/// Football live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Football`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum FootballLiveEvent {
    Goal(FootballGoalEvent),
    Card(FootballCardEvent),
    Substitution(FootballSubstitutionEvent),
    Period(FootballPeriodEvent),
    PenaltyShootoutKick(FootballPenaltyShootoutKick),
}

#[derive(Object, Clone)]
pub struct FootballPeriodEvent {
    pub period: FootballPeriod,
}

// ===========================================================================
// Event-log fold (formerly `live_score::football`).
// ===========================================================================

impl FootballScore {
    /// Folds the whole event log into a `FootballScore` from scratch — the
    /// slow path, used to bootstrap a match's first score or recover from a
    /// missing/unparseable persisted record. Just `apply_event` run once per
    /// event in order; the fast (single-event) and slow (whole-log) paths
    /// share the exact same fold, so they can't disagree — same pattern as
    /// `crate::sports::cricket::CricketScore::from_events`.
    fn from_events(events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)]) -> Self {
        let mut score = FootballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            cards: Some(Vec::new()),
            substitutions: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            penalty_shootout: Some(Vec::new()),
            penalty_shootout_score: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(*occurred_at, event);
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run on
    /// every append. `occurred_at` (not `recorded_at`) is threaded through
    /// separately from `event` so a period marker's timestamp reflects when
    /// the half actually started/ended on the pitch, not when the server
    /// received it. Every optional field is populated (`Some`, even if
    /// empty) by the time this is called from a live-scoring path — only a
    /// bare manual entry ever leaves them `None` — so this always writes
    /// into an existing `Some`, never leaves a field `None` behind.
    fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &FootballLiveEvent,
    ) {
        match event {
            FootballLiveEvent::Goal(g) => {
                *self.score.entry(g.side_id.clone()).or_insert(0) += 1;
                // Stamp with the envelope's own `occurred_at` — the
                // recording device's clock — regardless of whatever (if
                // anything) the client sent in the payload itself (see
                // `FootballGoalEvent::occurred_at`'s doc comment).
                let mut g = g.clone();
                g.occurred_at = Some(occurred_at);
                self.goals.get_or_insert_with(Vec::new).push(g);
            }
            FootballLiveEvent::Card(c) => {
                let mut c = c.clone();
                c.occurred_at = Some(occurred_at);
                self.cards.get_or_insert_with(Vec::new).push(c);
            }
            FootballLiveEvent::Substitution(sub) => {
                let mut sub = sub.clone();
                sub.occurred_at = Some(occurred_at);
                self.substitutions.get_or_insert_with(Vec::new).push(sub);
            }
            FootballLiveEvent::Period(p) => {
                self.period_times
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, occurred_at);
                self.period = Some(p.period);
            }
            FootballLiveEvent::PenaltyShootoutKick(k) => {
                if k.scored {
                    *self
                        .penalty_shootout_score
                        .get_or_insert_with(HashMap::new)
                        .entry(k.side_id.clone())
                        .or_insert(0) += 1;
                }
                self.penalty_shootout
                    .get_or_insert_with(Vec::new)
                    .push(k.clone());
            }
        }
    }
}

/// Folds an ordered event log into a full `FootballScore` — the uniform
/// signature `agon_sports!`'s `derive_live_score` dispatches through (see
/// `sports::mod`'s doc comment). Football's own `FootballScore::from_events`
/// takes no format, so `format` is unused here — kept in the signature only
/// so every sport's `from_events` has the same shape to call generically;
/// cricket's is the one that actually needs it.
pub fn from_events(
    events: &[(chrono::DateTime<chrono::Utc>, FootballLiveEvent)],
    _format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> FootballScore {
    FootballScore::from_events(events)
}

/// The incremental fast path for a live-scoring append — see
/// `crate::sports::netball::apply_new_events`'s doc comment.
pub fn apply_new_events(
    score: &mut FootballScore,
    new_events: &[NewLiveEventInput],
    _format: Option<&agon_core::dao::records::MatchFormatRecord>,
) -> Option<()> {
    for e in new_events {
        let crate::live_score::LiveEventInput::Football(event) = &e.event else {
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
pub fn side_ids(score: &FootballScore) -> Vec<&str> {
    score.score.keys().map(|k| k.as_str()).collect()
}

/// Re-point a `FootballScore`'s request-scoped client ids to the real side/
/// player ids assigned at creation — see `crate::sports::netball::resolve_ids`'s
/// doc comment. `None` if any referenced side or player is unknown.
pub fn resolve_ids(
    s: &FootballScore,
    side_ids: &HashMap<String, String>,
    player_ids: &HashMap<String, String>,
) -> Option<FootballScore> {
    let map = |client_id: &str| side_ids.get(client_id).cloned();
    let pmap = |client_id: &str| player_ids.get(client_id).cloned();

    let mut score = HashMap::with_capacity(s.score.len());
    for (side_id, goals) in &s.score {
        score.insert(map(side_id)?, *goals);
    }
    let goals = match &s.goals {
        Some(gs) => {
            let mut out = Vec::with_capacity(gs.len());
            for g in gs {
                out.push(FootballGoalEvent {
                    side_id: map(&g.side_id)?,
                    scorer_player_id: match &g.scorer_player_id {
                        Some(id) => Some(pmap(id)?),
                        None => None,
                    },
                    assist_player_id: match &g.assist_player_id {
                        Some(id) => Some(pmap(id)?),
                        None => None,
                    },
                    own_goal: g.own_goal,
                    penalty: g.penalty,
                    minute: g.minute,
                    occurred_at: g.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let cards = match &s.cards {
        Some(cs) => {
            let mut out = Vec::with_capacity(cs.len());
            for c in cs {
                out.push(FootballCardEvent {
                    side_id: map(&c.side_id)?,
                    player_id: pmap(&c.player_id)?,
                    color: c.color.clone(),
                    minute: c.minute,
                    occurred_at: c.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let substitutions = match &s.substitutions {
        Some(subs) => {
            let mut out = Vec::with_capacity(subs.len());
            for sub in subs {
                out.push(FootballSubstitutionEvent {
                    side_id: map(&sub.side_id)?,
                    player_in_id: pmap(&sub.player_in_id)?,
                    player_out_id: pmap(&sub.player_out_id)?,
                    minute: sub.minute,
                    occurred_at: sub.occurred_at,
                });
            }
            Some(out)
        }
        None => None,
    };
    let penalty_shootout = match &s.penalty_shootout {
        Some(ks) => {
            let mut out = Vec::with_capacity(ks.len());
            for k in ks {
                out.push(FootballPenaltyShootoutKick {
                    side_id: map(&k.side_id)?,
                    scored: k.scored,
                });
            }
            Some(out)
        }
        None => None,
    };
    let penalty_shootout_score = match &s.penalty_shootout_score {
        Some(pss) => {
            let mut out = HashMap::with_capacity(pss.len());
            for (side_id, kicks) in pss {
                out.insert(map(side_id)?, *kicks);
            }
            Some(out)
        }
        None => None,
    };
    Some(FootballScore {
        score,
        goals,
        cards,
        substitutions,
        period: s.period,
        period_times: s.period_times.clone(),
        penalty_shootout,
        penalty_shootout_score,
        players: HashMap::new(),
    })
}

/// Set this score's resolved-players map (see `Api::hydrate_score_players`).
pub fn set_players(
    score: &mut FootballScore,
    resolved: HashMap<String, crate::RosterPreviewPlayer>,
) {
    score.players = resolved;
}

/// Every player id referenced in a `FootballScore`: each goal's scorer/
/// assist, each card's player, each substitution's player in/out. May repeat
/// the same id many times over — deduped downstream by
/// `Dao::batch_get_match_players`.
pub fn player_ids(score: &FootballScore) -> Vec<String> {
    let mut ids = Vec::new();
    for goal in score.goals.iter().flatten() {
        ids.extend(goal.scorer_player_id.clone());
        ids.extend(goal.assist_player_id.clone());
    }
    for card in score.cards.iter().flatten() {
        ids.push(card.player_id.clone());
    }
    for sub in score.substitutions.iter().flatten() {
        ids.push(sub.player_in_id.clone());
        ids.push(sub.player_out_id.clone());
    }
    ids
}

/// Derives the winner (when decidable) from a live-scored football match's
/// persisted score. Still level on goals falls back to the penalty-shootout
/// tally.
pub fn winner(score: &FootballScore, side_ids: &[String]) -> Option<String> {
    crate::two_side_winner(side_ids, |sid| *score.score.get(sid).unwrap_or(&0) as i64).or_else(
        || {
            crate::two_side_winner(side_ids, |sid| {
                score
                    .penalty_shootout_score
                    .as_ref()
                    .and_then(|pss| pss.get(sid))
                    .copied()
                    .unwrap_or(0) as i64
            })
        },
    )
}

// ===========================================================================
// API<->DAO mapping.
// ===========================================================================

pub fn score_to_record(s: &FootballScore) -> FootballScoreRecord {
    FootballScoreRecord {
        score: s.score.clone(),
        goals: s
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_to_record).collect()),
        cards: s
            .cards
            .as_ref()
            .map(|cs| cs.iter().map(card_event_to_record).collect()),
        substitutions: s
            .substitutions
            .as_ref()
            .map(|subs| subs.iter().map(substitution_event_to_record).collect()),
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
        penalty_shootout: s.penalty_shootout.as_ref().map(|ks| {
            ks.iter()
                .map(|k| FootballPenaltyShootoutKickRecord {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
                .collect()
        }),
        penalty_shootout_score: s.penalty_shootout_score.clone(),
    }
}

pub fn score_from_record(rec: &FootballScoreRecord) -> FootballScore {
    FootballScore {
        score: rec.score.clone(),
        goals: rec
            .goals
            .as_ref()
            .map(|gs| gs.iter().map(goal_event_from_record).collect()),
        cards: rec
            .cards
            .as_ref()
            .map(|cs| cs.iter().map(card_event_from_record).collect()),
        substitutions: rec
            .substitutions
            .as_ref()
            .map(|subs| subs.iter().map(substitution_event_from_record).collect()),
        period: rec.period.as_ref().map(period_from_record),
        period_times: rec.period_times.as_ref().map(|pts| {
            pts.iter()
                .map(|(p, t)| (period_from_record(p), parse_ts(t)))
                .collect()
        }),
        penalty_shootout: rec.penalty_shootout.as_ref().map(|ks| {
            ks.iter()
                .map(|k| FootballPenaltyShootoutKick {
                    side_id: k.side_id.clone(),
                    scored: k.scored,
                })
                .collect()
        }),
        penalty_shootout_score: rec.penalty_shootout_score.clone(),
        // Not stored — `Api::hydrate_score_players` fills this afterward.
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &FootballFormat) -> FootballFormatRecord {
    FootballFormatRecord {
        half_length_minutes: f.half_length_minutes,
        num_halves: f.num_halves,
        extra_time: f.extra_time,
        extra_time_half_length_minutes: f.extra_time_half_length_minutes,
        penalties: f.penalties,
    }
}

pub fn format_from_record(f: &FootballFormatRecord) -> FootballFormat {
    FootballFormat {
        half_length_minutes: f.half_length_minutes,
        num_halves: f.num_halves,
        extra_time: f.extra_time,
        extra_time_half_length_minutes: f.extra_time_half_length_minutes,
        penalties: f.penalties,
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above — both carry the same `FootballGoalEvent`/
/// `FootballGoalEventRecord` shape.
fn goal_event_to_record(g: &FootballGoalEvent) -> FootballGoalEventRecord {
    FootballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        assist_player_id: g.assist_player_id.clone(),
        own_goal: g.own_goal,
        penalty: g.penalty,
        minute: g.minute,
        occurred_at: g
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn goal_event_from_record(rec: &FootballGoalEventRecord) -> FootballGoalEvent {
    FootballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        assist_player_id: rec.assist_player_id.clone(),
        own_goal: rec.own_goal,
        penalty: rec.penalty,
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above, same reasoning as `goal_event_to_record`.
fn card_event_to_record(c: &FootballCardEvent) -> FootballCardEventRecord {
    FootballCardEventRecord {
        side_id: c.side_id.clone(),
        player_id: c.player_id.clone(),
        color: match c.color {
            FootballCardColor::Yellow => FootballCardColorRecord::Yellow,
            FootballCardColor::Red => FootballCardColorRecord::Red,
        },
        minute: c.minute,
        occurred_at: c
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn card_event_from_record(rec: &FootballCardEventRecord) -> FootballCardEvent {
    FootballCardEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        color: match rec.color {
            FootballCardColorRecord::Yellow => FootballCardColor::Yellow,
            FootballCardColorRecord::Red => FootballCardColor::Red,
        },
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn substitution_event_to_record(s: &FootballSubstitutionEvent) -> FootballSubstitutionEventRecord {
    FootballSubstitutionEventRecord {
        side_id: s.side_id.clone(),
        player_in_id: s.player_in_id.clone(),
        player_out_id: s.player_out_id.clone(),
        minute: s.minute,
        occurred_at: s
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn substitution_event_from_record(
    rec: &FootballSubstitutionEventRecord,
) -> FootballSubstitutionEvent {
    FootballSubstitutionEvent {
        side_id: rec.side_id.clone(),
        player_in_id: rec.player_in_id.clone(),
        player_out_id: rec.player_out_id.clone(),
        minute: rec.minute,
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

/// Shared by the live-event mapping below and `score_to_record`/
/// `score_from_record` above (`period`/`period_times`' map keys) — both need
/// the same `FootballPeriod`/`FootballPeriodRecord` correspondence.
fn period_to_record(period: &FootballPeriod) -> FootballPeriodRecord {
    match period {
        FootballPeriod::KickOff => FootballPeriodRecord::KickOff,
        FootballPeriod::HalfTime => FootballPeriodRecord::HalfTime,
        FootballPeriod::SecondHalfKickOff => FootballPeriodRecord::SecondHalfKickOff,
        FootballPeriod::FullTime => FootballPeriodRecord::FullTime,
        FootballPeriod::ExtraTimeKickOff => FootballPeriodRecord::ExtraTimeKickOff,
        FootballPeriod::ExtraTimeHalfTime => FootballPeriodRecord::ExtraTimeHalfTime,
        FootballPeriod::ExtraTimeSecondHalfKickOff => {
            FootballPeriodRecord::ExtraTimeSecondHalfKickOff
        }
        FootballPeriod::ExtraTimeFullTime => FootballPeriodRecord::ExtraTimeFullTime,
        FootballPeriod::PenaltiesComplete => FootballPeriodRecord::PenaltiesComplete,
    }
}

fn period_from_record(rec: &FootballPeriodRecord) -> FootballPeriod {
    match rec {
        FootballPeriodRecord::KickOff => FootballPeriod::KickOff,
        FootballPeriodRecord::HalfTime => FootballPeriod::HalfTime,
        FootballPeriodRecord::SecondHalfKickOff => FootballPeriod::SecondHalfKickOff,
        FootballPeriodRecord::FullTime => FootballPeriod::FullTime,
        FootballPeriodRecord::ExtraTimeKickOff => FootballPeriod::ExtraTimeKickOff,
        FootballPeriodRecord::ExtraTimeHalfTime => FootballPeriod::ExtraTimeHalfTime,
        FootballPeriodRecord::ExtraTimeSecondHalfKickOff => {
            FootballPeriod::ExtraTimeSecondHalfKickOff
        }
        FootballPeriodRecord::ExtraTimeFullTime => FootballPeriod::ExtraTimeFullTime,
        FootballPeriodRecord::PenaltiesComplete => FootballPeriod::PenaltiesComplete,
    }
}

pub fn live_event_to_record(event: &FootballLiveEvent) -> FootballLiveEventRecord {
    match event {
        FootballLiveEvent::Goal(g) => FootballLiveEventRecord::Goal(goal_event_to_record(g)),
        FootballLiveEvent::Card(c) => FootballLiveEventRecord::Card(card_event_to_record(c)),
        FootballLiveEvent::Substitution(s) => {
            FootballLiveEventRecord::Substitution(substitution_event_to_record(s))
        }
        FootballLiveEvent::Period(p) => {
            FootballLiveEventRecord::Period(FootballPeriodEventRecord {
                period: period_to_record(&p.period),
            })
        }
        FootballLiveEvent::PenaltyShootoutKick(k) => {
            FootballLiveEventRecord::PenaltyShootoutKick(FootballPenaltyShootoutKickRecord {
                side_id: k.side_id.clone(),
                scored: k.scored,
            })
        }
    }
}

pub fn live_event_from_record(rec: &FootballLiveEventRecord) -> FootballLiveEvent {
    match rec {
        FootballLiveEventRecord::Goal(g) => FootballLiveEvent::Goal(goal_event_from_record(g)),
        FootballLiveEventRecord::Card(c) => FootballLiveEvent::Card(card_event_from_record(c)),
        FootballLiveEventRecord::Substitution(s) => {
            FootballLiveEvent::Substitution(substitution_event_from_record(s))
        }
        FootballLiveEventRecord::Period(p) => FootballLiveEvent::Period(FootballPeriodEvent {
            period: period_from_record(&p.period),
        }),
        FootballLiveEventRecord::PenaltyShootoutKick(k) => {
            FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                side_id: k.side_id.clone(),
                scored: k.scored,
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
        match_format_to_record,
    };
    use crate::match_format::MatchFormat;

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    #[test]
    fn derives_score_goals_cards_and_substitutions() {
        let events = vec![
            (
                ts(41),
                FootballLiveEvent::Goal(FootballGoalEvent {
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
                FootballLiveEvent::Card(FootballCardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(58),
                    occurred_at: None,
                }),
            ),
            // An own goal counts for the side benefiting from it.
            (
                ts(63),
                FootballLiveEvent::Goal(FootballGoalEvent {
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
                FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                    side_id: "oak_park".into(),
                    player_in_id: "moreno".into(),
                    player_out_id: "khan".into(),
                    minute: Some(70),
                    occurred_at: None,
                }),
            ),
            (
                ts(94),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::FullTime,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);

        assert_eq!(score.score.len(), 1);
        assert_eq!(score.score.get("riverside"), Some(&2));
        assert_eq!(score.goals.as_ref().unwrap().len(), 2);
        assert_eq!(score.cards.as_ref().unwrap().len(), 1);
        assert_eq!(score.substitutions.as_ref().unwrap().len(), 1);
        assert!(matches!(score.period, Some(FootballPeriod::FullTime)));
        assert_eq!(
            score
                .period_times
                .as_ref()
                .unwrap()
                .get(&FootballPeriod::FullTime),
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
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::KickOff,
                }),
            ),
            (
                ts(46),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::HalfTime,
                }),
            ),
            (
                ts(60),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::SecondHalfKickOff,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(period_times.get(&FootballPeriod::KickOff), Some(&ts(0)));
        assert_eq!(period_times.get(&FootballPeriod::HalfTime), Some(&ts(46)));
        assert_eq!(
            period_times.get(&FootballPeriod::SecondHalfKickOff),
            Some(&ts(60))
        );
        assert_eq!(period_times.get(&FootballPeriod::FullTime), None);
        assert!(matches!(
            score.period,
            Some(FootballPeriod::SecondHalfKickOff)
        ));
    }

    #[test]
    fn extra_time_and_penalties_markers_get_timestamps_too() {
        let events = vec![
            (
                ts(90),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeKickOff,
                }),
            ),
            (
                ts(105),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeHalfTime,
                }),
            ),
            (
                ts(120),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeSecondHalfKickOff,
                }),
            ),
            (
                ts(150),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::ExtraTimeFullTime,
                }),
            ),
            (
                ts(160),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::PenaltiesComplete,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);
        let period_times = score.period_times.as_ref().unwrap();

        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeKickOff),
            Some(&ts(90))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeHalfTime),
            Some(&ts(105))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeSecondHalfKickOff),
            Some(&ts(120))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::ExtraTimeFullTime),
            Some(&ts(150))
        );
        assert_eq!(
            period_times.get(&FootballPeriod::PenaltiesComplete),
            Some(&ts(160))
        );
    }

    #[test]
    fn penalty_shootout_kicks_tally_scored_only() {
        let events = vec![
            (
                ts(160),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(161),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: false,
                }),
            ),
            (
                ts(162),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "riverside".into(),
                    scored: true,
                }),
            ),
            (
                ts(163),
                FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                    side_id: "oak_park".into(),
                    scored: true,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);
        let shootout_score = score.penalty_shootout_score.as_ref().unwrap();

        assert_eq!(score.penalty_shootout.as_ref().unwrap().len(), 4);
        // Only sides with at least one scored kick get a tally entry — same
        // "absence means zero" convention as `score`.
        assert_eq!(shootout_score.len(), 2);
        assert_eq!(shootout_score.get("riverside"), Some(&2));
        assert_eq!(shootout_score.get("oak_park"), Some(&1));
        // A shootout kick never counts as a match goal.
        assert!(score.score.is_empty());
        assert!(score.goals.as_ref().unwrap().is_empty());
    }

    /// Same guard as netball's — see
    /// `netball::tests::goals_and_fouls_are_stamped_with_occurred_at_from_the_envelope_in_recorded_order`.
    /// Football's `minute` doesn't reset per period the way netball's does,
    /// so this doesn't fix a scrambling bug, but it still has to hold: a
    /// live-scored goal is timestamped by the server's own clock, not
    /// whatever (if anything) a client sends.
    #[test]
    fn goals_cards_and_subs_are_stamped_with_occurred_at_from_the_envelope() {
        let events = vec![
            (
                ts(0),
                FootballLiveEvent::Goal(FootballGoalEvent {
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
                FootballLiveEvent::Card(FootballCardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: FootballCardColor::Yellow,
                    minute: None,
                    occurred_at: None,
                }),
            ),
        ];

        let score = FootballScore::from_events(&events);

        assert_eq!(score.goals.as_ref().unwrap()[0].occurred_at, Some(ts(0)));
        assert_eq!(score.cards.as_ref().unwrap()[0].occurred_at, Some(ts(1)));
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            (
                ts(0),
                FootballLiveEvent::Period(FootballPeriodEvent {
                    period: FootballPeriod::KickOff,
                }),
            ),
            (
                ts(12),
                FootballLiveEvent::Goal(FootballGoalEvent {
                    side_id: "riverside".into(),
                    scorer_player_id: Some("alvarez".into()),
                    assist_player_id: None,
                    own_goal: false,
                    penalty: false,
                    minute: Some(12),
                    occurred_at: None,
                }),
            ),
            (
                ts(58),
                FootballLiveEvent::Card(FootballCardEvent {
                    side_id: "oak_park".into(),
                    player_id: "khan".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(58),
                    occurred_at: None,
                }),
            ),
        ];

        let full = FootballScore::from_events(&events);

        // Apply the same events one at a time, incrementally, and check the
        // final state matches the full fold exactly.
        let mut incremental = FootballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            cards: Some(Vec::new()),
            substitutions: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            penalty_shootout: Some(Vec::new()),
            penalty_shootout_score: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event);
        }

        assert_eq!(incremental.score.len(), full.score.len());
        assert_eq!(
            incremental.goals.as_ref().unwrap().len(),
            full.goals.as_ref().unwrap().len()
        );
        assert_eq!(
            incremental.cards.as_ref().unwrap().len(),
            full.cards.as_ref().unwrap().len()
        );
        assert_eq!(incremental.period_times, full.period_times);
    }

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    #[test]
    fn football_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            FootballLiveEvent::Goal(FootballGoalEvent {
                side_id: "riverside".into(),
                scorer_player_id: Some("alvarez".into()),
                assist_player_id: None,
                own_goal: true,
                penalty: false,
                minute: Some(63),
                occurred_at: Some(parse_ts("2024-05-01T20:03:00.000Z")),
            }),
            FootballLiveEvent::Card(FootballCardEvent {
                side_id: "oak_park".into(),
                player_id: "khan".into(),
                color: FootballCardColor::Red,
                minute: None,
                occurred_at: None,
            }),
            FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                side_id: "oak_park".into(),
                player_in_id: "moreno".into(),
                player_out_id: "khan".into(),
                minute: Some(70),
                occurred_at: Some(parse_ts("2024-05-01T20:10:00.000Z")),
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeKickOff,
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeSecondHalfKickOff,
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeFullTime,
            }),
            FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                side_id: "riverside".into(),
                scored: true,
            }),
        ];

        for event in events {
            let input = LiveEventInput::Football(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn football_format_round_trips_through_dao_mirror() {
        let format = MatchFormat::Football(FootballFormat {
            half_length_minutes: 40,
            num_halves: 2,
            extra_time: true,
            extra_time_half_length_minutes: Some(15),
            penalties: true,
        });
        let original_json = format.to_json();
        let round_tripped = match_format_from_record(&match_format_to_record(&format));
        assert_eq!(original_json, round_tripped.to_json());
    }

    #[test]
    fn football_score_round_trips_through_dao_mirror() {
        use crate::Score;

        let scores = vec![
            // A manually-entered football result: totals only, no detail.
            Score::Football(FootballScore {
                score: HashMap::from([("side_red".to_string(), 3), ("side_blue".to_string(), 1)]),
                goals: None,
                cards: None,
                substitutions: None,
                period: None,
                period_times: None,
                penalty_shootout: None,
                penalty_shootout_score: None,
                players: HashMap::new(),
            }),
            // A live-scored football result: full goal/card/sub detail.
            Score::Football(FootballScore {
                score: HashMap::from([("side_red".to_string(), 2), ("side_blue".to_string(), 1)]),
                goals: Some(vec![
                    FootballGoalEvent {
                        side_id: "side_red".into(),
                        scorer_player_id: Some("player_1".into()),
                        assist_player_id: Some("player_2".into()),
                        own_goal: false,
                        penalty: false,
                        minute: Some(23),
                        occurred_at: Some(parse_ts("2024-05-01T20:23:00.000Z")),
                    },
                    FootballGoalEvent {
                        side_id: "side_blue".into(),
                        scorer_player_id: None,
                        assist_player_id: None,
                        own_goal: true,
                        penalty: false,
                        minute: None,
                        occurred_at: None,
                    },
                ]),
                cards: Some(vec![FootballCardEvent {
                    side_id: "side_blue".into(),
                    player_id: "player_3".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(60),
                    occurred_at: Some(parse_ts("2024-05-01T21:00:00.000Z")),
                }]),
                substitutions: Some(vec![FootballSubstitutionEvent {
                    side_id: "side_red".into(),
                    player_in_id: "player_4".into(),
                    player_out_id: "player_1".into(),
                    minute: Some(75),
                    occurred_at: Some(parse_ts("2024-05-01T21:15:00.000Z")),
                }]),
                period: Some(FootballPeriod::FullTime),
                period_times: Some(HashMap::from([
                    (
                        FootballPeriod::KickOff,
                        parse_ts("2024-05-01T20:00:00.000Z"),
                    ),
                    (
                        FootballPeriod::FullTime,
                        parse_ts("2024-05-01T21:45:00.000Z"),
                    ),
                ])),
                penalty_shootout: Some(vec![FootballPenaltyShootoutKick {
                    side_id: "side_red".into(),
                    scored: true,
                }]),
                penalty_shootout_score: Some(HashMap::from([("side_red".to_string(), 1)])),
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
