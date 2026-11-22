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
//! team) have no W/D/L for the team, only games and points. They're kept out
//! of the external record (`ext` = league + cup + friendly), form and last 10,
//! but they do count on the leaderboard (`all`), per the in-house rule.
//!
//! **Points, not goals.** A side's stored `score` is whatever that sport
//! counts — goals in football and netball, runs in cricket, points or sets
//! elsewhere (`super::docs::side_points`). The fields here are named for
//! that, so a cricket team's record doesn't report its runs under a football
//! word.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::{MATCH_STATS, MATCHES};

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
    pub points_for: u64,
    pub points_against: u64,
    pub clean_sheets: u64,
    /// Sport counters, e.g. `runs`/`wickets`/`catches` for cricket or
    /// `goals`/`assists` for football — the side-level breakdown from
    /// `sides.stats`, summed across these matches. Zero totals are left
    /// out. `clean_sheets` isn't in here: it has its own field above.
    pub stats: BTreeMap<String, u64>,
}

impl RecordCounts {
    fn add(&mut self, o: &RecordCounts) {
        self.played += o.played;
        self.won += o.won;
        self.drawn += o.drawn;
        self.lost += o.lost;
        self.points_for += o.points_for;
        self.points_against += o.points_against;
        self.clean_sheets += o.clean_sheets;
        for (k, v) in &o.stats {
            *self.stats.entry(k.clone()).or_default() += v;
        }
    }
}

/// Squad games: the team plays itself, so there's no W/D/L, only how many
/// games and how many points were scored in them (both sides).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SquadCounts {
    pub games: u64,
    pub points: u64,
    /// See [`RecordCounts::stats`] — same breakdown, both sides' totals.
    pub stats: BTreeMap<String, u64>,
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
/// `sides`.
pub fn team_records_query(
    team_id: &str,
    sport: &str,
    year: Option<i32>,
    now: &str,
    stat_keys: &[String],
) -> Value {
    let scoped_filter = match year {
        Some(y) => json!({ "term": { "year": y } }),
        None => json!({ "match_all": {} }),
    };
    let mut ours_aggs = serde_json::Map::new();
    ours_aggs.insert(
        "outcome".into(),
        json!({ "terms": { "field": "sides.outcome", "size": 5 } }),
    );
    ours_aggs.insert("gf".into(), json!({ "sum": { "field": "sides.score" } }));
    ours_aggs.insert("ga".into(), json!({ "sum": { "field": "sides.conceded" } }));
    // Folded into `sides.stats` as a 1/0 counter (see
    // `docs::build_match_docs`): not a player's contribution, so it isn't
    // summed from `match_stats` like the sport counters below.
    ours_aggs.insert(
        "clean_sheets".into(),
        json!({ "sum": { "field": "sides.stats.clean_sheets" } }),
    );
    for k in stat_keys {
        ours_aggs.insert(
            format!("stat:{k}"),
            json!({ "sum": { "field": format!("sides.stats.{k}") } }),
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

/// `ours`'s `stat:<key>` sums, nonzero only — same convention as
/// [`parse_leaderboard`].
fn side_stats(ours: &Value, stat_keys: &[String]) -> BTreeMap<String, u64> {
    stat_keys
        .iter()
        .map(|k| (k.clone(), sum(&ours[format!("stat:{k}")])))
        .filter(|(_, v)| *v > 0)
        .collect()
}

/// Read a [`team_records_query`] response. `stat_keys` must be the same list
/// the query was built with.
pub fn parse_team_records(resp: &Value, stat_keys: &[String]) -> TeamRecords {
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
                    // Both sides are this team, so this sums both scores.
                    points: sum(&ours["gf"]),
                    stats: side_stats(ours, stat_keys),
                };
            }
            Some(k @ ("league" | "cup" | "friendly")) => {
                let mut rec = RecordCounts {
                    // One side per external match is ours.
                    played: count(&ours["doc_count"]),
                    points_for: sum(&ours["gf"]),
                    points_against: sum(&ours["ga"]),
                    clean_sheets: sum(&ours["clean_sheets"]),
                    stats: side_stats(ours, stat_keys),
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

/// The sport counter names (`sides.stats.*`, minus `clean_sheets` which has
/// its own dedicated field) the `matches` index holds so far, read from its
/// mapping — same idea as [`parse_stat_keys`], one level down through the
/// nested `sides` object.
pub fn parse_side_stat_keys(mapping: &Value) -> Vec<String> {
    let mut keys: Vec<String> = mapping
        .as_object()
        .into_iter()
        .flat_map(|indices| indices.values())
        .filter_map(|idx| {
            idx["mappings"]["properties"]["sides"]["properties"]["stats"]["properties"].as_object()
        })
        .flat_map(|props| props.keys().cloned())
        .filter(|k| k != "clean_sheets")
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// See [`parse_side_stat_keys`].
pub async fn side_stat_keys(os: &OpenSearchClient) -> OpenSearchResult<Vec<String>> {
    let mapping = os.get_mapping(&MATCHES.alias(os.prefix())).await?;
    Ok(parse_side_stat_keys(&mapping))
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
    let stat_keys = side_stat_keys(os).await?;
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            team_records_query(team_id, sport, year, now, &stat_keys),
        )
        .await?;
    Ok(parse_team_records(&resp, &stat_keys))
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
    score: Option<u64>,
    conceded: Option<u64>,
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
    /// `won`, `drawn` or `lost`.
    pub outcome: String,
    pub points_for: u64,
    pub points_against: u64,
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
                outcome: ours.outcome.clone()?,
                points_for: ours.score.unwrap_or(0),
                points_against: ours.conceded.unwrap_or(0),
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
    /// Sport counters, e.g. `goals`, `assists`. Zero totals are left out.
    pub stats: BTreeMap<String, u64>,
}

/// The `match_stats` query behind [`leaderboard`]. `stat_keys` are the
/// counters to total (see [`stat_keys`]): a dynamic map can't be summed
/// without naming its keys.
pub fn leaderboard_query(
    team_id: &str,
    sport: &str,
    scope: &LeaderboardScope,
    year: Option<i32>,
    stat_keys: &[String],
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
    for k in stat_keys {
        sums.insert(
            format!("stat:{k}"),
            json!({ "sum": { "field": format!("stats.{k}") } }),
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
pub fn parse_leaderboard(resp: &Value, stat_keys: &[String]) -> Vec<LeaderboardRow> {
    let mut rows: Vec<LeaderboardRow> = buckets(&resp["aggregations"]["players"])
        .iter()
        .filter_map(|b| {
            let user_id = b["key"].as_str()?.to_string();
            let stats = stat_keys
                .iter()
                .map(|k| (k.clone(), sum(&b[format!("stat:{k}")])))
                .filter(|(_, v)| *v > 0)
                .collect();
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

/// The counter names (`stats.*`) the `match_stats` index holds so far, read
/// from its mapping. New counters appear through the dynamic template the
/// first time a document carries them.
pub fn parse_stat_keys(mapping: &Value) -> Vec<String> {
    let mut keys: Vec<String> = mapping
        .as_object()
        .into_iter()
        .flat_map(|indices| indices.values())
        .filter_map(|idx| idx["mappings"]["properties"]["stats"]["properties"].as_object())
        .flat_map(|props| props.keys().cloned())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// See [`parse_stat_keys`].
pub async fn stat_keys(os: &OpenSearchClient) -> OpenSearchResult<Vec<String>> {
    let mapping = os.get_mapping(&MATCH_STATS.alias(os.prefix())).await?;
    Ok(parse_stat_keys(&mapping))
}

/// Per-player totals for `team_id` in `sport` over `scope` (and `year`).
pub async fn leaderboard(
    os: &OpenSearchClient,
    team_id: &str,
    sport: &str,
    scope: &LeaderboardScope,
    year: Option<i32>,
) -> OpenSearchResult<Vec<LeaderboardRow>> {
    let keys = stat_keys(os).await?;
    let resp = os
        .search(
            &MATCH_STATS.alias(os.prefix()),
            leaderboard_query(team_id, sport, scope, year, &keys),
        )
        .await?;
    Ok(parse_leaderboard(&resp, &keys))
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

    fn kind_bucket(kind: &str, docs: u64, ours: Value) -> Value {
        json!({ "key": kind, "doc_count": docs, "sides": { "doc_count": docs * 2, "ours": ours } })
    }

    fn ours(
        n: u64,
        w: u64,
        d: u64,
        l: u64,
        gf: f64,
        ga: f64,
        cs: u64,
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
            "outcome": { "buckets": outcomes },
            "gf": { "value": gf },
            "ga": { "value": ga },
            "clean_sheets": { "value": cs as f64 }
        });
        for (k, s) in stats {
            v[format!("stat:{k}")] = json!({ "value": s });
        }
        v
    }

    #[test]
    fn records_split_by_kind_and_sum_into_ext() {
        let keys = vec!["assists".to_string(), "goals".to_string()];
        let resp = json!({ "aggregations": {
            "years": { "buckets": [{ "key": 2026, "doc_count": 9 }, { "key": 2025, "doc_count": 3 }] },
            "recent_ext": { "doc_count": 7 },
            "scoped": { "doc_count": 12, "by_kind": { "buckets": [
                kind_bucket("league", 5, ours(5, 3, 1, 1, 9.0, 4.0, 2, &[("goals", 9.0), ("assists", 0.0)])),
                kind_bucket("friendly", 3, ours(3, 1, 0, 2, 4.0, 6.0, 0, &[("goals", 4.0), ("assists", 3.0)])),
                // Squad: both sides are ours, so gf already covers both scores.
                kind_bucket("squad", 4, json!({ "doc_count": 8, "gf": { "value": 21.0 }, "stat:goals": { "value": 21.0 } })),
            ] } }
        } });
        let r = parse_team_records(&resp, &keys);
        assert_eq!(r.years, vec![2026, 2025]);
        assert_eq!(r.ext_last_12_months, 7);
        assert_eq!(
            r.league,
            RecordCounts {
                played: 5,
                won: 3,
                drawn: 1,
                lost: 1,
                points_for: 9,
                points_against: 4,
                clean_sheets: 2,
                // `assists` is zero here, so it's left out, same as the
                // leaderboard's convention.
                stats: BTreeMap::from([("goals".to_string(), 9)]),
            }
        );
        assert_eq!(r.cup, RecordCounts::default());
        assert_eq!(r.ext.played, 8);
        assert_eq!(r.ext.won, 4);
        assert_eq!(r.ext.points_against, 10);
        // Both kinds' goals and assists sum into the external record.
        assert_eq!(
            r.ext.stats,
            BTreeMap::from([("goals".to_string(), 13), ("assists".to_string(), 3)])
        );
        assert_eq!(
            r.squad,
            SquadCounts {
                games: 4,
                points: 21,
                stats: BTreeMap::from([("goals".to_string(), 21)]),
            }
        );
    }

    #[test]
    fn records_query_applies_year_only_to_the_scoped_counts() {
        let keys = vec!["goals".to_string()];
        let q = team_records_query("t1", "football", Some(2025), "2026-11-15T00:00:00Z", &keys);
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
            json!({ "sum": { "field": "sides.stats.goals" } })
        );
        let q = team_records_query("t1", "football", None, "2026-11-15T00:00:00Z", &keys);
        assert_eq!(q["aggs"]["scoped"]["filter"], json!({ "match_all": {} }));
    }

    #[test]
    fn side_stat_keys_come_from_the_matches_mapping_and_exclude_clean_sheets() {
        let mapping = json!({ "matches_v2": { "mappings": { "properties": {
            "sides": { "properties": { "stats": { "properties": {
                "goals": { "type": "long" },
                "assists": { "type": "long" },
                "clean_sheets": { "type": "long" }
            } } } }
        } } } });
        assert_eq!(parse_side_stat_keys(&mapping), vec!["assists", "goals"]);
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
                { "side_id": "a", "team_id": "leyton", "name": "Leyton", "outcome": "won", "score": 2, "conceded": 1 },
                { "side_id": "b", "team_id": "hw", "outcome": "lost", "score": 1, "conceded": 2 }
            ])),
            match_hit("m1", "league", json!([
                { "side_id": "a", "team_id": "hw", "outcome": "lost", "score": 0, "conceded": 3 },
                { "side_id": "b", "name": "Pickup XI", "outcome": "won", "score": 3, "conceded": 0 }
            ])),
            match_hit("m0", "league", json!([
                { "side_id": "a", "team_id": "hw", "outcome": "won", "score": 2, "conceded": 0 },
                { "side_id": "b", "team_id": "x", "outcome": "lost", "score": 0, "conceded": 2 }
            ]))
        ] } });
        let rs = parse_recent_results(&resp, "hw");
        assert_eq!(rs.len(), 3);
        assert_eq!(rs[0].outcome, "lost");
        assert_eq!((rs[0].points_for, rs[0].points_against), (1, 2));
        assert_eq!(
            rs[0].opponent.as_ref().unwrap().name.as_deref(),
            Some("Leyton")
        );
        assert_eq!(rs[1].opponent.as_ref().unwrap().team_id, None);
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
        let keys = vec!["assists".to_string(), "goals".to_string()];
        let q = leaderboard_query(
            "hw",
            "football",
            &LeaderboardScope::Tournament("tour1".into()),
            Some(2026),
            &keys,
        );
        let filters = q["query"]["bool"]["filter"].to_string();
        assert!(filters.contains(r#""tournament_id":"tour1""#));
        assert!(filters.contains(r#""year":2026"#));
        assert_eq!(
            q["aggs"]["players"]["aggs"]["stat:goals"],
            json!({ "sum": { "field": "stats.goals" } })
        );
        let q = leaderboard_query("hw", "football", &LeaderboardScope::All, None, &keys);
        assert_eq!(q["query"]["bool"]["filter"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn leaderboard_rows_drop_zero_stats_and_sort_by_games() {
        let keys = vec!["assists".to_string(), "goals".to_string()];
        let resp = json!({ "aggregations": { "players": { "buckets": [
            { "key": "u2", "doc_count": 3, "played": { "value": 3.0 }, "won": { "value": 1.0 },
              "drawn": { "value": 0.0 }, "lost": { "value": 2.0 },
              "stat:goals": { "value": 0.0 }, "stat:assists": { "value": 2.0 } },
            { "key": "u1", "doc_count": 5, "played": { "value": 5.0 }, "won": { "value": 4.0 },
              "drawn": { "value": 1.0 }, "lost": { "value": 0.0 },
              "stat:goals": { "value": 6.0 }, "stat:assists": { "value": 0.0 } }
        ] } } });
        let rows = parse_leaderboard(&resp, &keys);
        assert_eq!(rows[0].user_id, "u1");
        assert_eq!(rows[0].stats, BTreeMap::from([("goals".to_string(), 6)]));
        assert_eq!(rows[1].stats, BTreeMap::from([("assists".to_string(), 2)]));
        assert_eq!(rows[1].lost, 2);
    }

    #[test]
    fn stat_keys_come_from_the_mapping() {
        let mapping = json!({ "match_stats_v1": { "mappings": { "properties": {
            "user_id": { "type": "keyword" },
            "stats": { "properties": { "goals": { "type": "long" }, "assists": { "type": "long" } } }
        } } } });
        assert_eq!(parse_stat_keys(&mapping), vec!["assists", "goals"]);
        assert!(parse_stat_keys(&json!({ "match_stats_v1": { "mappings": {} } })).is_empty());
    }
}
