//! Team stats reads: `GET /teams/{team_id}/summary` and
//! `GET /teams/{team_id}/leaderboard`.
//!
//! Both read OpenSearch (`agon_services_core::opensearch::team_stats`), which
//! the worker keeps in sync from DynamoDB. They're answered from aggregations,
//! so their cost doesn't grow with how many matches a team has played.
//!
//! The data is only complete once the OpenSearch backfill has run against the
//! table (`docs/opensearch.md`). Before then these endpoints work but only see
//! matches written since OpenSearch was deployed, so the UI shouldn't switch
//! to them until the backfill has been done.

use std::collections::HashSet;

use agon_services_core::dao;
use agon_services_core::opensearch::OpenSearchClient;
use agon_services_core::opensearch::team_stats::{
    self as ts, LeaderboardScope, RecordCounts, TeamResult, UpcomingMatch,
};
use poem::Result;
use poem::http::StatusCode;
use poem::web::Data;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};
use tracing::error;

use crate::mapping::{
    confirmed_score_from_record, dao_internal, match_stats_from_record, match_type_tag, parse_ts,
    side_result_from_record,
};
use crate::{
    AuthSchema, ConfirmedScore, MatchType, Photo, SportMatchStats, SportSideResult, require_uid,
};

/// The OpenSearch client the search, stats and admin endpoints read. The
/// service refuses to start without `OPENSEARCH_URL`.
#[derive(Clone)]
pub struct StatsStore(pub OpenSearchClient);

/// How many results `recent_results` covers.
const RECENT_RESULTS: u32 = 10;
/// The overview uses the standard layout once a team has played this many
/// external matches in the last 12 months.
const STANDARD_LAYOUT_MIN_MATCHES: u64 = 3;
/// Win rate is only shown from this many appearances.
const WIN_RATE_MIN_APPS: u64 = 5;
/// Page size when reading the member list to flag ringers.
const MEMBER_PAGE: u32 = 50;

pub struct TeamStatsApi;

// ── Shapes ──────────────────────────────────────────────────────────────────

/// What kind of match this is, from the team's point of view.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum TeamMatchKind {
    League,
    Cup,
    /// Against another team, outside a tournament.
    Friendly,
    /// The team against itself (an in-house game).
    Squad,
    /// No teams involved. Never appears on a team's own lists.
    Pickup,
}

pub(crate) fn kind_tag(k: TeamMatchKind) -> &'static str {
    match k {
        TeamMatchKind::League => "league",
        TeamMatchKind::Cup => "cup",
        TeamMatchKind::Friendly => "friendly",
        TeamMatchKind::Squad => "squad",
        TeamMatchKind::Pickup => "pickup",
    }
}

fn kind_from_str(s: &str) -> TeamMatchKind {
    match s {
        "league" => TeamMatchKind::League,
        "cup" => TeamMatchKind::Cup,
        "squad" => TeamMatchKind::Squad,
        "pickup" => TeamMatchKind::Pickup,
        _ => TeamMatchKind::Friendly,
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum TeamOutcome {
    Won,
    Drawn,
    Lost,
}

fn outcome_from_str(s: &str) -> TeamOutcome {
    match s {
        "won" => TeamOutcome::Won,
        "lost" => TeamOutcome::Lost,
        _ => TeamOutcome::Drawn,
    }
}

/// A team's results over a set of matches against other teams.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct TeamRecord {
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
    /// The team's scoreline over these matches in its sport's terms, e.g.
    /// `result.football` (goals for and against) or `result.cricket` (runs
    /// and wickets, for and against).
    pub result: SportSideResult,
    /// The sport's own counters for the team over these matches, e.g.
    /// `stats.football` (goals, assists, clean sheets) or `stats.cricket`
    /// (runs, wickets, catches, ...).
    pub stats: SportMatchStats,
}

impl TeamRecord {
    fn new(r: RecordCounts) -> Self {
        Self {
            played: r.played,
            won: r.won,
            drawn: r.drawn,
            lost: r.lost,
            result: side_result_from_record(&r.result),
            stats: match_stats_from_record(&r.stats),
        }
    }
}

/// Squad (in-house) games. The team plays itself, so there's no W/D/L.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct TeamSquadRecord {
    pub games: u64,
    /// Both sides' scorelines summed, so `goals_for` (or `runs_for`) is
    /// everything scored in these games.
    pub result: SportSideResult,
    /// See `TeamRecord::stats`: both sides' totals.
    pub stats: SportMatchStats,
}

/// Which overview the team gets.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum TeamOverviewLayout {
    /// Record card, form and last 10 first.
    Standard,
    /// For teams that mostly play among themselves: top performers,
    /// leaderboard and next match first, with no record card.
    Squad,
}

/// The other side in a team's match.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct TeamOpponent {
    /// The opposing team, if that side is a team.
    pub team_id: Option<String>,
    /// That side's display name, if it has one.
    pub name: Option<String>,
}

/// One confirmed result, from the team's side.
#[derive(Object)]
pub struct TeamResultItem {
    pub match_id: String,
    pub name: String,
    pub kind: TeamMatchKind,
    pub ranked: bool,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub tournament_id: Option<String>,
    /// The team's own side in this match.
    ///
    /// `confirmed_score` is keyed by side id throughout — `SimpleScore`'s
    /// and `SetsScore`'s `entries`, football's and netball's `score`,
    /// cricket's innings — and a match's side ids are scoped to that match,
    /// so there's nothing else here to match them against. Without this a
    /// client can see that a result was 3–2 but not which side of it the
    /// team was on, which makes the score unprintable rather than merely
    /// less convenient: `outcome` can orient a two-sided match, but
    /// `TeamOpponent` carries no side id either, so nothing identifies the
    /// team's own entry in the score.
    pub side_id: String,
    pub outcome: TeamOutcome,
    /// The match's score in its sport's own shape, the same as a feed
    /// card's `FeedMatch.confirmed_score`. Absent only if the match was
    /// deleted after the stats store last saw it.
    pub confirmed_score: Option<ConfirmedScore>,
    pub opponent: Option<TeamOpponent>,
}

/// The current run of identical results.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct TeamStreak {
    pub outcome: TeamOutcome,
    pub count: u32,
}

/// The team's next scheduled match.
#[derive(Object, Debug, Clone, PartialEq)]
pub struct TeamNextMatch {
    pub match_id: String,
    pub name: String,
    pub kind: TeamMatchKind,
    pub ranked: bool,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub tournament_id: Option<String>,
    /// Absent for squad games and for open slots.
    pub opponent: Option<TeamOpponent>,
}

/// `GET /teams/{team_id}/summary`.
#[derive(Object)]
pub struct TeamSummary {
    pub team_id: String,
    /// The sport these records are for — the one asked for, or the team's
    /// most-played when the request named none.
    pub sport: MatchType,
    /// Every sport this team has a match in, most matches first, for a sport
    /// switcher. Not affected by `sport`, and never empty when the team has
    /// any match at all.
    ///
    /// It lives here rather than on `Team` deliberately, and the reason is
    /// blast radius rather than cost. `GET /teams/:id` needs only DynamoDB
    /// today: `OPENSEARCH_URL` being unset or the cluster being down costs
    /// you the stats endpoints and nothing else. A sport list on `Team`
    /// would put an OpenSearch read behind opening a team at all — and
    /// behind `POST /teams/:id/members`, which has no business running an
    /// aggregation to answer an add-members call.
    ///
    /// This response already requires OpenSearch, so it is the one place
    /// the field is free. `years` a row below is the same pattern: the
    /// picker's options, returned with the thing being picked.
    ///
    /// The limit of that reasoning is worth stating, because it is where
    /// this stops being the right answer. Nothing here helps a *list* of
    /// teams — `TeamListItem`, what search and "my teams" actually return —
    /// show which sports each one plays. That genuinely would be a per-team
    /// fan-out, and the answer then isn't this field: it's the worker
    /// maintaining a sport set on the team record, the way it maintains
    /// `follower_count`.
    pub sports: Vec<MatchType>,
    /// The year the records cover, or absent for all time.
    pub year: Option<i32>,
    /// Every year the team has a confirmed match in this sport, newest
    /// first. Use it for a year picker. Not affected by `year`.
    pub years: Vec<i32>,
    pub layout: TeamOverviewLayout,
    /// League + cup + friendly: everything against other teams.
    pub record: TeamRecord,
    pub league: TeamRecord,
    pub cup: TeamRecord,
    pub friendly: TeamRecord,
    pub squad: TeamSquadRecord,
    /// The last 10 confirmed results against other teams, newest first,
    /// each with its sport's own score. Form is the first 5. All time, not
    /// affected by `year`.
    pub recent_results: Vec<TeamResultItem>,
    /// The current run of identical results. Absent before the first result.
    pub streak: Option<TeamStreak>,
    /// The next scheduled match, squad games included.
    pub next_match: Option<TeamNextMatch>,
}

/// Which of the team's matches a leaderboard counts.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum TeamLeaderboardScope {
    /// Every match for the team, squad games included.
    All,
    League,
    Cup,
    Friendly,
    Squad,
}

/// One player's totals for the team.
#[derive(Object)]
pub struct TeamLeaderboardRow {
    pub user_id: String,
    pub name: String,
    pub profile_image: Option<Photo>,
    /// False for a ringer: someone who played for the team but isn't a
    /// member (any more).
    pub is_member: bool,
    pub is_me: bool,
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
    /// `won / played`, from 5 appearances. Absent below that.
    pub win_rate: Option<f64>,
    /// The sport's own counters for this player, e.g. `stats.football`.
    pub stats: SportMatchStats,
}

/// `GET /teams/{team_id}/leaderboard`.
#[derive(Object)]
pub struct TeamLeaderboard {
    pub team_id: String,
    pub sport: MatchType,
    pub scope: TeamLeaderboardScope,
    pub tournament_id: Option<String>,
    pub year: Option<i32>,
    /// Most appearances first. Re-sort for other views; it's the whole slice.
    pub rows: Vec<TeamLeaderboardRow>,
}

#[derive(ApiResponse)]
enum TeamSummaryResponse {
    #[oai(status = 200)]
    Summary(Json<TeamSummary>),
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum TeamLeaderboardResponse {
    #[oai(status = 200)]
    Leaderboard(Json<TeamLeaderboard>),
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

// ── Mapping ─────────────────────────────────────────────────────────────────

fn opponent(o: Option<ts::Opponent>) -> Option<TeamOpponent> {
    o.map(|o| TeamOpponent {
        team_id: o.team_id,
        name: o.name,
    })
}

fn result_item(
    r: TeamResult,
    matches: &std::collections::HashMap<String, dao::records::MatchRecord>,
) -> TeamResultItem {
    let confirmed_score = matches
        .get(&r.match_id)
        .and_then(|m| m.confirmed_score.as_ref())
        .map(confirmed_score_from_record);
    TeamResultItem {
        kind: kind_from_str(&r.kind),
        outcome: outcome_from_str(&r.outcome),
        starts_at: parse_ts(&r.starts_at),
        match_id: r.match_id,
        name: r.name,
        ranked: r.ranked,
        tournament_id: r.tournament_id,
        side_id: r.side_id,
        confirmed_score,
        opponent: opponent(r.opponent),
    }
}

fn next_match_item(m: UpcomingMatch) -> TeamNextMatch {
    TeamNextMatch {
        kind: kind_from_str(&m.kind),
        starts_at: parse_ts(&m.starts_at),
        match_id: m.match_id,
        name: m.name,
        ranked: m.ranked,
        tournament_id: m.tournament_id,
        opponent: opponent(m.opponent),
    }
}

fn layout(ext_last_12_months: u64) -> TeamOverviewLayout {
    if ext_last_12_months >= STANDARD_LAYOUT_MIN_MATCHES {
        TeamOverviewLayout::Standard
    } else {
        TeamOverviewLayout::Squad
    }
}

fn win_rate(played: u64, won: u64) -> Option<f64> {
    (played >= WIN_RATE_MIN_APPS).then(|| won as f64 / played as f64)
}

fn scope_of(scope: TeamLeaderboardScope, tournament_id: Option<&str>) -> LeaderboardScope {
    if let Some(t) = tournament_id {
        return LeaderboardScope::Tournament(t.to_string());
    }
    match scope {
        TeamLeaderboardScope::All => LeaderboardScope::All,
        TeamLeaderboardScope::League => LeaderboardScope::Kind("league".into()),
        TeamLeaderboardScope::Cup => LeaderboardScope::Kind("cup".into()),
        TeamLeaderboardScope::Friendly => LeaderboardScope::Kind("friendly".into()),
        TeamLeaderboardScope::Squad => LeaderboardScope::Kind("squad".into()),
    }
}

pub(crate) fn store(store: &StatsStore) -> &OpenSearchClient {
    &store.0
}

pub(crate) fn stats_internal(err: agon_services_core::opensearch::OpenSearchError) -> poem::Error {
    error!("opensearch error: {err}");
    poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── Endpoints ───────────────────────────────────────────────────────────────

#[OpenApi]
impl TeamStatsApi {
    /// A team's record, form and next match in one sport.
    ///
    /// Counts confirmed matches only. `record` covers league, cup and
    /// friendly matches; squad (in-house) games are counted separately in
    /// `squad`. `year` limits the records to one year. `recent_results`,
    /// `streak` and `next_match` are always current.
    ///
    /// `sport` is optional because a client has no way to know what to ask
    /// for: nothing on `Team` says which sports it plays. Omit it and this
    /// answers for the team's most-played sport, with every sport it plays
    /// in `sports` — enough to render the page and its sport switcher from
    /// one call, with no guessing and no speculative request per sport.
    #[oai(path = "/teams/:team_id/summary", method = "get")]
    async fn get_team_summary(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(stats): Data<&StatsStore>,
        AuthSchema(jwt): AuthSchema,
        Path(team_id): Path<String>,
        /// The sport to summarise. Omit for the team's most-played.
        Query(sport): Query<Option<MatchType>>,
        /// Limit the records to this year. Omit for all time.
        Query(year): Query<Option<i32>>,
    ) -> Result<TeamSummaryResponse> {
        require_uid(dao, &jwt).await?;
        let os = store(stats);
        if dao
            .get_team_meta(&team_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(TeamSummaryResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        // `sports` is in the response either way, so the only question is
        // whether it can be read alongside the rest. It can when the caller
        // named a sport; when they didn't, its answer is what decides which
        // sport the other three queries are even about.
        let (sports, sport) = match sport {
            Some(s) => (None, s),
            None => {
                let sports = ts::team_sports(os, &team_id)
                    .await
                    .map_err(stats_internal)?;
                // A team with no matches has no sports to pick from. It still
                // gets a summary: every record is zero and `sports` is empty,
                // which is the honest answer and renders as a new team rather
                // than an error.
                let first = sports
                    .first()
                    .map_or(MatchType::Other, |t| crate::mapping::match_type_from_tag(t));
                (Some(sports), first)
            }
        };
        let tag = match_type_tag(&sport);
        let (records, recent, next, fetched_sports) = tokio::try_join!(
            ts::team_records(os, &team_id, tag, year, &now),
            ts::recent_results(os, &team_id, tag, RECENT_RESULTS),
            ts::next_match(os, &team_id, tag, &now),
            async {
                match &sports {
                    Some(_) => Ok(Vec::new()),
                    None => ts::team_sports(os, &team_id).await,
                }
            },
        )
        .map_err(stats_internal)?;
        let sports: Vec<MatchType> = sports
            .unwrap_or(fetched_sports)
            .iter()
            .map(|t| crate::mapping::match_type_from_tag(t))
            .collect();

        let streak = ts::streak(&recent).map(|(o, count)| TeamStreak {
            outcome: outcome_from_str(&o),
            count,
        });
        // The stats store has which matches and their outcomes; each
        // match's own score comes from the match itself, as on the feed.
        let match_ids: Vec<String> = recent.iter().map(|r| r.match_id.clone()).collect();
        let matches = dao
            .batch_get_match_metas(&match_ids)
            .await
            .map_err(dao_internal)?;
        let recent_results = recent
            .into_iter()
            .map(|r| result_item(r, &matches))
            .collect();
        Ok(TeamSummaryResponse::Summary(Json(TeamSummary {
            team_id,
            sport,
            sports,
            year,
            years: records.years,
            layout: layout(records.ext_last_12_months),
            record: TeamRecord::new(records.ext),
            league: TeamRecord::new(records.league),
            cup: TeamRecord::new(records.cup),
            friendly: TeamRecord::new(records.friendly),
            squad: TeamSquadRecord {
                games: records.squad.games,
                result: side_result_from_record(&records.squad.result),
                stats: match_stats_from_record(&records.squad.stats),
            },
            recent_results,
            streak,
            next_match: next.map(next_match_item),
        })))
    }

    /// Per-player totals for a team in one sport.
    ///
    /// Everyone who played for the team counts, members or not: a
    /// non-member is a ringer (`is_member: false`). `scope` picks the
    /// matches (squad games count in `all`); `tournament_id` narrows it to
    /// one league season or cup and overrides `scope`.
    #[oai(path = "/teams/:team_id/leaderboard", method = "get")]
    async fn get_team_leaderboard(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(stats): Data<&StatsStore>,
        AuthSchema(jwt): AuthSchema,
        Path(team_id): Path<String>,
        /// The sport.
        Query(sport): Query<MatchType>,
        /// Which matches count. Defaults to `all`.
        Query(scope): Query<Option<TeamLeaderboardScope>>,
        /// Only this tournament. Overrides `scope`.
        Query(tournament_id): Query<Option<String>>,
        /// Only this year. Omit for all time.
        Query(year): Query<Option<i32>>,
    ) -> Result<TeamLeaderboardResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let os = store(stats);
        if dao
            .get_team_meta(&team_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(TeamLeaderboardResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        }
        let scope = scope.unwrap_or(TeamLeaderboardScope::All);
        let rows = ts::leaderboard(
            os,
            &team_id,
            match_type_tag(&sport),
            &scope_of(scope, tournament_id.as_deref()),
            year,
        )
        .await
        .map_err(stats_internal)?;

        let user_ids: Vec<String> = rows.iter().map(|r| r.user_id.clone()).collect();
        let (users, members) = tokio::try_join!(
            async { dao.batch_get_users(&user_ids).await.map_err(dao_internal) },
            member_user_ids(dao, &team_id),
        )?;

        let rows = rows
            .into_iter()
            .map(|r| {
                let user = users.get(&r.user_id);
                TeamLeaderboardRow {
                    // A deleted account keeps its rows; show it without a name.
                    name: user.map(|u| u.name.clone()).unwrap_or_default(),
                    profile_image: user.and_then(|u| u.profile_image_url.clone()).map(
                        |image_url| Photo {
                            image_url,
                            asset_id: None,
                        },
                    ),
                    is_member: members.contains(&r.user_id),
                    is_me: r.user_id == uid,
                    win_rate: win_rate(r.played, r.won),
                    user_id: r.user_id,
                    played: r.played,
                    won: r.won,
                    drawn: r.drawn,
                    lost: r.lost,
                    stats: match_stats_from_record(&r.stats),
                }
            })
            .collect();
        Ok(TeamLeaderboardResponse::Leaderboard(Json(
            TeamLeaderboard {
                team_id,
                sport,
                scope,
                tournament_id,
                year,
                rows,
            },
        )))
    }
}

/// The user ids of the team's current members (accepted, with an account).
async fn member_user_ids(dao: &dao::Dao, team_id: &str) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = dao
            .list_team_members(team_id, cursor.as_deref(), MEMBER_PAGE)
            .await
            .map_err(dao_internal)?;
        out.extend(page.items.into_iter().filter_map(|m| m.user_id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cricket record carries cricket's scoreline and counters and
    /// nothing of football's (no goals, no clean sheets).
    #[test]
    fn a_record_carries_only_its_sports_scoreline_and_counters() {
        use agon_services_core::sports::cricket::{
            CricketMatchStatsRecord, CricketSideResultRecord,
        };
        use agon_services_core::sports::{MatchStatsRecord, SideResultRecord};

        let counts = RecordCounts {
            played: 4,
            won: 2,
            drawn: 1,
            lost: 1,
            result: SideResultRecord {
                cricket: Some(CricketSideResultRecord {
                    runs_for: 240,
                    wickets_lost: 30,
                    runs_against: 198,
                    wickets_taken: 36,
                }),
                ..Default::default()
            },
            stats: MatchStatsRecord {
                cricket: Some(CricketMatchStatsRecord {
                    wickets: 7,
                    ..Default::default()
                }),
                ..Default::default()
            },
        };

        let cricket = TeamRecord::new(counts);
        assert_eq!(cricket.result.football, None);
        assert_eq!(
            cricket
                .result
                .cricket
                .map(|c| (c.runs_for, c.wickets_taken)),
            Some((240, 36))
        );
        assert_eq!(cricket.stats.football, None);
        assert_eq!(cricket.stats.cricket.map(|c| c.wickets), Some(7));
    }

    #[test]
    fn layout_switches_at_three_external_matches() {
        assert_eq!(layout(0), TeamOverviewLayout::Squad);
        assert_eq!(layout(2), TeamOverviewLayout::Squad);
        assert_eq!(layout(3), TeamOverviewLayout::Standard);
    }

    #[test]
    fn win_rate_needs_five_apps() {
        assert_eq!(win_rate(4, 4), None);
        assert_eq!(win_rate(5, 2), Some(0.4));
    }

    #[test]
    fn tournament_overrides_scope() {
        assert_eq!(
            scope_of(TeamLeaderboardScope::Squad, Some("t1")),
            LeaderboardScope::Tournament("t1".into())
        );
        assert_eq!(
            scope_of(TeamLeaderboardScope::Friendly, None),
            LeaderboardScope::Kind("friendly".into())
        );
    }
}
