//! Typed per-user stats reads over the OpenSearch indices: a player's own
//! record per match kind, recent results and next match.
//!
//! Unlike a team's record (`team_stats`), a player's record comes straight off
//! `match_stats`: that index already carries one row per confirmed match for
//! this exact player, with the outcome from their own side and no nested
//! aggregation needed. There's no per-player equivalent of a team's
//! goals-for/against or clean sheets — those are side-level, not personal —
//! and no leaderboard (a leaderboard is inherently team-scoped).
//!
//! Every query here is built by a pure `*_query` function and every response
//! is read by a pure `parse_*` function, so both halves are unit-tested
//! without a cluster. The `async` functions only glue them to
//! [`OpenSearchClient::search`]. Callers never see query JSON.
//!
//! **What counts.** `match_stats` only ever holds confirmed matches, so no
//! extra filter is needed for that. Squad (in-house) games are kept out of the
//! external record, form and last 10 — same convention as `team_stats`, even
//! though (unlike for a team) a squad game does have a real personal outcome
//! for the player, which is why `squad` still carries W/D/L here rather than
//! just a game/goal count.

use serde::Deserialize;
use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::{MATCH_STATS, MATCHES};

// ── Records ─────────────────────────────────────────────────────────────────

/// A player's results over some set of matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UserRecordCounts {
    pub played: u64,
    pub won: u64,
    pub drawn: u64,
    pub lost: u64,
}

impl UserRecordCounts {
    fn add(&mut self, o: &UserRecordCounts) {
        self.played += o.played;
        self.won += o.won;
        self.drawn += o.drawn;
        self.lost += o.lost;
    }
}

/// A player's record in one sport, per kind, optionally for one year.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserRecords {
    /// League + cup + friendly.
    pub ext: UserRecordCounts,
    pub league: UserRecordCounts,
    pub cup: UserRecordCounts,
    pub friendly: UserRecordCounts,
    pub squad: UserRecordCounts,
    /// Every year this player has a confirmed match in (this sport, any
    /// kind), newest first. Unaffected by the year filter.
    pub years: Vec<i32>,
    /// External matches in the 12 months before `now`. Unaffected by the
    /// year filter. Drives the overview layout.
    pub ext_last_12_months: u64,
}

/// The `match_stats` query behind [`user_records`]. One request: `years` and
/// `recent_ext` ignore the year filter; `scoped` applies it and splits by
/// kind, summing this player's own `played`/`won`/`drawn`/`lost` columns —
/// each already 0/1 per row, so a `sum` per kind is the per-kind count.
pub fn user_records_query(user_id: &str, sport: &str, year: Option<i32>, now: &str) -> Value {
    let scoped_filter = match year {
        Some(y) => json!({ "term": { "year": y } }),
        None => json!({ "match_all": {} }),
    };
    json!({
        "size": 0,
        "track_total_hits": false,
        "query": { "bool": { "filter": [
            { "term": { "user_id": user_id } },
            { "term": { "sport": sport } }
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
                    "aggs": {
                        "played": { "sum": { "field": "played" } },
                        "won": { "sum": { "field": "won" } },
                        "drawn": { "sum": { "field": "drawn" } },
                        "lost": { "sum": { "field": "lost" } }
                    }
                } }
            }
        }
    })
}

/// Read a [`user_records_query`] response.
pub fn parse_user_records(resp: &Value) -> UserRecords {
    let aggs = &resp["aggregations"];
    let mut out = UserRecords {
        years: buckets(&aggs["years"])
            .iter()
            .filter_map(|b| b["key"].as_i64())
            .map(|y| y as i32)
            .collect(),
        ext_last_12_months: count(&aggs["recent_ext"]["doc_count"]),
        ..Default::default()
    };
    for kind in buckets(&aggs["scoped"]["by_kind"]) {
        let rec = UserRecordCounts {
            played: sum(&kind["played"]),
            won: sum(&kind["won"]),
            drawn: sum(&kind["drawn"]),
            lost: sum(&kind["lost"]),
        };
        match kind["key"].as_str() {
            Some("squad") => out.squad = rec,
            Some(k @ ("league" | "cup" | "friendly")) => {
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

/// A player's record in `sport`, per kind. `year` limits the per-kind counts
/// to one year; `years` and `ext_last_12_months` always cover everything.
pub async fn user_records(
    os: &OpenSearchClient,
    user_id: &str,
    sport: &str,
    year: Option<i32>,
    now: &str,
) -> OpenSearchResult<UserRecords> {
    let resp = os
        .search(
            &MATCH_STATS.alias(os.prefix()),
            user_records_query(user_id, sport, year, now),
        )
        .await?;
    Ok(parse_user_records(&resp))
}

// ── Recent results ──────────────────────────────────────────────────────────

/// The `match_stats` fields the recent-results query reads.
#[derive(Debug, Clone, Deserialize)]
struct StatSource {
    match_id: String,
    kind: String,
    ranked: bool,
    starts_at: String,
    tournament_id: Option<String>,
    won: u32,
    drawn: u32,
    lost: u32,
    opponent_team_ids: Vec<String>,
}

const STAT_SOURCE_FIELDS: [&str; 9] = [
    "match_id",
    "kind",
    "ranked",
    "starts_at",
    "tournament_id",
    "won",
    "drawn",
    "lost",
    "opponent_team_ids",
];

/// One of a player's confirmed external results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserResult {
    pub match_id: String,
    pub kind: String,
    pub ranked: bool,
    pub starts_at: String,
    pub tournament_id: Option<String>,
    /// `won`, `drawn` or `lost`.
    pub outcome: String,
    pub opponent_team_ids: Vec<String>,
}

/// The `match_stats` query behind [`recent_results`]: not squad, newest first.
pub fn recent_results_query(user_id: &str, sport: &str, limit: u32) -> Value {
    json!({
        "size": limit,
        "_source": STAT_SOURCE_FIELDS,
        "query": { "bool": {
            "filter": [
                { "term": { "user_id": user_id } },
                { "term": { "sport": sport } }
            ],
            "must_not": [{ "term": { "kind": "squad" } }]
        } },
        "sort": [{ "starts_at": "desc" }, { "match_id": "desc" }]
    })
}

fn outcome_of(s: &StatSource) -> &'static str {
    debug_assert_eq!(
        s.won + s.drawn + s.lost,
        1,
        "expected exactly one outcome flag set"
    );
    if s.won > 0 {
        "won"
    } else if s.lost > 0 {
        "lost"
    } else {
        "drawn"
    }
}

/// Read a [`recent_results_query`] response.
pub fn parse_recent_results(resp: &Value) -> Vec<UserResult> {
    resp["hits"]["hits"]
        .as_array()
        .map(|hs| {
            hs.iter()
                .filter_map(|h| {
                    let s: StatSource = serde_json::from_value(h["_source"].clone()).ok()?;
                    Some(UserResult {
                        outcome: outcome_of(&s).to_string(),
                        match_id: s.match_id,
                        kind: s.kind,
                        ranked: s.ranked,
                        starts_at: s.starts_at,
                        tournament_id: s.tournament_id,
                        opponent_team_ids: s.opponent_team_ids,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The current run of identical outcomes, newest first (`results` must be
/// newest first). `None` with no results.
pub fn streak(results: &[UserResult]) -> Option<(String, u32)> {
    let first = results.first()?;
    let n = results
        .iter()
        .take_while(|r| r.outcome == first.outcome)
        .count();
    Some((first.outcome.clone(), n as u32))
}

/// The player's last `limit` confirmed external results, newest first.
pub async fn recent_results(
    os: &OpenSearchClient,
    user_id: &str,
    sport: &str,
    limit: u32,
) -> OpenSearchResult<Vec<UserResult>> {
    let resp = os
        .search(
            &MATCH_STATS.alias(os.prefix()),
            recent_results_query(user_id, sport, limit),
        )
        .await?;
    Ok(parse_recent_results(&resp))
}

// ── Next match ───────────────────────────────────────────────────────────────

/// One side of a match as stored in `matches`, with enough to tell whether
/// this player is on it.
#[derive(Debug, Clone, Deserialize)]
struct SideSource {
    side_id: String,
    team_id: Option<String>,
    name: Option<String>,
    player_ids: Vec<String>,
}

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

/// The other side in a player's match: its team (if any) and display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opponent {
    pub team_id: Option<String>,
    pub name: Option<String>,
}

/// A player's next scheduled match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingMatch {
    pub match_id: String,
    pub name: String,
    pub kind: String,
    pub ranked: bool,
    pub starts_at: String,
    pub tournament_id: Option<String>,
    /// Absent for a squad game — the "opponent" would just be the player's
    /// own team again.
    pub opponent: Option<Opponent>,
}

/// The `matches` query behind [`next_match`]: scheduled, from `now`, soonest
/// first. Squad games count: it's still the player's next fixture.
pub fn next_match_query(user_id: &str, sport: &str, now: &str) -> Value {
    json!({
        "size": 1,
        "_source": MATCH_SOURCE_FIELDS,
        "query": { "bool": { "filter": [
            { "term": { "participant_ids": user_id } },
            { "term": { "sport": sport } },
            { "term": { "status": "scheduled" } },
            { "range": { "starts_at": { "gte": now } } }
        ] } },
        "sort": [{ "starts_at": "asc" }, { "match_id": "asc" }]
    })
}

fn ours_and_theirs<'a>(m: &'a MatchSource, user_id: &str) -> Option<(&'a SideSource, Opponent)> {
    let ours = m
        .sides
        .iter()
        .find(|s| s.player_ids.iter().any(|p| p == user_id))?;
    let theirs = m.sides.iter().find(|s| s.side_id != ours.side_id)?;
    Some((
        ours,
        Opponent {
            team_id: theirs.team_id.clone(),
            name: theirs.name.clone(),
        },
    ))
}

/// Read a [`next_match_query`] response.
pub fn parse_next_match(resp: &Value, user_id: &str) -> Option<UpcomingMatch> {
    let m = resp["hits"]["hits"]
        .as_array()
        .and_then(|hs| hs.first())
        .and_then(|h| serde_json::from_value::<MatchSource>(h["_source"].clone()).ok())?;
    let opponent = if m.kind == "squad" {
        None
    } else {
        ours_and_theirs(&m, user_id).map(|(_, o)| o)
    };
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

/// The player's next scheduled match from `now`, if any.
pub async fn next_match(
    os: &OpenSearchClient,
    user_id: &str,
    sport: &str,
    now: &str,
) -> OpenSearchResult<Option<UpcomingMatch>> {
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            next_match_query(user_id, sport, now),
        )
        .await?;
    Ok(parse_next_match(&resp, user_id))
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

    fn kind_bucket(kind: &str, played: u64, won: u64, drawn: u64, lost: u64) -> Value {
        json!({
            "key": kind, "doc_count": played,
            "played": { "value": played as f64 }, "won": { "value": won as f64 },
            "drawn": { "value": drawn as f64 }, "lost": { "value": lost as f64 }
        })
    }

    #[test]
    fn records_split_by_kind_and_sum_into_ext_with_squad_keeping_its_own_wdl() {
        let resp = json!({ "aggregations": {
            "years": { "buckets": [{ "key": 2026, "doc_count": 9 }, { "key": 2025, "doc_count": 3 }] },
            "recent_ext": { "doc_count": 7 },
            "scoped": { "doc_count": 12, "by_kind": { "buckets": [
                kind_bucket("league", 5, 3, 1, 1),
                kind_bucket("friendly", 3, 1, 0, 2),
                kind_bucket("squad", 4, 2, 1, 1),
            ] } }
        } });
        let r = parse_user_records(&resp);
        assert_eq!(r.years, vec![2026, 2025]);
        assert_eq!(r.ext_last_12_months, 7);
        assert_eq!(
            r.league,
            UserRecordCounts {
                played: 5,
                won: 3,
                drawn: 1,
                lost: 1
            }
        );
        assert_eq!(r.cup, UserRecordCounts::default());
        assert_eq!(r.ext.played, 8);
        assert_eq!(r.ext.won, 4);
        assert_eq!(
            r.squad,
            UserRecordCounts {
                played: 4,
                won: 2,
                drawn: 1,
                lost: 1
            }
        );
    }

    #[test]
    fn records_query_applies_year_only_to_the_scoped_counts() {
        let q = user_records_query("u1", "football", Some(2025), "2026-11-15T00:00:00Z");
        assert_eq!(
            q["aggs"]["scoped"]["filter"],
            json!({ "term": { "year": 2025 } })
        );
        let top = q["query"]["bool"]["filter"].to_string();
        assert!(
            !top.contains("year"),
            "the year must not filter the whole query"
        );
        let q = user_records_query("u1", "football", None, "2026-11-15T00:00:00Z");
        assert_eq!(q["aggs"]["scoped"]["filter"], json!({ "match_all": {} }));
    }

    fn stat_hit(id: &str, kind: &str, won: u32, drawn: u32, lost: u32) -> Value {
        json!({ "_source": {
            "match_id": id, "kind": kind, "ranked": false, "tournament_id": null,
            "starts_at": "2026-11-08T10:30:00Z", "won": won, "drawn": drawn, "lost": lost,
            "opponent_team_ids": ["hw"]
        } })
    }

    #[test]
    fn recent_results_read_outcome_from_the_wdl_columns() {
        let resp = json!({ "hits": { "hits": [
            stat_hit("m2", "friendly", 0, 0, 1),
            stat_hit("m1", "league", 0, 0, 1),
            stat_hit("m0", "league", 1, 0, 0),
        ] } });
        let rs = parse_recent_results(&resp);
        assert_eq!(rs.len(), 3);
        assert_eq!(rs[0].outcome, "lost");
        assert_eq!(rs[0].opponent_team_ids, vec!["hw".to_string()]);
        assert_eq!(streak(&rs), Some(("lost".into(), 2)));
        assert_eq!(streak(&[]), None);
    }

    fn match_hit(id: &str, kind: &str, sides: Value) -> Value {
        json!({ "_source": {
            "match_id": id, "name": id, "kind": kind, "ranked": false,
            "starts_at": "2026-11-08T10:30:00Z", "sides": sides
        } })
    }

    #[test]
    fn next_match_reads_opponent_from_the_other_side() {
        let resp = json!({ "hits": { "hits": [match_hit("m9", "friendly", json!([
            { "side_id": "a", "team_id": "hw", "name": null, "player_ids": ["u1"] },
            { "side_id": "b", "team_id": "lo", "name": "Leyton", "player_ids": ["u2"] }
        ]))] } });
        let m = parse_next_match(&resp, "u1").unwrap();
        assert_eq!(m.kind, "friendly");
        assert_eq!(
            m.opponent,
            Some(Opponent {
                team_id: Some("lo".into()),
                name: Some("Leyton".into())
            })
        );
    }

    #[test]
    fn squad_game_has_no_opponent() {
        let resp = json!({ "hits": { "hits": [match_hit("m9", "squad", json!([
            { "side_id": "l", "team_id": "hw", "name": null, "player_ids": ["u1"] },
            { "side_id": "d", "team_id": "hw", "name": null, "player_ids": ["u2"] }
        ]))] } });
        let m = parse_next_match(&resp, "u1").unwrap();
        assert_eq!(m.opponent, None);
        assert_eq!(
            parse_next_match(&json!({ "hits": { "hits": [] } }), "u1"),
            None
        );
    }
}
