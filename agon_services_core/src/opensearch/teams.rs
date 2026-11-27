//! Typed read of the `teams` index for `GET /teams/search`.
//!
//! Same pure-query / pure-parse split as [`super::users`]: [`search_query`]
//! builds the request and [`parse_search`] reads the response, both unit
//! tested without a cluster; [`search_teams`] only glues them to
//! [`OpenSearchClient::search`].

use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::TEAMS;
use super::text::typo_tolerant_prefix;

/// The `teams` query behind [`search_teams`]. An empty/absent `q` browses
/// every team, newest first. A non-empty `q` is a typo-tolerant prefix match
/// on the name (see [`typo_tolerant_prefix`]).
pub fn search_query(q: Option<&str>, from: u32, size: u32) -> Value {
    let query = match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => typo_tolerant_prefix(q, &["name"]),
        None => json!({ "match_all": {} }),
    };
    json!({
        "from": from,
        "size": size,
        "track_total_hits": true,
        "query": query,
        "sort": [{ "_score": "desc" }, { "created_at": "desc" }, { "team_id": "desc" }],
        "_source": false
    })
}

/// Read a [`search_query`] response: matching team ids (ranked order) and the
/// total hit count.
pub fn parse_search(resp: &Value) -> (Vec<String>, u64) {
    let ids = resp["hits"]["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| h["_id"].as_str().map(str::to_string))
        .collect();
    let total = resp["hits"]["total"]["value"].as_u64().unwrap_or(0);
    (ids, total)
}

/// Search/browse the `teams` index: matching ids (ranked) and the total hit
/// count, for the caller to turn into a next-page offset.
pub async fn search_teams(
    os: &OpenSearchClient,
    q: Option<&str>,
    from: u32,
    size: u32,
) -> OpenSearchResult<(Vec<String>, u64)> {
    let resp = os
        .search(&TEAMS.alias(os.prefix()), search_query(q, from, size))
        .await?;
    Ok(parse_search(&resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_browses_every_team_newest_first() {
        let q = search_query(None, 0, 20);
        assert_eq!(q["query"], json!({ "match_all": {} }));
        assert_eq!(q["sort"][1], json!({ "created_at": "desc" }));
        assert_eq!(
            search_query(Some("  "), 0, 20)["query"],
            json!({ "match_all": {} })
        );
    }

    #[test]
    fn non_empty_query_is_a_typo_tolerant_prefix_on_name() {
        let q = search_query(Some("rov"), 40, 20);
        assert_eq!(q["query"], typo_tolerant_prefix("rov", &["name"]));
        assert_eq!(q["from"], 40);
        assert_eq!(q["size"], 20);
    }

    #[test]
    fn parse_reads_ids_in_hit_order_and_the_total() {
        let resp = json!({ "hits": {
            "total": { "value": 3 },
            "hits": [{ "_id": "t2" }, { "_id": "t1" }]
        } });
        let (ids, total) = parse_search(&resp);
        assert_eq!(ids, vec!["t2".to_string(), "t1".to_string()]);
        assert_eq!(total, 3);
    }
}
