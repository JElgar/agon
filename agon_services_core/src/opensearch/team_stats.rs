//! Typed team-stats reads over the OpenSearch indices: a team's record per
//! match kind, its recent results and next match, and its per-player
//! leaderboard.
//!
//! Every query here is built by a pure `*_query` function and every response
//! is read by a pure `parse_*` function, so both halves are unit-tested
//! without a cluster. The `async` functions only glue them to
//! [`OpenSearchClient::search`]. Callers never see query JSON.
//!
//! **What counts.** Only confirmed matches count towards records and
//! leaderboards, matching lifetime stats. Squad games (both sides are this
//! team) have no W/D/L for the team, only games and the scoreline. They're
//! kept out of the external record (`ext` = league + cup + friendly) and
//! recent results, but they do count on the leaderboard (`all`), per the
//! in-house rule.
//!
//! **Sport-specific.** A side's scoreline (football goals for and against;
//! cricket runs and wickets) is read from `result.<sport>` and its counters
//! (football goals, assists, clean sheets; cricket runs, wickets, catches,
//! ...) from `stats.<sport>`, each with that sport's own fields into its own
//! type. A generic sport (tennis, ...) has neither, only W/D/L.

use serde::Deserialize;
use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::{MATCH_STATS, MATCHES};
use crate::sports::{MatchStatsRecord, SideResultRecord};

/// Upper bound on distinct players in one leaderboard slice. A team's roster
/// plus ringers over its whole history is far below this.
const MAX_LEADERBOARD_PLAYERS: u32 = 2000;

// ── Records ─────────────────────────────────────────────────────────────────

/// A team's results over some set of external (non-squad) matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordCounts {
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
    /// This team's scoreline summed across these matches (e.g. football
    /// goals for and against).
    pub result: SideResultRecord,
    /// The sport's own counters for this team's side, summed across these
    /// matches (e.g. football clean sheets, cricket wickets).
    pub stats: MatchStatsRecord,
}

impl RecordCounts {
    fn add(&mut self, o: &RecordCounts) {
        self.played += o.played;
        self.won += o.won;
        self.drawn += o.drawn;
        self.lost += o.lost;
        self.result.add(&o.result);
        self.stats.add(&o.stats);
    }
}

/// Squad games: the team plays itself, so there's no W/D/L, only how many
/// games and what was scored in them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SquadCounts {
    pub games: u64,
    /// Both sides' scorelines summed, so `goals_for` (or `runs_for`) is
    /// everything scored in these games.
    pub result: SideResultRecord,
    /// See [`RecordCounts::stats`]: both sides' totals.
    pub stats: MatchStatsRecord,
}

/// A team's record in one sport, per kind, optionally for one year.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamRecords {
    /// League + cup + friendly.
    pub ext: RecordCounts,
    pub league: RecordCounts,
    pub cup: RecordCounts,
    pub friendly: RecordCounts,
    pub squad: SquadCounts,
    /// Every year this team has a confirmed match in (this sport, any kind),
    /// newest first. Unaffected by the year filter, so a year picker can
    /// list them.
    pub years: Vec<i32>,
    /// External matches in the 12 months before `now`. Unaffected by the
    /// year filter. Drives the overview layout.
    pub ext_last_12_months: u64,
}

/// The `matches` query behind [`team_records`].
///
/// One request: the query selects this team's confirmed matches in the
/// sport; `years` and `recent_ext` ignore the year filter; `scoped` applies
/// it and splits by kind, then reads this team's side(s) through the nested
/// `sides`, summing `sport`'s own scoreline and counters.
pub fn team_records_query(team_id: &str, sport: &str, year: Option<i32>, now: &str) -> Value {
    let scoped_filter = match year {
        Some(y) => json!({ "term": { "year": y } }),
        None => json!({ "match_all": {} }),
    };
    let mut ours_aggs = serde_json::Map::new();
    ours_aggs.insert(
        "outcome".into(),
        json!({ "terms": { "field": "sides.outcome", "size": 5 } }),
    );
    for f in SideResultRecord::fields(sport) {
        ours_aggs.insert(
            format!("result:{f}"),
            json!({ "sum": { "field": format!("sides.result.{sport}.{f}") } }),
        );
    }
    for f in MatchStatsRecord::fields(sport) {
        ours_aggs.insert(
            format!("stat:{f}"),
            json!({ "sum": { "field": format!("sides.stats.{sport}.{f}") } }),
        );
    }
    json!({
        "size": 0,
        "track_total_hits": false,
        "query": { "bool": { "filter": [
            { "term": { "team_ids": team_id } },
            { "term": { "sport": sport } },
            { "term": { "confirmed": true } }
        ] } },
        "aggs": {
            "years": { "terms": { "field": "year", "size": 200, "order": { "_key": "desc" } } },
            "recent_ext": { "filter": { "bool": {
                "filter": [{ "range": { "starts_at": { "gte": format!("{now}||-365d"), "lte": now } } }],
                "must_not": [{ "term": { "kind": "squad" } }]
            } } },
            "scoped": {
                "filter": scoped_filter,
                "aggs": { "by_kind": {
                    "terms": { "field": "kind", "size": 10 },
                    "aggs": { "sides": {
                        "nested": { "path": "sides" },
                        "aggs": { "ours": {
                            "filter": { "term": { "sides.team_id": team_id } },
                            "aggs": ours_aggs
                        } }
                    } }
                } }
            }
        }
    })
}

/// `sport`'s counters from an aggregation's `stat:<field>` sums.
fn sport_stats(agg: &Value, sport: &str) -> MatchStatsRecord {
    MatchStatsRecord::from_sums(sport, |f| sum(&agg[format!("stat:{f}")]))
}

/// `sport`'s scoreline from an aggregation's `result:<field>` sums.
fn side_result(agg: &Value, sport: &str) -> SideResultRecord {
    SideResultRecord::from_sums(sport, |f| sum(&agg[format!("result:{f}")]))
}

/// Read a [`team_records_query`] response for `sport`.
pub fn parse_team_records(resp: &Value, sport: &str) -> TeamRecords {
    let aggs = &resp["aggregations"];
    let mut out = TeamRecords {
        years: buckets(&aggs["years"])
            .iter()
            .filter_map(|b| b["key"].as_i64())
            .map(|y| y as i32)
            .collect(),
        ext_last_12_months: count(&aggs["recent_ext"]["doc_count"]),
        ..Default::default()
    };
    for kind in buckets(&aggs["scoped"]["by_kind"]) {
        let ours = &kind["sides"]["ours"];
        match kind["key"].as_str() {
            Some("squad") => {
                out.squad = SquadCounts {
                    games: count(&kind["doc_count"]),
                    // Both sides are this team, so this sums both.
                    result: side_result(ours, sport),
                    stats: sport_stats(ours, sport),
                };
            }
            Some(k @ ("league" | "cup" | "friendly")) => {
                let mut rec = RecordCounts {
                    // One side per external match is ours.
                    played: count(&ours["doc_count"]),
                    result: side_result(ours, sport),
                    stats: sport_stats(ours, sport),
                    ..Default::default()
                };
                for o in buckets(&ours["outcome"]) {
                    let n = count(&o["doc_count"]);
                    match o["key"].as_str() {
                        Some("won") => rec.won = n,
                        Some("drawn") => rec.drawn = n,
                        Some("lost") => rec.lost = n,
                        _ => {}
                    }
                }
                out.ext.add(&rec);
                match k {
                    "league" => out.league = rec,
                    "cup" => out.cup = rec,
                    _ => out.friendly = rec,
                }
            }
            _ => {}
        }
    }
    out
}

/// A team's record in `sport`, per kind. `year` limits the per-kind counts to
/// one year; `years` and `ext_last_12_months` always cover everything.
pub async fn team_records(
    os: &OpenSearchClient,
    team_id: &str,
    sport: &str,
    year: Option<i32>,
    now: &str,
) -> OpenSearchResult<TeamRecords> {
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            team_records_query(team_id, sport, year, now),
        )
        .await?;
    Ok(parse_team_records(&resp, sport))
}

// ── Sports ──────────────────────────────────────────────────────────────────

/// The `matches` query behind [`team_sports`]: every sport this team has a
/// match in, busiest first.
///
/// Deliberately **not** filtered on `confirmed`. A team that has only ever
/// had matches scheduled still plays that sport, and a sport picker that
/// waited for the first agreed score would leave a new team with nothing to
/// pick. The records behind each sport are still confirmed-only; this only
/// decides what is on the menu.
pub fn team_sports_query(team_id: &str) -> Value {
    json!({
        "size": 0,
        "track_total_hits": false,
        "query": { "bool": { "filter": [{ "term": { "team_ids": team_id } }] } },
        "aggs": { "sports": { "terms": { "field": "sport", "size": 50 } } }
    })
}

/// Read a [`team_sports_query`] response: sport tags, most-played first.
pub fn parse_team_sports(resp: &Value) -> Vec<String> {
    buckets(&resp["aggregations"]["sports"])
        .iter()
        .filter_map(|b| b["key"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Every sport this team has played or has a match scheduled in, most
/// matches first. Empty for a team that has never been on a match side.
pub async fn team_sports(os: &OpenSearchClient, team_id: &str) -> OpenSearchResult<Vec<String>> {
    let resp = os
        .search(&MATCHES.alias(os.prefix()), team_sports_query(team_id))
        .await?;
    Ok(parse_team_sports(&resp))
}

// ── Recent results and next match ───────────────────────────────────────────

/// One side of a match as stored in `matches`.
#[derive(Debug, Clone, Deserialize)]
struct SideSource {
    side_id: String,
    team_id: Option<String>,
    name: Option<String>,
    outcome: Option<String>,
}

/// The `_source` fields the result and next-match queries read.
#[derive(Debug, Clone, Deserialize)]
struct MatchSource {
    match_id: String,
    name: String,
    kind: String,
    ranked: bool,
    starts_at: String,
    tournament_id: Option<String>,
    sides: Vec<SideSource>,
}

const MATCH_SOURCE_FIELDS: [&str; 7] = [
    "match_id",
    "name",
    "kind",
    "ranked",
    "starts_at",
    "tournament_id",
    "sides",
];

/// The other side in a team's match: its team (if any) and display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opponent {
    pub side_id: String,
    pub team_id: Option<String>,
    pub name: Option<String>,
}

/// One of a team's confirmed external results, from its side's view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamResult {
    pub match_id: String,
    pub name: String,
    pub kind: String,
    pub ranked: bool,
    pub starts_at: String,
    pub tournament_id: Option<String>,
    /// The team's own side in this match. Every score is keyed by side id,
    /// so without this the match's score can't be read from the team's
    /// point of view — see `TeamResultItem::side_id` in `agon_service`.
    pub side_id: String,
    /// `won`, `drawn` or `lost`.
    pub outcome: String,
    pub opponent: Option<Opponent>,
}

/// A team's next scheduled match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingMatch {
    pub match_id: String,
    pub name: String,
    pub kind: String,
    pub ranked: bool,
    pub starts_at: String,
    pub tournament_id: Option<String>,
    pub opponent: Option<Opponent>,
}

/// The `matches` query behind [`recent_results`]: confirmed, not squad,
/// newest first.
pub fn recent_results_query(team_id: &str, sport: &str, limit: u32) -> Value {
    json!({
        "size": limit,
        "_source": MATCH_SOURCE_FIELDS,
        "query": { "bool": {
            "filter": [
                { "term": { "team_ids": team_id } },
                { "term": { "sport": sport } },
                { "term": { "confirmed": true } }
            ],
            "must_not": [{ "term": { "kind": "squad" } }]
        } },
        "sort": [{ "starts_at": "desc" }, { "match_id": "desc" }]
    })
}

/// The `matches` query behind [`next_match`]: scheduled, from `now`, soonest
/// first. Squad games count: they're still the team's next fixture.
pub fn next_match_query(team_id: &str, sport: &str, now: &str) -> Value {
    json!({
        "size": 1,
        "_source": MATCH_SOURCE_FIELDS,
        "query": { "bool": { "filter": [
            { "term": { "team_ids": team_id } },
            { "term": { "sport": sport } },
            { "term": { "status": "scheduled" } },
            { "range": { "starts_at": { "gte": now } } }
        ] } },
        "sort": [{ "starts_at": "asc" }, { "match_id": "asc" }]
    })
}

fn hits(resp: &Value) -> Vec<MatchSource> {
    resp["hits"]["hits"]
        .as_array()
        .map(|hs| {
            hs.iter()
                .filter_map(|h| serde_json::from_value(h["_source"].clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// This team's side and the first other side, in a non-squad match.
fn ours_and_theirs<'a>(
    m: &'a MatchSource,
    team_id: &str,
) -> Option<(&'a SideSource, Option<Opponent>)> {
    let ours = m
        .sides
        .iter()
        .find(|s| s.team_id.as_deref() == Some(team_id))?;
    let opponent = m
        .sides
        .iter()
        .find(|s| s.side_id != ours.side_id)
        .map(|s| Opponent {
            side_id: s.side_id.clone(),
            team_id: s.team_id.clone(),
            name: s.name.clone(),
        });
    Some((ours, opponent))
}

/// Read a [`recent_results_query`] response, from `team_id`'s side.
pub fn parse_recent_results(resp: &Value, team_id: &str) -> Vec<TeamResult> {
    hits(resp)
        .iter()
        .filter_map(|m| {
            let (ours, opponent) = ours_and_theirs(m, team_id)?;
            Some(TeamResult {
                match_id: m.match_id.clone(),
                name: m.name.clone(),
                kind: m.kind.clone(),
                ranked: m.ranked,
                starts_at: m.starts_at.clone(),
                tournament_id: m.tournament_id.clone(),
                side_id: ours.side_id.clone(),
                outcome: ours.outcome.clone()?,
                opponent,
            })
        })
        .collect()
}

/// Read a [`next_match_query`] response.
pub fn parse_next_match(resp: &Value, team_id: &str) -> Option<UpcomingMatch> {
    let m = hits(resp).into_iter().next()?;
    // In a squad game the "opponent" is the team itself; leave it out.
    let opponent = ours_and_theirs(&m, team_id)
        .and_then(|(_, o)| o)
        .filter(|o| o.team_id.as_deref() != Some(team_id));
    Some(UpcomingMatch {
        match_id: m.match_id,
        name: m.name,
        kind: m.kind,
        ranked: m.ranked,
        starts_at: m.starts_at,
        tournament_id: m.tournament_id,
        opponent,
    })
}

/// The team's last `limit` confirmed external results, newest first.
pub async fn recent_results(
    os: &OpenSearchClient,
    team_id: &str,
    sport: &str,
    limit: u32,
) -> OpenSearchResult<Vec<TeamResult>> {
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            recent_results_query(team_id, sport, limit),
        )
        .await?;
    Ok(parse_recent_results(&resp, team_id))
}

/// The team's next scheduled match from `now`, if any.
pub async fn next_match(
    os: &OpenSearchClient,
    team_id: &str,
    sport: &str,
    now: &str,
) -> OpenSearchResult<Option<UpcomingMatch>> {
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            next_match_query(team_id, sport, now),
        )
        .await?;
    Ok(parse_next_match(&resp, team_id))
}

/// The current run of identical outcomes, newest first (`results` must be
/// newest first). `None` with no results.
pub fn streak(results: &[TeamResult]) -> Option<(String, u32)> {
    let first = results.first()?;
    let n = results
        .iter()
        .take_while(|r| r.outcome == first.outcome)
        .count();
    Some((first.outcome.clone(), n as u32))
}

// ── Leaderboard ─────────────────────────────────────────────────────────────

/// Which of a team's matches a leaderboard counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderboardScope {
    /// Every kind, squad games included.
    All,
    /// One kind: `league`, `cup`, `friendly` or `squad`.
    Kind(String),
    /// One tournament (a league season or a cup).
    Tournament(String),
}

/// One player's totals for this team in a slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeaderboardRow {
    pub user_id: String,
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
    /// The sport's own counters, summed (e.g. football goals and assists).
    pub stats: MatchStatsRecord,
}

/// The `match_stats` query behind [`leaderboard`], summing W/D/L and
/// `sport`'s own counters per player.
pub fn leaderboard_query(
    team_id: &str,
    sport: &str,
    scope: &LeaderboardScope,
    year: Option<i32>,
) -> Value {
    let mut filter = vec![
        json!({ "term": { "team_id": team_id } }),
        json!({ "term": { "sport": sport } }),
    ];
    match scope {
        LeaderboardScope::All => {}
        LeaderboardScope::Kind(k) => filter.push(json!({ "term": { "kind": k } })),
        LeaderboardScope::Tournament(t) => filter.push(json!({ "term": { "tournament_id": t } })),
    }
    if let Some(y) = year {
        filter.push(json!({ "term": { "year": y } }));
    }
    let mut sums = serde_json::Map::new();
    for f in ["played", "won", "drawn", "lost"] {
        sums.insert(f.into(), json!({ "sum": { "field": f } }));
    }
    for f in MatchStatsRecord::fields(sport) {
        sums.insert(
            format!("stat:{f}"),
            json!({ "sum": { "field": format!("stats.{sport}.{f}") } }),
        );
    }
    json!({
        "size": 0,
        "track_total_hits": false,
        "query": { "bool": { "filter": filter } },
        "aggs": { "players": {
            "terms": { "field": "user_id", "size": MAX_LEADERBOARD_PLAYERS },
            "aggs": sums
        } }
    })
}

/// Read a [`leaderboard_query`] response. Rows come back most games first;
/// callers re-sort for their view.
pub fn parse_leaderboard(resp: &Value, sport: &str) -> Vec<LeaderboardRow> {
    let mut rows: Vec<LeaderboardRow> = buckets(&resp["aggregations"]["players"])
        .iter()
        .filter_map(|b| {
            let user_id = b["key"].as_str()?.to_string();
            let stats = sport_stats(b, sport);
            Some(LeaderboardRow {
                user_id,
                played: sum(&b["played"]),
                won: sum(&b["won"]),
                drawn: sum(&b["drawn"]),
                lost: sum(&b["lost"]),
                stats,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.played
            .cmp(&a.played)
            .then(b.won.cmp(&a.won))
            .then(a.user_id.cmp(&b.user_id))
    });
    rows
}

/// Per-player totals for `team_id` in `sport` over `scope` (and `year`).
pub async fn leaderboard(
    os: &OpenSearchClient,
    team_id: &str,
    sport: &str,
    scope: &LeaderboardScope,
    year: Option<i32>,
) -> OpenSearchResult<Vec<LeaderboardRow>> {
    let resp = os
        .search(
            &MATCH_STATS.alias(os.prefix()),
            leaderboard_query(team_id, sport, scope, year),
        )
        .await?;
    Ok(parse_leaderboard(&resp, sport))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn buckets(agg: &Value) -> Vec<Value> {
    agg["buckets"].as_array().cloned().unwrap_or_default()
}

fn count(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// A `sum` aggregation's value. OpenSearch returns sums as floats.
fn sum(agg: &Value) -> u64 {
    agg["value"].as_f64().map(|f| f.round() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::cricket::{CricketMatchStatsRecord, CricketSideResultRecord};
    use crate::sports::football::{FootballMatchStatsRecord, FootballSideResultRecord};

    fn kind_bucket(kind: &str, docs: u64, ours: Value) -> Value {
        json!({ "key": kind, "doc_count": docs, "sides": { "doc_count": docs * 2, "ours": ours } })
    }

    fn ours(
        n: u64,
        w: u64,
        d: u64,
        l: u64,
        result: &[(&str, f64)],
        stats: &[(&str, f64)],
    ) -> Value {
        let mut outcomes = Vec::new();
        for (k, c) in [("won", w), ("drawn", d), ("lost", l)] {
            if c > 0 {
                outcomes.push(json!({ "key": k, "doc_count": c }));
            }
        }
        let mut v = json!({
            "doc_count": n,
            "outcome": { "buckets": outcomes }
        });
        for (k, s) in result {
            v[format!("result:{k}")] = json!({ "value": s });
        }
        for (k, s) in stats {
            v[format!("stat:{k}")] = json!({ "value": s });
        }
        v
    }

    #[test]
    fn records_split_by_kind_and_sum_into_ext() {
        let resp = json!({ "aggregations": {
            "years": { "buckets": [{ "key": 2026, "doc_count": 9 }, { "key": 2025, "doc_count": 3 }] },
            "recent_ext": { "doc_count": 7 },
            "scoped": { "doc_count": 12, "by_kind": { "buckets": [
                kind_bucket("league", 5, ours(5, 3, 1, 1,
                    &[("goals_for", 9.0), ("goals_against", 4.0)],
                    &[("goals", 9.0), ("assists", 0.0), ("clean_sheets", 2.0)])),
                kind_bucket("friendly", 3, ours(3, 1, 0, 2,
                    &[("goals_for", 4.0), ("goals_against", 6.0)],
                    &[("goals", 4.0), ("assists", 3.0), ("clean_sheets", 0.0)])),
                // Squad: both sides are ours, so goals_for covers both.
                kind_bucket("squad", 4, json!({ "doc_count": 8, "result:goals_for": { "value": 21.0 }, "stat:goals": { "value": 21.0 } })),
            ] } }
        } });
        let r = parse_team_records(&resp, "football");
        assert_eq!(r.years, vec![2026, 2025]);
        assert_eq!(r.ext_last_12_months, 7);
        assert_eq!(
            r.league,
            RecordCounts {
                played: 5,
                won: 3,
                drawn: 1,
                lost: 1,
                result: SideResultRecord {
                    football: Some(FootballSideResultRecord {
                        goals_for: 9,
                        goals_against: 4,
                    }),
                    ..Default::default()
                },
                stats: MatchStatsRecord {
                    football: Some(FootballMatchStatsRecord {
                        goals: 9,
                        assists: 0,
                        clean_sheets: 2,
                    }),
                    ..Default::default()
                },
            }
        );
        assert_eq!(r.cup, RecordCounts::default());
        assert_eq!(r.ext.played, 8);
        assert_eq!(r.ext.won, 4);
        assert_eq!(r.ext.result.football.map(|f| f.goals_against), Some(10));
        // Both kinds' counters sum into the external record.
        assert_eq!(
            r.ext.stats.football,
            Some(FootballMatchStatsRecord {
                goals: 13,
                assists: 3,
                clean_sheets: 2,
            })
        );
        assert_eq!(r.squad.games, 4);
        assert_eq!(r.squad.result.football.map(|f| f.goals_for), Some(21));
        assert_eq!(r.squad.stats.football.map(|f| f.goals), Some(21));
    }

    #[test]
    fn cricket_records_read_cricket_counters() {
        let q = team_records_query("t1", "cricket", None, "2026-11-15T00:00:00Z");
        let aggs = &q["aggs"]["scoped"]["aggs"]["by_kind"]["aggs"]["sides"]["aggs"]["ours"]["aggs"];
        assert_eq!(
            aggs["stat:wickets"],
            json!({ "sum": { "field": "sides.stats.cricket.wickets" } })
        );
        assert_eq!(
            aggs["result:wickets_taken"],
            json!({ "sum": { "field": "sides.result.cricket.wickets_taken" } })
        );
        assert!(aggs.get("stat:goals").is_none(), "no football counters");
        assert!(
            aggs.get("result:goals_for").is_none(),
            "no football scoreline"
        );

        let resp = json!({ "aggregations": {
            "years": { "buckets": [] },
            "recent_ext": { "doc_count": 1 },
            "scoped": { "doc_count": 1, "by_kind": { "buckets": [
                kind_bucket("friendly", 1, ours(1, 1, 0, 0,
                    &[("runs_for", 180.0), ("wickets_lost", 6.0), ("runs_against", 140.0), ("wickets_taken", 10.0)],
                    &[("runs", 170.0), ("wickets", 9.0), ("catches", 4.0)])),
            ] } }
        } });
        let r = parse_team_records(&resp, "cricket");
        assert_eq!(
            r.friendly.result.cricket,
            Some(CricketSideResultRecord {
                runs_for: 180,
                wickets_lost: 6,
                runs_against: 140,
                wickets_taken: 10,
            })
        );
        assert_eq!(r.friendly.stats.football, None);
        assert_eq!(
            r.friendly.stats.cricket,
            Some(CricketMatchStatsRecord {
                runs: 170,
                wickets: 9,
                catches: 4,
                ..Default::default()
            })
        );
    }

    #[test]
    fn a_generic_sport_has_no_counters_to_read() {
        let q = team_records_query("t1", "tennis", None, "2026-11-15T00:00:00Z");
        let ours = q["aggs"]["scoped"]["aggs"]["by_kind"]["aggs"]["sides"]["aggs"]["ours"]["aggs"]
            .as_object()
            .unwrap();
        assert!(
            ours.keys()
                .all(|k| !k.starts_with("stat:") && !k.starts_with("result:"))
        );
    }

    #[test]
    fn records_query_applies_year_only_to_the_scoped_counts() {
        let q = team_records_query("t1", "football", Some(2025), "2026-11-15T00:00:00Z");
        assert_eq!(
            q["aggs"]["scoped"]["filter"],
            json!({ "term": { "year": 2025 } })
        );
        let top = q["query"]["bool"]["filter"].to_string();
        assert!(
            !top.contains("year"),
            "the year must not filter the whole query"
        );
        assert_eq!(
            q["aggs"]["scoped"]["aggs"]["by_kind"]["aggs"]["sides"]["aggs"]["ours"]["aggs"]["stat:goals"],
            json!({ "sum": { "field": "sides.stats.football.goals" } })
        );
        let q = team_records_query("t1", "football", None, "2026-11-15T00:00:00Z");
        assert_eq!(q["aggs"]["scoped"]["filter"], json!({ "match_all": {} }));
    }

    /// The sport list is ordered by how much the team plays each one, and
    /// is not confirmed-only: a team whose only football match is next
    /// Tuesday still plays football, and a sport switcher that waited for a
    /// first agreed score would be empty for a brand-new team.
    #[test]
    fn the_sports_query_is_busiest_first_and_counts_unplayed_matches() {
        let q = team_sports_query("t1");
        assert_eq!(
            q["query"]["bool"]["filter"],
            json!([{ "term": { "team_ids": "t1" } }])
        );
        assert!(
            !q.to_string().contains("confirmed"),
            "a scheduled match still says which sport the team plays"
        );
        // A terms agg is already ordered by doc count, descending.
        assert_eq!(q["aggs"]["sports"]["terms"]["field"], "sport");

        let resp = json!({ "aggregations": { "sports": { "buckets": [
            { "key": "football", "doc_count": 12 },
            { "key": "cricket", "doc_count": 3 }
        ] } } });
        assert_eq!(parse_team_sports(&resp), vec!["football", "cricket"]);
    }

    /// A team nobody has ever put in a match has no sports, and that is an
    /// answer rather than an error — the caller renders a new team.
    #[test]
    fn a_team_with_no_matches_has_no_sports() {
        let resp = json!({ "aggregations": { "sports": { "buckets": [] } } });
        assert!(parse_team_sports(&resp).is_empty());
    }

    fn match_hit(id: &str, kind: &str, sides: Value) -> Value {
        json!({ "_source": {
            "match_id": id, "name": id, "kind": kind, "ranked": false,
            "starts_at": "2026-11-08T10:30:00Z", "sides": sides
        } })
    }

    #[test]
    fn results_are_read_from_our_side() {
        let resp = json!({ "hits": { "hits": [
            match_hit("m2", "friendly", json!([
                { "side_id": "a", "team_id": "leyton", "name": "Leyton", "outcome": "won" },
                { "side_id": "b", "team_id": "hw", "outcome": "lost" }
            ])),
            match_hit("m1", "league", json!([
                { "side_id": "a", "team_id": "hw", "outcome": "lost" },
                { "side_id": "b", "name": "Pickup XI", "outcome": "won" }
            ])),
            match_hit("m0", "league", json!([
                { "side_id": "a", "team_id": "hw", "outcome": "won" },
                { "side_id": "b", "team_id": "x", "outcome": "lost" }
            ]))
        ] } });
        let rs = parse_recent_results(&resp, "hw");
        assert_eq!(rs.len(), 3);
        assert_eq!(rs[0].outcome, "lost");
        assert_eq!(
            rs[0].opponent.as_ref().unwrap().name.as_deref(),
            Some("Leyton")
        );
        assert_eq!(rs[1].opponent.as_ref().unwrap().team_id, None);
        // Our side isn't always the first one listed: "hw" is side b in m2
        // and side a in the two below it.
        assert_eq!(rs[0].side_id, "b");
        assert_eq!(rs[1].side_id, "a");
        assert_eq!(rs[2].side_id, "a");
        assert_eq!(streak(&rs), Some(("lost".into(), 2)));
        assert_eq!(streak(&[]), None);
    }

    #[test]
    fn next_squad_game_has_no_opponent() {
        let resp = json!({ "hits": { "hits": [match_hit("m9", "squad", json!([
            { "side_id": "l", "team_id": "hw" },
            { "side_id": "d", "team_id": "hw" }
        ]))] } });
        let m = parse_next_match(&resp, "hw").unwrap();
        assert_eq!(m.kind, "squad");
        assert_eq!(m.opponent, None);
        assert_eq!(
            parse_next_match(&json!({ "hits": { "hits": [] } }), "hw"),
            None
        );
    }

    #[test]
    fn leaderboard_query_filters_scope_and_sums_named_stats() {
        let q = leaderboard_query(
            "hw",
            "football",
            &LeaderboardScope::Tournament("tour1".into()),
            Some(2026),
        );
        let filters = q["query"]["bool"]["filter"].to_string();
        assert!(filters.contains(r#""tournament_id":"tour1""#));
        assert!(filters.contains(r#""year":2026"#));
        assert_eq!(
            q["aggs"]["players"]["aggs"]["stat:goals"],
            json!({ "sum": { "field": "stats.football.goals" } })
        );
        let q = leaderboard_query("hw", "football", &LeaderboardScope::All, None);
        assert_eq!(q["query"]["bool"]["filter"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn leaderboard_rows_read_the_sports_counters_and_sort_by_games() {
        let resp = json!({ "aggregations": { "players": { "buckets": [
            { "key": "u2", "doc_count": 3, "played": { "value": 3.0 }, "won": { "value": 1.0 },
              "drawn": { "value": 0.0 }, "lost": { "value": 2.0 },
              "stat:goals": { "value": 0.0 }, "stat:assists": { "value": 2.0 } },
            { "key": "u1", "doc_count": 5, "played": { "value": 5.0 }, "won": { "value": 4.0 },
              "drawn": { "value": 1.0 }, "lost": { "value": 0.0 },
              "stat:goals": { "value": 6.0 }, "stat:assists": { "value": 0.0 } }
        ] } } });
        let rows = parse_leaderboard(&resp, "football");
        assert_eq!(rows[0].user_id, "u1");
        assert_eq!(rows[0].stats.football.as_ref().map(|f| f.goals), Some(6));
        assert_eq!(rows[1].stats.football.as_ref().map(|f| f.assists), Some(2));
        assert_eq!(rows[1].lost, 2);
    }
}
