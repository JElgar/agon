//! `GET /teams/{team_id}/competitions`: the competition boxes on a team's
//! page — its leagues and cups (live ones first, with the team's current
//! league position), then Friendlies and Squad games summaries.
//!
//! Tournaments come from DynamoDB: a team's entries are projected into GSI1
//! as `TEAMTOURNS#<teamId>` (`Dao::list_team_tournament_entries`). A squad
//! team of a teamless tournament has no entry — it is created at `start` —
//! so its one tournament is read from `TeamRecord::tournament_id` instead.
//!
//! League standings are computed on read with the same code as
//! `GET /tournaments/{id}/standings` (`agon_services_core::standings`).
//!
//! The Friendlies and Squad games summaries come from OpenSearch
//! (`team_stats::team_records`). They're best-effort: if OpenSearch isn't
//! configured or the query fails, those two items are left out and the
//! tournaments are still returned.

use std::collections::HashSet;

use agon_services_core::dao::{self, records::TournamentRecord};
use agon_services_core::opensearch::team_stats as ts;
use agon_services_core::standings::{league_standings, position_of};
use poem::Result;
use poem::web::Data;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Object, OpenApi, Union};
use tracing::warn;

use crate::mapping::{
    dao_internal, match_type_from_tag, match_type_tag, tournament_format_from_record,
};
use crate::team_stats::StatsStore;
use crate::{AuthSchema, MatchType, TournamentFormat, require_uid};

/// How many competition boxes the team page shows.
const MAX_ITEMS: usize = 4;

pub struct TeamCompetitionsApi;

/// A league or cup the team is in.
#[derive(Object)]
pub struct TeamCompetitionTournament {
    pub tournament_id: String,
    pub name: String,
    /// `league` or `knockout` (a cup).
    pub format: TournamentFormat,
    /// `"open"` | `"in_progress"` | `"completed"` (cancelled tournaments
    /// aren't listed).
    pub status: String,
    pub sport: MatchType,
    /// How many entrants the tournament has, e.g. for "3rd of 8".
    pub entry_count: u64,
    /// League only: the team's 1-based position in the standings. Absent
    /// for a cup, and for a league before the team's first confirmed match.
    pub position: Option<u32>,
    /// League only: the team's points so far (0 before its first match).
    pub points: Option<u32>,
    /// League only: confirmed matches the team has played.
    pub played: Option<u32>,
}

/// The team's record in friendlies (matches against another team outside a
/// tournament), in `sport`.
#[derive(Object)]
pub struct TeamCompetitionFriendlies {
    pub sport: MatchType,
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
}

/// The team's squad (in-house) games in `sport`. No W/D/L: the team plays
/// itself.
#[derive(Object)]
pub struct TeamCompetitionSquadGames {
    pub sport: MatchType,
    pub games: u64,
}

/// One competition box.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum TeamCompetition {
    #[oai(mapping = "tournament")]
    Tournament(TeamCompetitionTournament),
    #[oai(mapping = "friendlies")]
    Friendlies(TeamCompetitionFriendlies),
    #[oai(mapping = "squad_games")]
    SquadGames(TeamCompetitionSquadGames),
}

/// `GET /teams/{team_id}/competitions`.
#[derive(Object)]
pub struct TeamCompetitions {
    /// At most 4: tournaments first (in progress, then open, then
    /// completed; newest entry first within each), then Friendlies, then
    /// Squad games.
    pub items: Vec<TeamCompetition>,
    /// How many competitions the team has in all, including any not in
    /// `items` — for a "see all (N)" link.
    pub total: u32,
}

#[derive(ApiResponse)]
enum TeamCompetitionsResponse {
    #[oai(status = 200)]
    Competitions(Json<TeamCompetitions>),
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

/// Sort rank of a listed tournament status; `None` = not listed.
fn status_rank(status: &str) -> Option<u8> {
    match status {
        "in_progress" => Some(0),
        "open" => Some(1),
        "completed" => Some(2),
        _ => None,
    }
}

/// Order the team's tournaments for display, dropping unlisted statuses.
/// `tournaments` comes in newest-entry-first; that order is kept within
/// each status (the sort is stable).
fn order_tournaments(mut tournaments: Vec<TournamentRecord>) -> Vec<TournamentRecord> {
    tournaments.retain(|t| status_rank(&t.status).is_some());
    tournaments.sort_by_key(|t| status_rank(&t.status));
    tournaments
}

/// The Friendlies and Squad games items, or nothing on any OpenSearch
/// problem. `sport` defaults to the team's most-played.
async fn summary_items(
    stats: &StatsStore,
    team_id: &str,
    sport: Option<MatchType>,
) -> Vec<TeamCompetition> {
    let Some(os) = stats.0.as_ref() else {
        return Vec::new();
    };
    let sport = match sport {
        Some(s) => s,
        None => match ts::team_sports(os, team_id).await {
            Ok(sports) => match sports.first() {
                Some(t) => match_type_from_tag(t),
                // Never on a match side: nothing to summarise.
                None => return Vec::new(),
            },
            Err(e) => {
                warn!("competitions: team_sports failed for {team_id}: {e}");
                return Vec::new();
            }
        },
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let records = match ts::team_records(os, team_id, match_type_tag(&sport), None, &now).await {
        Ok(r) => r,
        Err(e) => {
            warn!("competitions: team_records failed for {team_id}: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    if records.friendly.played > 0 {
        out.push(TeamCompetition::Friendlies(TeamCompetitionFriendlies {
            // `MatchType` isn't `Clone`; round-trip it through its tag.
            sport: match_type_from_tag(match_type_tag(&sport)),
            played: records.friendly.played,
            won: records.friendly.won,
            drawn: records.friendly.drawn,
            lost: records.friendly.lost,
        }));
    }
    if records.squad.games > 0 {
        out.push(TeamCompetition::SquadGames(TeamCompetitionSquadGames {
            sport,
            games: records.squad.games,
        }));
    }
    out
}

/// One tournament box, with the team's league standing computed on read.
async fn tournament_item(
    dao: &dao::Dao,
    team_id: &str,
    t: &TournamentRecord,
) -> Result<TeamCompetitionTournament> {
    let (position, points, played) = match t.format {
        dao::records::TournamentFormatRecord::League => {
            let matches = dao
                .list_all_tournament_matches(&t.id)
                .await
                .map_err(dao_internal)?;
            let rows = league_standings(t, &matches);
            match position_of(&rows, team_id) {
                Some((pos, row)) => (Some(pos), Some(row.points), Some(row.played)),
                None => (None, Some(0), Some(0)),
            }
        }
        dao::records::TournamentFormatRecord::Knockout => (None, None, None),
    };
    Ok(TeamCompetitionTournament {
        tournament_id: t.id.clone(),
        name: t.name.clone(),
        format: tournament_format_from_record(t.format),
        status: t.status.clone(),
        sport: match_type_from_tag(&t.match_type),
        entry_count: t.entry_count,
        position,
        points,
        played,
    })
}

#[OpenApi]
impl TeamCompetitionsApi {
    /// The competitions a team plays in, for the team page's competition
    /// boxes: its leagues and cups (in progress first, then open, then
    /// completed), then a Friendlies and a Squad games summary if it has
    /// played any. At most 4 items; `total` counts them all.
    ///
    /// Only accepted entries count (a pending request isn't listed), and
    /// cancelled tournaments are left out. A league item carries the
    /// team's current position, points and games played. The Friendlies
    /// and Squad games items are for `sport` (default: the team's
    /// most-played) and are omitted if the stats store is unavailable.
    #[oai(path = "/teams/:team_id/competitions", method = "get")]
    async fn get_team_competitions(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(stats): Data<&StatsStore>,
        AuthSchema(jwt): AuthSchema,
        Path(team_id): Path<String>,
        /// Sport for the Friendlies and Squad games summaries. Omit for the
        /// team's most-played.
        Query(sport): Query<Option<MatchType>>,
    ) -> Result<TeamCompetitionsResponse> {
        require_uid(dao, &jwt).await?;
        let Some(team) = dao.get_team_meta(&team_id).await.map_err(dao_internal)? else {
            return Ok(TeamCompetitionsResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        };

        let tournaments = async {
            // A squad team plays only in the tournament that made it, and
            // has no entry there (squads are created at `start`).
            let ids: Vec<String> = match &team.tournament_id {
                Some(tid) => vec![tid.clone()],
                None => {
                    let mut seen = HashSet::new();
                    dao.list_all_team_tournament_entries(&team_id)
                        .await
                        .map_err(dao_internal)?
                        .into_iter()
                        .filter(|e| e.status == "accepted")
                        .map(|e| e.tournament_id)
                        .filter(|id| seen.insert(id.clone()))
                        .collect()
                }
            };
            let by_id = dao
                .batch_get_tournaments(&ids)
                .await
                .map_err(dao_internal)?;
            Ok::<_, poem::Error>(order_tournaments(
                ids.iter().filter_map(|id| by_id.get(id).cloned()).collect(),
            ))
        };
        let (tournaments, summaries) =
            tokio::join!(tournaments, summary_items(stats, &team_id, sport));
        let tournaments = tournaments?;

        let total = (tournaments.len() + summaries.len()) as u32;
        let mut items = Vec::new();
        for t in tournaments.iter().take(MAX_ITEMS) {
            items.push(TeamCompetition::Tournament(
                tournament_item(dao, &team_id, t).await?,
            ));
        }
        items.extend(summaries.into_iter().take(MAX_ITEMS - items.len()));

        Ok(TeamCompetitionsResponse::Competitions(Json(
            TeamCompetitions { items, total },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_rank_orders_live_first_and_drops_cancelled() {
        assert!(status_rank("in_progress") < status_rank("open"));
        assert!(status_rank("open") < status_rank("completed"));
        assert_eq!(status_rank("cancelled"), None);
        let mut v: Vec<&str> = ["completed", "cancelled", "open", "in_progress"]
            .into_iter()
            .filter(|s| status_rank(s).is_some())
            .collect();
        v.sort_by_key(|s| status_rank(s));
        assert_eq!(v, ["in_progress", "open", "completed"]);
    }
}
