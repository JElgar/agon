//! Typed read of the `matches` index for the admin panel's match search/
//! browse (`GET /admin/matches`).
//!
//! Same pure-query / pure-parse split as [`super::users`]: [`search_query`]
//! builds the request and [`parse_search`] reads the response, both unit
//! tested without a cluster; [`search_matches`] only glues them to
//! [`OpenSearchClient::search`].

use serde::Deserialize;
use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::MATCHES;
use super::text::typo_tolerant_prefix;

/// One match row, as read off the `matches` index for the admin list — just
/// what that list shows, not the full match detail (which the admin panel
/// links out to instead of duplicating).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MatchRow {
    pub match_id: String,
    pub name: String,
    pub sport: String,
    pub status: String,
    pub starts_at: String,
}

const SOURCE_FIELDS: [&str; 5] = ["match_id", "name", "sport", "status", "starts_at"];
const OUTCOME_FIELDS: [&str; 3] = [
    "winning_participant_ids",
    "losing_participant_ids",
    "drawing_participant_ids",
];

/// Filters for [`search_query`]/[`search_matches`], bundled into one type so
/// neither function has to take each one as its own argument. Every field is
/// optional (an empty slice or `None` doesn't filter), and all that are set
/// must hold.
#[derive(Debug, Clone, Copy, Default)]
pub struct MatchFilters<'a> {
    /// Free-text match on the name, tolerant of typos. Empty/absent browses
    /// everyone.
    pub q: Option<&'a str>,
    /// Any one of these sports.
    pub sports: &'a [&'a str],
    pub status: Option<&'a str>,
    /// Only matches starting at or after this time (inclusive).
    pub from: Option<&'a str>,
    /// Only matches starting at or before this time (inclusive).
    pub to: Option<&'a str>,
    /// Only matches this person is on the roster of, by user id or player id
    /// (whatever the invitation status). Also the subject of each hit's
    /// [`MatchHit::outcome`].
    pub participant: Option<&'a str>,
    /// Matches these teams played in: any one of them, or every one when
    /// `all_teams` is set (head-to-head).
    pub team_ids: &'a [&'a str],
    pub all_teams: bool,
    /// `league`, `cup`, `friendly`, `squad` or `pickup`.
    pub kind: Option<&'a str>,
    pub tournament_id: Option<&'a str>,
    /// Matches with (`true`) or without (`false`) an open-join policy.
    pub open: Option<bool>,
    /// With `open: Some(true)`, only matches that still have room for
    /// another individual arrival.
    pub open_has_space: bool,
    /// Oldest first, instead of the default newest first.
    pub ascending: bool,
}

/// The `matches` query behind [`search_matches`]. Sorted by `starts_at`
/// (newest first unless `ascending`), with `match_id` breaking ties so pages
/// don't overlap.
pub fn search_query(filters: MatchFilters, offset: u32, size: u32) -> Value {
    let MatchFilters {
        q,
        sports,
        status,
        from,
        to,
        participant,
        team_ids,
        all_teams,
        kind,
        tournament_id,
        open,
        open_has_space,
        ascending,
    } = filters;

    let mut filter = Vec::new();
    if !sports.is_empty() {
        filter.push(json!({ "terms": { "sport": sports } }));
    }
    if let Some(status) = status {
        filter.push(json!({ "term": { "status": status } }));
    }
    if from.is_some() || to.is_some() {
        let mut range = serde_json::Map::new();
        if let Some(from) = from {
            range.insert("gte".to_string(), json!(from));
        }
        if let Some(to) = to {
            range.insert("lte".to_string(), json!(to));
        }
        filter.push(json!({ "range": { "starts_at": range } }));
    }
    if let Some(participant) = participant {
        filter.push(json!({ "term": { "roster_ids": participant } }));
    }
    if all_teams {
        filter.extend(
            team_ids
                .iter()
                .map(|t| json!({ "term": { "team_ids": t } })),
        );
    } else if !team_ids.is_empty() {
        filter.push(json!({ "terms": { "team_ids": team_ids } }));
    }
    if let Some(kind) = kind {
        filter.push(json!({ "term": { "kind": kind } }));
    }
    if let Some(t) = tournament_id {
        filter.push(json!({ "term": { "tournament_id": t } }));
    }
    if let Some(open) = open {
        filter.push(json!({ "term": { "open": open } }));
        if open && open_has_space {
            filter.push(json!({ "term": { "open_has_space": true } }));
        }
    }
    let must = match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => vec![typo_tolerant_prefix(q, &["name"])],
        None => Vec::new(),
    };

    let mut source = SOURCE_FIELDS.to_vec();
    if participant.is_some() {
        source.extend(OUTCOME_FIELDS);
    }
    let dir = if ascending { "asc" } else { "desc" };
    json!({
        "from": offset,
        "size": size,
        "track_total_hits": true,
        "_source": source,
        "query": { "bool": { "must": must, "filter": filter } },
        "sort": [{ "starts_at": dir }, { "match_id": dir }]
    })
}

/// One hit of [`search_query`]: the match and, when the query had a
/// `participant`, how that person fared in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchHit {
    pub row: MatchRow,
    /// `None` without a `participant`, or until the score is confirmed.
    pub outcome: Option<ParticipantOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticipantOutcome {
    Won,
    Lost,
    Drew,
}

/// The outcome lists of a hit's `_source`, present when the query asked for
/// them.
#[derive(Debug, Default, Deserialize)]
struct OutcomeLists {
    #[serde(default)]
    winning_participant_ids: Vec<String>,
    #[serde(default)]
    losing_participant_ids: Vec<String>,
    #[serde(default)]
    drawing_participant_ids: Vec<String>,
}

/// Read a [`search_query`] response: matching hits (ranked order) and the
/// total hit count. `participant` must be the one the query was built with.
pub fn parse_search(resp: &Value, participant: Option<&str>) -> (Vec<MatchHit>, u64) {
    let hits = resp["hits"]["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let row: MatchRow = serde_json::from_value(h["_source"].clone()).ok()?;
            let outcome = participant.and_then(|p| {
                let lists: OutcomeLists =
                    serde_json::from_value(h["_source"].clone()).unwrap_or_default();
                let has = |ids: &[String]| ids.iter().any(|id| id == p);
                if has(&lists.winning_participant_ids) {
                    Some(ParticipantOutcome::Won)
                } else if has(&lists.losing_participant_ids) {
                    Some(ParticipantOutcome::Lost)
                } else if has(&lists.drawing_participant_ids) {
                    Some(ParticipantOutcome::Drew)
                } else {
                    None
                }
            });
            Some(MatchHit { row, outcome })
        })
        .collect();
    let total = resp["hits"]["total"]["value"].as_u64().unwrap_or(0);
    (hits, total)
}

/// Search/browse the `matches` index: matching hits (ranked) and the total
/// hit count, for the caller to turn into a next-page offset.
pub async fn search_matches(
    os: &OpenSearchClient,
    filters: MatchFilters<'_>,
    offset: u32,
    size: u32,
) -> OpenSearchResult<(Vec<MatchHit>, u64)> {
    let participant = filters.participant;
    let resp = os
        .search(
            &MATCHES.alias(os.prefix()),
            search_query(filters, offset, size),
        )
        .await?;
    Ok(parse_search(&resp, participant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filters_browses_everyone_newest_first() {
        let q = search_query(MatchFilters::default(), 0, 20);
        assert_eq!(q["query"], json!({ "bool": { "must": [], "filter": [] } }));
        assert_eq!(q["sort"][0], json!({ "starts_at": "desc" }));
        assert_eq!(q["from"], 0);
        assert_eq!(q["size"], 20);
    }

    #[test]
    fn sport_status_and_date_range_become_term_and_range_filters() {
        let q = search_query(
            MatchFilters {
                sports: &["football"],
                status: Some("completed"),
                from: Some("2026-01-01T00:00:00Z"),
                to: Some("2026-12-31T23:59:59Z"),
                ..Default::default()
            },
            0,
            20,
        );
        let filter = q["query"]["bool"]["filter"].as_array().unwrap();
        assert!(filter.contains(&json!({ "terms": { "sport": ["football"] } })));
        assert!(filter.contains(&json!({ "term": { "status": "completed" } })));
        assert!(filter.contains(&json!({
            "range": { "starts_at": { "gte": "2026-01-01T00:00:00Z", "lte": "2026-12-31T23:59:59Z" } }
        })));
    }

    #[test]
    fn non_empty_query_matches_name_as_a_typo_tolerant_prefix() {
        let q = search_query(
            MatchFilters {
                q: Some("derby"),
                ..Default::default()
            },
            40,
            20,
        );
        assert_eq!(
            q["query"]["bool"]["must"][0],
            typo_tolerant_prefix("derby", &["name"])
        );
        assert_eq!(q["from"], 40);
    }

    #[test]
    fn parse_reads_rows_in_hit_order_and_the_total() {
        let resp = json!({ "hits": {
            "total": { "value": 3 },
            "hits": [
                { "_source": { "match_id": "m2", "name": "B", "sport": "football", "status": "scheduled", "starts_at": "2026-02-01T00:00:00Z" } },
                { "_source": { "match_id": "m1", "name": "A", "sport": "tennis", "status": "completed", "starts_at": "2026-01-01T00:00:00Z" } }
            ]
        } });
        let (hits, total) = parse_search(&resp, None);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].row.match_id, "m2");
        assert_eq!(hits[1].row.match_id, "m1");
        assert!(hits.iter().all(|h| h.outcome.is_none()));
        assert_eq!(total, 3);
    }

    #[test]
    fn parse_handles_no_hits() {
        let (hits, total) = parse_search(
            &json!({ "hits": { "total": { "value": 0 }, "hits": [] } }),
            None,
        );
        assert!(hits.is_empty());
        assert_eq!(total, 0);
    }

    #[test]
    fn participant_team_kind_and_open_filters() {
        let q = search_query(
            MatchFilters {
                participant: Some("u1"),
                team_ids: &["a", "b"],
                all_teams: true,
                kind: Some("league"),
                tournament_id: Some("t1"),
                open: Some(true),
                open_has_space: true,
                ascending: true,
                ..Default::default()
            },
            0,
            20,
        );
        let filter = q["query"]["bool"]["filter"].as_array().unwrap();
        for f in [
            json!({ "term": { "roster_ids": "u1" } }),
            json!({ "term": { "team_ids": "a" } }),
            json!({ "term": { "team_ids": "b" } }),
            json!({ "term": { "kind": "league" } }),
            json!({ "term": { "tournament_id": "t1" } }),
            json!({ "term": { "open": true } }),
            json!({ "term": { "open_has_space": true } }),
        ] {
            assert!(filter.contains(&f), "missing {f}");
        }
        assert_eq!(
            q["sort"],
            json!([{ "starts_at": "asc" }, { "match_id": "asc" }])
        );
        assert!(
            q["_source"]
                .as_array()
                .unwrap()
                .contains(&json!("winning_participant_ids"))
        );
    }

    #[test]
    fn any_team_is_one_terms_filter_and_open_space_needs_open() {
        let q = search_query(
            MatchFilters {
                team_ids: &["a", "b"],
                open: Some(false),
                open_has_space: true,
                ..Default::default()
            },
            0,
            20,
        );
        let filter = q["query"]["bool"]["filter"].as_array().unwrap();
        assert!(filter.contains(&json!({ "terms": { "team_ids": ["a", "b"] } })));
        assert!(filter.contains(&json!({ "term": { "open": false } })));
        assert!(!filter.contains(&json!({ "term": { "open_has_space": true } })));
        assert!(
            !q["_source"]
                .as_array()
                .unwrap()
                .contains(&json!("winning_participant_ids"))
        );
    }

    #[test]
    fn the_participants_outcome_comes_from_the_bucket_they_are_in() {
        let hit = |id: &str, win: &[&str], lose: &[&str], draw: &[&str]| {
            json!({ "_source": {
                "match_id": id, "name": "n", "sport": "football", "status": "completed",
                "starts_at": "2026-01-01T00:00:00Z",
                "winning_participant_ids": win, "losing_participant_ids": lose,
                "drawing_participant_ids": draw
            } })
        };
        let resp = json!({ "hits": { "total": { "value": 4 }, "hits": [
            hit("w", &["u1"], &["x"], &[]),
            hit("l", &["x"], &["u1"], &[]),
            hit("d", &[], &[], &["u1", "x"]),
            hit("none", &[], &[], &[]),
        ] } });
        let (hits, _) = parse_search(&resp, Some("u1"));
        let outcomes: Vec<_> = hits.iter().map(|h| h.outcome).collect();
        assert_eq!(
            outcomes,
            vec![
                Some(ParticipantOutcome::Won),
                Some(ParticipantOutcome::Lost),
                Some(ParticipantOutcome::Drew),
                None
            ]
        );
    }
}
