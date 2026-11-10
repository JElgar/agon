//! Auto team builder endpoints: preview a split of a pool between squads
//! (teamless tournament) or sides (match), then apply exactly that split.
//!
//! Preview writes nothing. Apply takes the previewed assignment back plus
//! the `roster_version` the preview was computed from, and refuses with 409
//! if the pool has changed since — so what's written is always what the
//! organiser saw, and a re-preview is the only recovery. The decision itself
//! (`agon_core::team_builder`) is pure; this module only reads the pool,
//! guards hidden ratings on the way out, and writes the result.
//!
//! Organiser-only throughout: nothing here is visible to players, and the
//! squads/sides it writes are ordinary `squad` / `side_id` values.

use std::collections::{BTreeMap, HashMap, HashSet};

use agon_core::dao;
use agon_core::dao::records::{MatchPlayerRecord, UserRecord};
use agon_core::rating::{display_rating, ladder_for_tag};
use agon_core::team_builder as tb;
use poem::Result;
use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};
use tracing::info;

use crate::mapping::dao_internal;
use crate::{AuthSchema, caller_is_match_admin, caller_is_tournament_admin, require_uid};

pub struct TeamBuilderApi;

// ===========================================================================
// API types
// ===========================================================================

/// How to split the pool.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum AutoAssignMethod {
    /// Shuffle and deal.
    Random,
    /// Even out strength, then spread the players we know least about.
    AutoBalanced,
}

/// Who the split may move.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[oai(rename_all = "snake_case")]
pub enum AutoAssignScope {
    /// Only players with no squad/side yet; everyone placed stays put.
    #[default]
    Unassigned,
    /// Start over (pins still hold).
    Everyone,
}

/// Organiser override for one player, relative to this pool, for this
/// draw only. Absent means "as expected".
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum PlayerLevel {
    Newer,
    Stronger,
}

/// Where a player's estimate came from.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum StrengthSignal {
    /// 10+ ranked games: rating only.
    Rated,
    /// Placed, under 10 ranked games: rating blended with win rate.
    Blended,
    /// Still in placements. No number is returned.
    Placement,
    /// Games played, none ranked.
    WinRate,
    /// Account with no games in this sport.
    New,
    /// No account.
    Guest,
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum BalanceVerdict {
    Even,
    Close,
    Uneven,
}

#[derive(Object, Debug, Clone)]
pub struct PlayerLevelInput {
    /// Entrant id (tournament) or player id (match).
    pub id: String,
    pub level: PlayerLevel,
}

#[derive(Object, Debug, Clone)]
pub struct SquadPin {
    /// Entrant id.
    pub id: String,
    pub squad: u32,
}

#[derive(Object, Debug, Clone)]
pub struct SidePin {
    /// Player id.
    pub id: String,
    pub side_id: String,
}

/// `POST /tournaments/{id}/squads/auto-assign`.
#[derive(Object, Debug, Clone)]
pub struct TournamentAutoAssignInput {
    pub method: AutoAssignMethod,
    pub scope: Option<AutoAssignScope>,
    /// Defaults to `ceil(pool / squad_size)`, at least 2, and never fewer
    /// than the squads already in use when `scope` is `unassigned`.
    pub squad_count: Option<u32>,
    pub pins: Option<Vec<SquadPin>>,
    pub levels: Option<Vec<PlayerLevelInput>>,
    /// Same seed, same split. Omit for a fresh one (echoed back).
    pub seed: Option<u32>,
}

/// `POST /matches/{id}/sides/auto-assign`.
#[derive(Object, Debug, Clone)]
pub struct MatchAutoAssignInput {
    pub method: AutoAssignMethod,
    pub scope: Option<AutoAssignScope>,
    pub pins: Option<Vec<SidePin>>,
    pub levels: Option<Vec<PlayerLevelInput>>,
    pub seed: Option<u32>,
}

#[derive(Object, Debug, Clone)]
pub struct AutoAssignWinRate {
    /// `(wins + ½·draws) / played`.
    pub raw: f64,
    /// Pulled toward 50% for small samples.
    pub adjusted: f64,
    pub played: u32,
}

#[derive(Object, Debug, Clone)]
pub struct AutoAssignPlayer {
    /// Entrant id (tournament) or player id (match).
    pub id: String,
    pub user_id: Option<String>,
    pub signal: StrengthSignal,
    /// 1 = little history, 2 = some, 3 = solid.
    pub certainty: u8,
    /// Estimated strength on the displayed rating scale. `null` while the
    /// player is still in placements.
    pub estimate: Option<i32>,
    /// `null` while the player is still in placements.
    pub ranked_games: Option<u32>,
    pub win_rate: Option<AutoAssignWinRate>,
    /// Share of the estimate that came from the rating (0–1).
    pub rating_weight: f64,
    pub level: Option<PlayerLevel>,
    pub pinned: bool,
    /// This split puts them somewhere other than where they are now.
    pub moved: bool,
}

#[derive(Object, Debug, Clone)]
pub struct AutoAssignSide {
    /// Squad index (tournament).
    pub squad: Option<u32>,
    /// Side id (match).
    pub side_id: Option<String>,
    /// Average estimated strength, display scale, rounded to 10. `null` for
    /// an empty side.
    pub average: Option<i32>,
    pub unsure_count: u32,
    pub players: Vec<AutoAssignPlayer>,
}

#[derive(Object, Debug, Clone)]
pub struct AutoAssignWinProbability {
    /// Chance the first listed side wins.
    pub expected: f64,
    /// The same chance if our estimates are one standard deviation off
    /// either way.
    pub low: f64,
    pub high: f64,
}

#[derive(Object, Debug, Clone)]
pub struct AutoAssignPreview {
    pub seed: u32,
    /// Send back on apply; apply refuses if the pool changed since.
    pub roster_version: String,
    pub method: AutoAssignMethod,
    pub verdict: BalanceVerdict,
    /// Display points between the strongest and weakest side's average.
    pub spread: i32,
    /// Two sides only.
    pub win_probability: Option<AutoAssignWinProbability>,
    pub unsure_count: u32,
    pub sides: Vec<AutoAssignSide>,
}

#[derive(Object, Debug, Clone)]
pub struct SquadAssignment {
    pub id: String,
    pub squad: u32,
}

#[derive(Object, Debug, Clone)]
pub struct SideAssignment {
    pub id: String,
    pub side_id: String,
}

/// `PUT /tournaments/{id}/squads/assignments`.
#[derive(Object, Debug, Clone)]
pub struct ApplySquadAssignmentsInput {
    pub roster_version: String,
    pub assignments: Vec<SquadAssignment>,
}

/// `PUT /matches/{id}/sides/assignments`.
#[derive(Object, Debug, Clone)]
pub struct ApplySideAssignmentsInput {
    pub roster_version: String,
    pub assignments: Vec<SideAssignment>,
}

#[derive(Object, Debug, Clone)]
pub struct ApplyAssignmentsResult {
    /// How many entries/players actually changed squad or side.
    pub updated: u32,
}

#[derive(ApiResponse)]
pub enum AutoAssignResponse {
    #[oai(status = 200)]
    Preview(Json<AutoAssignPreview>),

    /// Bad input: unknown pin/level id, too few squads, a pool that doesn't
    /// fit the sides' caps.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 403)]
    Forbidden(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The tournament/match isn't in a state where squads/sides can change.
    #[oai(status = 409)]
    Conflict(PlainText<String>),
}

#[derive(ApiResponse)]
pub enum ApplyAssignmentsResponse {
    #[oai(status = 200)]
    Applied(Json<ApplyAssignmentsResult>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 403)]
    Forbidden(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The pool changed since the preview (`roster_version` mismatch), or
    /// the tournament/match can no longer be changed. Re-preview.
    #[oai(status = 409)]
    Conflict(PlainText<String>),
}

// ===========================================================================
// Shared planning
// ===========================================================================

/// One pool member, surface-agnostic.
struct Member<'a> {
    id: String,
    user_id: Option<String>,
    user: Option<&'a UserRecord>,
    /// Side index they're on now.
    current: Option<usize>,
}

/// A stable fingerprint of who is in the pool and where. FNV-1a, so it is
/// the same across processes and deploys (unlike `DefaultHasher`).
fn roster_version(entries: &mut [(String, String)]) -> String {
    entries.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (id, at) in entries.iter() {
        for b in id.bytes().chain([0u8]).chain(at.bytes()).chain([0xff]) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

fn history(member: &Member<'_>, sport_tag: &str, level: Option<PlayerLevel>) -> tb::History {
    let ladder = ladder_for_tag(sport_tag);
    let rating = member.user.and_then(|u| {
        let r = u.ratings.get(ladder.as_ref()?.as_str())?;
        Some(tb::RatingInput {
            mu: r.mu,
            sigma: r.sigma,
            matches_rated: r.matches_rated,
        })
    });
    let record = member
        .user
        .and_then(|u| u.stats.common(sport_tag))
        .map(|c| tb::WinRecord {
            played: c.matches_played,
            wins: c.wins,
            draws: c.draws,
            losses: c.losses,
        })
        .unwrap_or_default();
    tb::History {
        rating,
        record,
        guest: member.user_id.is_none(),
        level: level.map(|l| match l {
            PlayerLevel::Newer => tb::Level::Newer,
            PlayerLevel::Stronger => tb::Level::Stronger,
        }),
    }
}

fn signal_to_api(s: tb::Signal) -> StrengthSignal {
    match s {
        tb::Signal::Rated => StrengthSignal::Rated,
        tb::Signal::Blended => StrengthSignal::Blended,
        tb::Signal::Placement => StrengthSignal::Placement,
        tb::Signal::WinRate => StrengthSignal::WinRate,
        tb::Signal::New => StrengthSignal::New,
        tb::Signal::Guest => StrengthSignal::Guest,
    }
}

/// The one place a hidden placement rating is withheld: the solver used
/// it, the response never carries it.
fn player_to_api(
    m: &Member<'_>,
    e: &tb::Estimate,
    pinned: bool,
    placed: usize,
) -> AutoAssignPlayer {
    let hidden = e.signal == tb::Signal::Placement;
    AutoAssignPlayer {
        id: m.id.clone(),
        user_id: m.user_id.clone(),
        signal: signal_to_api(e.signal),
        certainty: e.certainty,
        estimate: (!hidden).then(|| display_rating(e.mu).round() as i32),
        ranked_games: (!hidden).then_some(e.ranked_games as u32),
        win_rate: e.win_rate.map(|w| AutoAssignWinRate {
            raw: w.raw,
            adjusted: w.adjusted,
            played: w.played as u32,
        }),
        rating_weight: e.rating_weight,
        level: e.level.map(|l| match l {
            tb::Level::Newer => PlayerLevel::Newer,
            tb::Level::Stronger => PlayerLevel::Stronger,
        }),
        pinned,
        moved: m.current != Some(placed),
    }
}

fn round_to_10(x: f64) -> i32 {
    ((x / 10.0).round() * 10.0) as i32
}

struct PlanInput<'a> {
    members: Vec<Member<'a>>,
    sport_tag: &'a str,
    caps: Vec<Option<usize>>,
    /// Member index → side index.
    pins: HashMap<usize, usize>,
    levels: HashMap<usize, PlayerLevel>,
    scope: AutoAssignScope,
    method: AutoAssignMethod,
    seed: u32,
    roster_version: String,
}

/// Run the core and shape the preview. `side_key` labels side `i` in the
/// response (squad index or side id).
fn plan(
    input: PlanInput<'_>,
    side_key: impl Fn(usize) -> (Option<u32>, Option<String>),
) -> std::result::Result<AutoAssignPreview, String> {
    let histories: Vec<tb::History> = input
        .members
        .iter()
        .enumerate()
        .map(|(i, m)| history(m, input.sport_tag, input.levels.get(&i).copied()))
        .collect();
    let estimates = tb::estimate_pool(&histories);
    let slots: Vec<tb::PoolSlot> = input
        .members
        .iter()
        .enumerate()
        .zip(&estimates)
        .map(|((i, m), e)| tb::PoolSlot {
            mu: e.mu,
            sigma: e.sigma,
            pinned_to: input.pins.get(&i).copied().or(match input.scope {
                AutoAssignScope::Unassigned => m.current,
                AutoAssignScope::Everyone => None,
            }),
        })
        .collect();
    let method = match input.method {
        AutoAssignMethod::Random => tb::Method::Random,
        AutoAssignMethod::AutoBalanced => tb::Method::AutoBalanced,
    };
    let sides =
        tb::build(&slots, &input.caps, method, u64::from(input.seed)).map_err(|e| e.to_string())?;
    let balance = tb::balance(&slots, &sides);

    let api_sides = sides
        .iter()
        .enumerate()
        .map(|(s, idxs)| {
            let (squad, side_id) = side_key(s);
            AutoAssignSide {
                squad,
                side_id,
                average: balance.side_mean[s].map(|m| round_to_10(display_rating(m))),
                unsure_count: idxs
                    .iter()
                    .filter(|&&i| estimates[i].certainty == 1)
                    .count() as u32,
                players: idxs
                    .iter()
                    .map(|&i| {
                        player_to_api(
                            &input.members[i],
                            &estimates[i],
                            slots[i].pinned_to.is_some(),
                            s,
                        )
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(AutoAssignPreview {
        seed: input.seed,
        roster_version: input.roster_version,
        method: input.method,
        verdict: match balance.verdict {
            tb::Verdict::Even => BalanceVerdict::Even,
            tb::Verdict::Close => BalanceVerdict::Close,
            tb::Verdict::Uneven => BalanceVerdict::Uneven,
        },
        spread: balance.spread.round() as i32,
        win_probability: balance.win_probability.map(|w| AutoAssignWinProbability {
            expected: w.expected,
            low: w.low,
            high: w.high,
        }),
        unsure_count: estimates.iter().filter(|e| e.certainty == 1).count() as u32,
        sides: api_sides,
    })
}

/// Resolve `id`-keyed pins/levels to member indices; unknown ids are a
/// validation error rather than silently ignored.
fn index_by_id<'a, T>(
    members: &[Member<'_>],
    items: impl IntoIterator<Item = (&'a str, T)>,
) -> std::result::Result<HashMap<usize, T>, String> {
    let by_id: HashMap<&str, usize> = members
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id.as_str(), i))
        .collect();
    items
        .into_iter()
        .map(|(id, v)| {
            by_id
                .get(id)
                .map(|&i| (i, v))
                .ok_or_else(|| format!("`{id}` isn't in the pool"))
        })
        .collect()
}

// ===========================================================================
// Pool loading
// ===========================================================================

struct TournamentPool {
    tournament: dao::records::TournamentRecord,
    entries: Vec<dao::records::TournamentEntryRecord>,
    users: HashMap<String, UserRecord>,
}

enum Loaded<T> {
    Ok(T),
    NotFound(String),
    Forbidden(String),
    Conflict(String),
}

async fn load_tournament_pool(
    dao: &dao::Dao,
    tournament_id: &str,
    uid: &str,
) -> Result<Loaded<TournamentPool>> {
    let Some(tournament) = dao
        .get_tournament(tournament_id)
        .await
        .map_err(dao_internal)?
    else {
        return Ok(Loaded::NotFound("tournament not found".into()));
    };
    if !caller_is_tournament_admin(&tournament, uid) {
        return Ok(Loaded::Forbidden(
            "only the tournament's owner or an admin can assign squads".into(),
        ));
    }
    if tournament.entry_audience != dao::records::TournamentEntryAudienceRecord::Players
        || tournament.team_formation.is_none()
    {
        return Ok(Loaded::Conflict(
            "squads only apply to a teamless tournament".into(),
        ));
    }
    if tournament.status != "open" {
        return Ok(Loaded::Conflict(format!(
            "tournament is `{}`, not `open`",
            tournament.status
        )));
    }
    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = dao
            .list_tournament_entries(tournament_id, cursor.as_deref(), 100)
            .await
            .map_err(dao_internal)?;
        entries.extend(page.items.into_iter().filter(|e| e.status == "accepted"));
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    entries.sort_by(|a, b| a.entrant_id.cmp(&b.entrant_id));
    let ids: Vec<String> = entries.iter().map(|e| e.entrant_id.clone()).collect();
    let users = dao.batch_get_users(&ids).await.map_err(dao_internal)?;
    Ok(Loaded::Ok(TournamentPool {
        tournament,
        entries,
        users,
    }))
}

fn tournament_roster_version(entries: &[dao::records::TournamentEntryRecord]) -> String {
    let mut v: Vec<(String, String)> = entries
        .iter()
        .map(|e| {
            (
                e.entrant_id.clone(),
                e.squad.map(|s| s.to_string()).unwrap_or_default(),
            )
        })
        .collect();
    roster_version(&mut v)
}

struct MatchPool {
    agg: dao::match_ops::MatchAggregate,
    /// Sides in a stable order (by side id).
    side_ids: Vec<String>,
    /// Players who take a spot, by player id.
    players: Vec<MatchPlayerRecord>,
    users: HashMap<String, UserRecord>,
}

async fn load_match_pool(dao: &dao::Dao, match_id: &str, uid: &str) -> Result<Loaded<MatchPool>> {
    let Some(agg) = dao.get_match(match_id).await.map_err(dao_internal)? else {
        return Ok(Loaded::NotFound("match not found".into()));
    };
    if !caller_is_match_admin(dao, &agg, uid).await? {
        return Ok(Loaded::Forbidden(
            "only the match's organisers can assign sides".into(),
        ));
    }
    if agg.match_.status != "scheduled"
        || agg.match_.confirmed_score.is_some()
        || agg.match_.pending_score.is_some()
    {
        return Ok(Loaded::Conflict(
            "sides can only be auto-assigned before the match is played".into(),
        ));
    }
    let mut side_ids: Vec<String> = agg.sides.iter().map(|s| s.side_id.clone()).collect();
    side_ids.sort();
    if side_ids.len() < 2 {
        return Ok(Loaded::Conflict(
            "the match needs at least two sides".into(),
        ));
    }
    let mut players: Vec<MatchPlayerRecord> = agg
        .players
        .iter()
        .filter(|p| p.occupies_slot())
        .cloned()
        .collect();
    players.sort_by(|a, b| a.player_id.cmp(&b.player_id));
    let user_ids: Vec<String> = players.iter().filter_map(|p| p.user_id.clone()).collect();
    let users = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;
    Ok(Loaded::Ok(MatchPool {
        agg,
        side_ids,
        players,
        users,
    }))
}

fn match_roster_version(players: &[MatchPlayerRecord]) -> String {
    let mut v: Vec<(String, String)> = players
        .iter()
        .map(|p| (p.player_id.clone(), p.side_id.clone().unwrap_or_default()))
        .collect();
    roster_version(&mut v)
}

macro_rules! unwrap_loaded {
    ($loaded:expr, $resp:ident) => {
        match $loaded {
            Loaded::Ok(v) => v,
            Loaded::NotFound(m) => return Ok($resp::NotFound(PlainText(m))),
            Loaded::Forbidden(m) => return Ok($resp::Forbidden(PlainText(m))),
            Loaded::Conflict(m) => return Ok($resp::Conflict(PlainText(m))),
        }
    };
}

// ===========================================================================
// Endpoints
// ===========================================================================

#[OpenApi]
impl TeamBuilderApi {
    /// Preview an auto-assigned split of a teamless tournament's accepted
    /// entrants into squads. Writes nothing; apply with
    /// `PUT /tournaments/{id}/squads/assignments`. Organiser-only.
    #[oai(
        path = "/tournaments/:tournament_id/squads/auto-assign",
        method = "post"
    )]
    async fn preview_tournament_squads(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
        Path(tournament_id): Path<String>,
        input: Json<TournamentAutoAssignInput>,
    ) -> Result<AutoAssignResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let pool = unwrap_loaded!(
            load_tournament_pool(dao, &tournament_id, &uid).await?,
            AutoAssignResponse
        );
        let scope = input.scope.unwrap_or_default();
        if pool.entries.len() < 2 {
            return Ok(AutoAssignResponse::ValidationError(PlainText(
                "need at least two accepted entrants".into(),
            )));
        }

        let in_use = pool
            .entries
            .iter()
            .filter_map(|e| e.squad)
            .max()
            .map_or(0, |m| m + 1);
        let pinned_max = input
            .pins
            .iter()
            .flatten()
            .map(|p| p.squad + 1)
            .max()
            .unwrap_or(0);
        let size = pool.tournament.squad_size.unwrap_or(2).max(1) as usize;
        let default_count = pool.entries.len().div_ceil(size).max(2) as u32;
        let mut squad_count = input.squad_count.unwrap_or(default_count);
        if scope == AutoAssignScope::Unassigned {
            squad_count = squad_count.max(in_use);
        }
        squad_count = squad_count.max(pinned_max);
        if squad_count < 2 {
            return Ok(AutoAssignResponse::ValidationError(PlainText(
                "need at least two squads".into(),
            )));
        }
        if squad_count as usize > pool.entries.len() {
            return Ok(AutoAssignResponse::ValidationError(PlainText(format!(
                "{squad_count} squads is more than the {} entrants",
                pool.entries.len()
            ))));
        }

        let version = tournament_roster_version(&pool.entries);
        let members: Vec<Member<'_>> = pool
            .entries
            .iter()
            .map(|e| Member {
                id: e.entrant_id.clone(),
                user_id: Some(e.entrant_id.clone()),
                user: pool.users.get(&e.entrant_id),
                current: e.squad.map(|s| s as usize),
            })
            .collect();
        let pins = match index_by_id(
            &members,
            input
                .pins
                .iter()
                .flatten()
                .map(|p| (p.id.as_str(), p.squad as usize)),
        ) {
            Ok(p) => p,
            Err(m) => return Ok(AutoAssignResponse::ValidationError(PlainText(m))),
        };
        let levels = match index_by_id(
            &members,
            input
                .levels
                .iter()
                .flatten()
                .map(|l| (l.id.as_str(), l.level)),
        ) {
            Ok(l) => l,
            Err(m) => return Ok(AutoAssignResponse::ValidationError(PlainText(m))),
        };
        let seed = input.seed.unwrap_or_else(tb::new_seed);
        info!(
            tournament_id = %tournament_id,
            method = ?input.method,
            pool = members.len(),
            squads = squad_count,
            "auto-assign preview"
        );
        let preview = plan(
            PlanInput {
                members,
                sport_tag: &pool.tournament.match_type,
                caps: vec![None; squad_count as usize],
                pins,
                levels,
                scope,
                method: input.method,
                seed,
                roster_version: version,
            },
            |s| (Some(s as u32), None),
        );
        Ok(match preview {
            Ok(p) => AutoAssignResponse::Preview(Json(p)),
            Err(m) => AutoAssignResponse::ValidationError(PlainText(m)),
        })
    }

    /// Apply a previewed squad split. `roster_version` must match the
    /// tournament's current pool, or this is a 409 and nothing is written.
    /// Entrants not listed keep their squad. Organiser-only.
    #[oai(
        path = "/tournaments/:tournament_id/squads/assignments",
        method = "put"
    )]
    async fn apply_tournament_squads(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
        Path(tournament_id): Path<String>,
        input: Json<ApplySquadAssignmentsInput>,
    ) -> Result<ApplyAssignmentsResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let pool = unwrap_loaded!(
            load_tournament_pool(dao, &tournament_id, &uid).await?,
            ApplyAssignmentsResponse
        );
        if tournament_roster_version(&pool.entries) != input.roster_version {
            return Ok(ApplyAssignmentsResponse::Conflict(PlainText(
                "entries have changed since this split was previewed".into(),
            )));
        }
        let current: HashMap<&str, Option<u32>> = pool
            .entries
            .iter()
            .map(|e| (e.entrant_id.as_str(), e.squad))
            .collect();
        let mut seen = HashSet::new();
        let mut changes = Vec::new();
        for a in &input.assignments {
            let Some(&now) = current.get(a.id.as_str()) else {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("`{}` isn't an accepted entrant", a.id),
                )));
            };
            if !seen.insert(a.id.as_str()) {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("`{}` is assigned twice", a.id),
                )));
            }
            if now != Some(a.squad) {
                changes.push((a.id.clone(), a.squad));
            }
        }
        dao.set_tournament_entry_squads(&tournament_id, &changes)
            .await
            .map_err(|e| match e {
                dao::DaoError::Conflict(_) => poem::Error::from_string(
                    "an entry changed while applying; preview again",
                    poem::http::StatusCode::CONFLICT,
                ),
                other => dao_internal(other),
            })?;
        info!(tournament_id = %tournament_id, updated = changes.len(), "auto-assign applied");
        Ok(ApplyAssignmentsResponse::Applied(Json(
            ApplyAssignmentsResult {
                updated: changes.len() as u32,
            },
        )))
    }

    /// Preview an auto-assigned split of a match's players between its
    /// existing sides, respecting each side's `max_players`. Writes
    /// nothing; apply with `PUT /matches/{id}/sides/assignments`.
    /// Organiser-only, and only before the match is played.
    #[oai(path = "/matches/:match_id/sides/auto-assign", method = "post")]
    async fn preview_match_sides(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<MatchAutoAssignInput>,
    ) -> Result<AutoAssignResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let pool = unwrap_loaded!(
            load_match_pool(dao, &match_id, &uid).await?,
            AutoAssignResponse
        );
        let side_index: HashMap<&str, usize> = pool
            .side_ids
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i))
            .collect();
        let caps: Vec<Option<usize>> = pool
            .side_ids
            .iter()
            .map(|sid| {
                pool.agg
                    .sides
                    .iter()
                    .find(|s| &s.side_id == sid)
                    .and_then(|s| s.max_players)
                    .map(|m| m as usize)
            })
            .collect();
        let members: Vec<Member<'_>> = pool
            .players
            .iter()
            .map(|p| Member {
                id: p.player_id.clone(),
                user_id: p.user_id.clone(),
                user: p.user_id.as_ref().and_then(|u| pool.users.get(u)),
                current: p
                    .side_id
                    .as_deref()
                    .and_then(|s| side_index.get(s).copied()),
            })
            .collect();
        let mut pin_pairs = Vec::new();
        for p in input.pins.iter().flatten() {
            let Some(&s) = side_index.get(p.side_id.as_str()) else {
                return Ok(AutoAssignResponse::ValidationError(PlainText(format!(
                    "side `{}` isn't on this match",
                    p.side_id
                ))));
            };
            pin_pairs.push((p.id.as_str(), s));
        }
        let pins = match index_by_id(&members, pin_pairs) {
            Ok(p) => p,
            Err(m) => return Ok(AutoAssignResponse::ValidationError(PlainText(m))),
        };
        let levels = match index_by_id(
            &members,
            input
                .levels
                .iter()
                .flatten()
                .map(|l| (l.id.as_str(), l.level)),
        ) {
            Ok(l) => l,
            Err(m) => return Ok(AutoAssignResponse::ValidationError(PlainText(m))),
        };
        let seed = input.seed.unwrap_or_else(tb::new_seed);
        info!(
            match_id = %match_id,
            method = ?input.method,
            pool = members.len(),
            sides = pool.side_ids.len(),
            "auto-assign preview"
        );
        let side_ids = pool.side_ids.clone();
        let preview = plan(
            PlanInput {
                members,
                sport_tag: &pool.agg.match_.match_type,
                caps,
                pins,
                levels,
                scope: input.scope.unwrap_or_default(),
                method: input.method,
                seed,
                roster_version: match_roster_version(&pool.players),
            },
            |s| (None, Some(side_ids[s].clone())),
        );
        Ok(match preview {
            Ok(p) => AutoAssignResponse::Preview(Json(p)),
            Err(m) => AutoAssignResponse::ValidationError(PlainText(m)),
        })
    }

    /// Apply a previewed side split. `roster_version` must match the
    /// match's current roster, or this is a 409 and nothing is written.
    /// Players not listed keep their side. Moving a player off a side drops
    /// any side-admin grant they held for it, same as a manual reassign.
    #[oai(path = "/matches/:match_id/sides/assignments", method = "put")]
    async fn apply_match_sides(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<ApplySideAssignmentsInput>,
    ) -> Result<ApplyAssignmentsResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let pool = unwrap_loaded!(
            load_match_pool(dao, &match_id, &uid).await?,
            ApplyAssignmentsResponse
        );
        if match_roster_version(&pool.players) != input.roster_version {
            return Ok(ApplyAssignmentsResponse::Conflict(PlainText(
                "the roster has changed since this split was previewed".into(),
            )));
        }
        let by_id: HashMap<&str, &MatchPlayerRecord> = pool
            .players
            .iter()
            .map(|p| (p.player_id.as_str(), p))
            .collect();
        let mut final_side: HashMap<&str, Option<&str>> = pool
            .players
            .iter()
            .map(|p| (p.player_id.as_str(), p.side_id.as_deref()))
            .collect();
        let mut seen = HashSet::new();
        let mut moves: Vec<(MatchPlayerRecord, Option<String>)> = Vec::new();
        for a in &input.assignments {
            let Some(&player) = by_id.get(a.id.as_str()) else {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("`{}` isn't on this match's roster", a.id),
                )));
            };
            if !pool.side_ids.contains(&a.side_id) {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("side `{}` isn't on this match", a.side_id),
                )));
            }
            if !seen.insert(a.id.as_str()) {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("`{}` is assigned twice", a.id),
                )));
            }
            final_side.insert(a.id.as_str(), Some(a.side_id.as_str()));
            if player.side_id.as_deref() != Some(a.side_id.as_str()) {
                let mut updated = player.clone();
                updated.side_id = Some(a.side_id.clone());
                moves.push((updated, player.side_id.clone()));
            }
        }
        // Caps, judged on where everyone ends up.
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for side in final_side.values().flatten() {
            *counts.entry(side).or_default() += 1;
        }
        for side in &pool.agg.sides {
            let n = counts.get(side.side_id.as_str()).copied().unwrap_or(0);
            if let Some(max) = side.max_players
                && n > max as usize
            {
                return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                    format!("{n} players don't fit a side of {max}"),
                )));
            }
        }
        if moves.len() > 99 {
            return Ok(ApplyAssignmentsResponse::ValidationError(PlainText(
                "too many players to move in one go".into(),
            )));
        }

        dao.reassign_match_players_tx(&match_id, &moves)
            .await
            .map_err(|e| match e {
                dao::DaoError::Conflict(_) => poem::Error::from_string(
                    "the roster changed while applying; preview again",
                    poem::http::StatusCode::CONFLICT,
                ),
                other => dao_internal(other),
            })?;

        // Same default as a manual reassign: leaving a side drops its
        // side-admin grant. One write per affected side.
        let mut revoked: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (updated, old) in &moves {
            if let (Some(old), Some(user_id)) = (old.as_deref(), updated.user_id.as_deref()) {
                revoked.entry(old).or_default().push(user_id);
            }
        }
        for (side_id, users) in revoked {
            if let Some(admins) = pool.agg.match_.side_authority.get(side_id)
                && admins.iter().any(|u| users.contains(&u.as_str()))
            {
                let remaining: Vec<String> = admins
                    .iter()
                    .filter(|u| !users.contains(&u.as_str()))
                    .cloned()
                    .collect();
                dao.set_side_admins(&match_id, side_id, &remaining)
                    .await
                    .map_err(dao_internal)?;
            }
        }
        if !moves.is_empty() {
            dao.refresh_side_roster_previews(&match_id)
                .await
                .map_err(dao_internal)?;
        }
        info!(match_id = %match_id, updated = moves.len(), "auto-assign applied");
        Ok(ApplyAssignmentsResponse::Applied(Json(
            ApplyAssignmentsResult {
                updated: moves.len() as u32,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_version_is_order_independent_and_sensitive_to_moves() {
        let mut a = vec![
            ("p1".to_string(), "s1".to_string()),
            ("p2".to_string(), String::new()),
        ];
        let mut b = vec![
            ("p2".to_string(), String::new()),
            ("p1".to_string(), "s1".to_string()),
        ];
        assert_eq!(roster_version(&mut a), roster_version(&mut b));
        let mut moved = vec![
            ("p1".to_string(), "s2".to_string()),
            ("p2".to_string(), String::new()),
        ];
        assert_ne!(roster_version(&mut a), roster_version(&mut moved));
        let mut joined = a.clone();
        joined.push(("p3".to_string(), String::new()));
        assert_ne!(roster_version(&mut a), roster_version(&mut joined));
    }

    #[test]
    fn placement_ratings_never_leave_the_service() {
        let member = Member {
            id: "u1".into(),
            user_id: Some("u1".into()),
            user: None,
            current: None,
        };
        let estimate = tb::Estimate {
            mu: 30.0,
            sigma: 5.0,
            signal: tb::Signal::Placement,
            certainty: 2,
            rating_weight: 0.3,
            ranked_games: 3,
            win_rate: None,
            level: None,
        };
        let p = player_to_api(&member, &estimate, false, 0);
        assert_eq!(p.estimate, None);
        assert_eq!(p.ranked_games, None);
        assert!(p.moved);
    }
}
