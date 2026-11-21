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

/// Filters for [`search_query`]/[`search_matches`], bundled into one type so
/// neither function has to take each one as its own argument.
#[derive(Debug, Clone, Copy, Default)]
pub struct MatchFilters<'a> {
    /// Free-text match on the name. Empty/absent browses everyone.
    pub q: Option<&'a str>,
    pub sport: Option<&'a str>,
    pub status: Option<&'a str>,
    /// Only matches starting at or after this time (inclusive).
    pub from: Option<&'a str>,
    /// Only matches starting at or before this time (inclusive).
    pub to: Option<&'a str>,
}

/// The `matches` query behind [`search_matches`]. An empty/absent `q` browses
/// everyone; `sport`/`status` are exact filters; `from`/`to` bound
/// `starts_at` (either end optional). Newest-first, same convention as
/// [`super::users::search_query`].
pub fn search_query(filters: MatchFilters, offset: u32, size: u32) -> Value {
    let MatchFilters {
        q,
        sport,
        status,
        from,
        to,
    } = filters;

    let mut filter = Vec::new();
    if let Some(sport) = sport {
        filter.push(json!({ "term": { "sport": sport } }));
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
    let must = match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => vec![json!({
            "multi_match": { "query": q, "type": "bool_prefix", "fields": ["name"] }
        })],
        None => Vec::new(),
    };

    json!({
        "from": offset,
        "size": size,
        "track_total_hits": true,
        "_source": SOURCE_FIELDS,
        "query": { "bool": { "must": must, "filter": filter } },
        "sort": [{ "starts_at": "desc" }, { "match_id": "desc" }]
    })
}

/// Read a [`search_query`] response: matching rows (ranked order) and the
/// total hit count.
pub fn parse_search(resp: &Value) -> (Vec<MatchRow>, u64) {
    let rows = resp["hits"]["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| serde_json::from_value(h["_source"].clone()).ok())
        .collect();
    let total = resp["hits"]["total"]["value"].as_u64().unwrap_or(0);
    (rows, total)
}

/// Search/browse the `matches` index: matching rows (ranked) and the total
/// hit count, for the caller to turn into a next-page offset.
pub async fn search_matches(
    os: &OpenSearchClient,
    filters: MatchFilters<'_>,
    offset: u32,
    size: u32,
) -> OpenSearchResult<(Vec<MatchRow>, u64)> {
    let resp = os
        .search(&MATCHES.alias(os.prefix()), search_query(filters, offset, size))
        .await?;
    Ok(parse_search(&resp))
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
                sport: Some("football"),
                status: Some("completed"),
                from: Some("2026-01-01T00:00:00Z"),
                to: Some("2026-12-31T23:59:59Z"),
                ..Default::default()
            },
            0,
            20,
        );
        let filter = q["query"]["bool"]["filter"].as_array().unwrap();
        assert!(filter.contains(&json!({ "term": { "sport": "football" } })));
        assert!(filter.contains(&json!({ "term": { "status": "completed" } })));
        assert!(filter.contains(&json!({
            "range": { "starts_at": { "gte": "2026-01-01T00:00:00Z", "lte": "2026-12-31T23:59:59Z" } }
        })));
    }

    #[test]
    fn non_empty_query_matches_name_as_a_prefix() {
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
            json!({ "multi_match": { "query": "derby", "type": "bool_prefix", "fields": ["name"] } })
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
        let (rows, total) = parse_search(&resp);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].match_id, "m2");
        assert_eq!(rows[1].match_id, "m1");
        assert_eq!(total, 3);
    }

    #[test]
    fn parse_handles_no_hits() {
        let (rows, total) =
            parse_search(&json!({ "hits": { "total": { "value": 0 }, "hits": [] } }));
        assert!(rows.is_empty());
        assert_eq!(total, 0);
    }
}
