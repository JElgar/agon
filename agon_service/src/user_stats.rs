//! A player's own stats, read from OpenSearch: `GET /users/{user_id}/summary`.
//!
//! Mirrors `team_stats`'s summary endpoint, but there's no per-user
//! leaderboard (a leaderboard is inherently team-scoped) and no
//! goals-for/against or clean sheets (those are side-level, not personal).
//!
//! The data is only complete once the OpenSearch backfill has run against the
//! table (`docs/opensearch.md`). Before then this endpoint works but only
//! sees matches written since OpenSearch was deployed, so the UI shouldn't
//! switch to it until the backfill has been done.

use agon_services_core::dao;
use agon_services_core::opensearch::user_stats::{self as us, Opponent, UpcomingMatch, UserResult};
use poem::Result;
use poem::web::Data;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};

use crate::mapping::{dao_internal, match_type_tag, parse_ts};
use crate::team_stats::{StatsStore, stats_internal, store};
use crate::{AuthSchema, MatchType, require_uid};

pub struct UserStatsApi;

// ── Shapes ──────────────────────────────────────────────────────────────────

/// What kind of match this is, from the player's point of view.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum UserMatchKind {
    League,
    Cup,
    Friendly,
    /// The player's team against itself (an in-house game). Still a real
    /// personal result, unlike for the team.
    Squad,
    Pickup,
}

fn kind_from_str(s: &str) -> UserMatchKind {
    match s {
        "league" => UserMatchKind::League,
        "cup" => UserMatchKind::Cup,
        "squad" => UserMatchKind::Squad,
        "pickup" => UserMatchKind::Pickup,
        _ => UserMatchKind::Friendly,
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum UserOutcome {
    Won,
    Drawn,
    Lost,
}

fn outcome_from_str(s: &str) -> UserOutcome {
    match s {
        "won" => UserOutcome::Won,
        "lost" => UserOutcome::Lost,
        _ => UserOutcome::Drawn,
    }
}

/// A player's results over a set of matches.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
}

impl From<us::UserRecordCounts> for UserRecord {
    fn from(r: us::UserRecordCounts) -> Self {
        Self {
            played: r.played,
            won: r.won,
            drawn: r.drawn,
            lost: r.lost,
        }
    }
}

/// Which overview the player gets.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum UserOverviewLayout {
    /// Record card, form and last 10 first.
    Standard,
    /// For players who mostly play squad games: next match and recent form
    /// first, with no record card.
    Squad,
}

/// The other side in a player's match.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct UserOpponent {
    /// The opposing team, if that side is a team.
    pub team_id: Option<String>,
    /// That side's display name, if it has one.
    pub name: Option<String>,
}

fn opponent(o: Option<Opponent>) -> Option<UserOpponent> {
    o.map(|o| UserOpponent {
        team_id: o.team_id,
        name: o.name,
    })
}

/// One confirmed result, from the player's own side.
#[derive(Object, Debug, Clone, PartialEq)]
pub struct UserResultItem {
    pub match_id: String,
    pub kind: UserMatchKind,
    pub ranked: bool,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub tournament_id: Option<String>,
    pub outcome: UserOutcome,
    /// The opposing side's team(s), if any.
    pub opponent_team_ids: Vec<String>,
}

fn result_item(r: UserResult) -> UserResultItem {
    UserResultItem {
        kind: kind_from_str(&r.kind),
        outcome: outcome_from_str(&r.outcome),
        starts_at: parse_ts(&r.starts_at),
        match_id: r.match_id,
        ranked: r.ranked,
        tournament_id: r.tournament_id,
        opponent_team_ids: r.opponent_team_ids,
    }
}

/// The current run of identical results.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct UserStreak {
    pub outcome: UserOutcome,
    pub count: u32,
}

/// W/D/L over the last (up to) 10 confirmed results.
#[derive(Object, Debug, Clone, PartialEq, Eq)]
pub struct UserLastN {
    /// How many results this covers (10, or fewer for a new player).
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
}

fn last_n(results: &[UserResult]) -> UserLastN {
    let mut out = UserLastN {
        played: results.len() as u32,
        won: 0,
        drawn: 0,
        lost: 0,
    };
    for r in results {
        match r.outcome.as_str() {
            "won" => out.won += 1,
            "lost" => out.lost += 1,
            _ => out.drawn += 1,
        }
    }
    out
}

/// The player's next scheduled match.
#[derive(Object, Debug, Clone, PartialEq)]
pub struct UserNextMatch {
    pub match_id: String,
    pub name: String,
    pub kind: UserMatchKind,
    pub ranked: bool,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub tournament_id: Option<String>,
    /// Absent for squad games and for open slots.
    pub opponent: Option<UserOpponent>,
}

fn next_match_item(m: UpcomingMatch) -> UserNextMatch {
    UserNextMatch {
        kind: kind_from_str(&m.kind),
        starts_at: parse_ts(&m.starts_at),
        match_id: m.match_id,
        name: m.name,
        ranked: m.ranked,
        tournament_id: m.tournament_id,
        opponent: opponent(m.opponent),
    }
}

/// `GET /users/{user_id}/summary`.
#[derive(Object)]
pub struct UserSummary {
    pub user_id: String,
    pub sport: MatchType,
    /// The year the records cover, or absent for all time.
    pub year: Option<i32>,
    /// Every year the player has a confirmed match in this sport, newest
    /// first. Use it for a year picker. Not affected by `year`.
    pub years: Vec<i32>,
    pub layout: UserOverviewLayout,
    /// League + cup + friendly: everything against other teams.
    pub record: UserRecord,
    pub league: UserRecord,
    pub cup: UserRecord,
    pub friendly: UserRecord,
    /// Squad (in-house) games. Unlike a team's squad record, this carries a
    /// real W/D/L: the player was only ever on one of the two sides.
    pub squad: UserRecord,
    /// The last 5 confirmed results, newest first. All time, not affected by
    /// `year`.
    pub form: Vec<UserResultItem>,
    /// The current run of identical results. Absent before the first result.
    pub streak: Option<UserStreak>,
    pub last10: UserLastN,
    /// The next scheduled match, squad games included.
    pub next_match: Option<UserNextMatch>,
}

#[derive(ApiResponse)]
enum UserSummaryResponse {
    #[oai(status = 200)]
    Summary(Json<UserSummary>),
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

const FORM_LEN: usize = 5;
const LAST_N: u32 = 10;
const STANDARD_LAYOUT_MIN_MATCHES: u64 = 3;

fn layout(ext_last_12_months: u64) -> UserOverviewLayout {
    if ext_last_12_months >= STANDARD_LAYOUT_MIN_MATCHES {
        UserOverviewLayout::Standard
    } else {
        UserOverviewLayout::Squad
    }
}

// ── Endpoints ───────────────────────────────────────────────────────────────

#[OpenApi]
impl UserStatsApi {
    /// A player's own record, form and next match in one sport.
    ///
    /// `record` covers league, cup and friendly matches; squad (in-house)
    /// games are counted separately in `squad`, with their own W/D/L — unlike
    /// a team's squad record, a squad game has a real personal outcome for
    /// the player. `year` limits the records to one year. `form`, `streak`,
    /// `last10` and `next_match` are always current.
    #[oai(path = "/users/:user_id/summary", method = "get")]
    async fn get_user_summary(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(stats): Data<&StatsStore>,
        AuthSchema(jwt): AuthSchema,
        Path(user_id): Path<String>,
        /// The sport to summarise.
        Query(sport): Query<MatchType>,
        /// Limit the records to this year. Omit for all time.
        Query(year): Query<Option<i32>>,
    ) -> Result<UserSummaryResponse> {
        require_uid(dao, &jwt).await?;
        let os = store(stats)?;
        if dao
            .get_user(&user_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(UserSummaryResponse::NotFound(PlainText(
                "user not found".into(),
            )));
        }
        let tag = match_type_tag(&sport);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (records, recent, next) = tokio::try_join!(
            us::user_records(os, &user_id, tag, year, &now),
            us::recent_results(os, &user_id, tag, LAST_N),
            us::next_match(os, &user_id, tag, &now),
        )
        .map_err(stats_internal)?;

        let streak = us::streak(&recent).map(|(o, count)| UserStreak {
            outcome: outcome_from_str(&o),
            count,
        });
        let last10 = last_n(&recent);
        let form = recent.into_iter().take(FORM_LEN).map(result_item).collect();
        Ok(UserSummaryResponse::Summary(Json(UserSummary {
            user_id,
            sport,
            year,
            years: records.years,
            layout: layout(records.ext_last_12_months),
            record: records.ext.into(),
            league: records.league.into(),
            cup: records.cup.into(),
            friendly: records.friendly.into(),
            squad: records.squad.into(),
            form,
            streak,
            last10,
            next_match: next.map(next_match_item),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(outcome: &str) -> UserResult {
        UserResult {
            match_id: "m".into(),
            kind: "friendly".into(),
            ranked: false,
            starts_at: "2026-11-08T10:30:00Z".into(),
            tournament_id: None,
            outcome: outcome.into(),
            opponent_team_ids: vec![],
        }
    }

    #[test]
    fn layout_switches_at_three_external_matches() {
        assert_eq!(layout(0), UserOverviewLayout::Squad);
        assert_eq!(layout(2), UserOverviewLayout::Squad);
        assert_eq!(layout(3), UserOverviewLayout::Standard);
    }

    #[test]
    fn last_n_counts_outcomes() {
        let rs = [result("won"), result("lost"), result("won")];
        assert_eq!(
            last_n(&rs),
            UserLastN {
                played: 3,
                won: 2,
                drawn: 0,
                lost: 1
            }
        );
    }
}
