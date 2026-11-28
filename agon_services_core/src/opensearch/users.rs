//! Typed read of the `users` index for user search (`GET /users/search`) and
//! the admin panel's user search/browse (`GET /admin/users`).
//!
//! Same pure-query / pure-parse split as [`super::team_stats`]: [`search_query`]
//! builds the request and [`parse_search`] reads the response, both unit
//! tested without a cluster; [`search_users`] only glues them to
//! [`OpenSearchClient::search`].

use serde_json::{Value, json};

use super::client::{OpenSearchClient, OpenSearchResult};
use super::schema::USERS;
use super::text::typo_tolerant_prefix;

/// Which fields a user search matches against.
#[derive(Debug, Clone, Copy)]
pub enum UserSearchFields {
    /// What anyone may search by.
    Name,
    /// For the admin panel, which also finds a user by email.
    NameAndEmail,
}

impl UserSearchFields {
    fn fields(self) -> &'static [&'static str] {
        match self {
            UserSearchFields::Name => &["name"],
            UserSearchFields::NameAndEmail => &["name", "email"],
        }
    }
}

/// The `users` query behind [`search_users`]. An empty/absent `q` browses
/// everyone, newest first. A non-empty `q` is a typo-tolerant prefix match on
/// `fields` (see [`typo_tolerant_prefix`]).
pub fn search_query(q: Option<&str>, fields: UserSearchFields, from: u32, size: u32) -> Value {
    let query = match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => typo_tolerant_prefix(q, fields.fields()),
        None => json!({ "match_all": {} }),
    };
    json!({
        "from": from,
        "size": size,
        "track_total_hits": true,
        "query": query,
        "sort": [{ "_score": "desc" }, { "created_at": "desc" }, { "user_id": "desc" }],
        "_source": false
    })
}

/// Read a [`search_query`] response: matching user ids (ranked order) and the
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

/// Search/browse the `users` index: matching ids (ranked) and the total hit
/// count, for the caller to turn into a next-page offset.
pub async fn search_users(
    os: &OpenSearchClient,
    q: Option<&str>,
    fields: UserSearchFields,
    from: u32,
    size: u32,
) -> OpenSearchResult<(Vec<String>, u64)> {
    let resp = os
        .search(
            &USERS.alias(os.prefix()),
            search_query(q, fields, from, size),
        )
        .await?;
    Ok(parse_search(&resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_browses_everyone_newest_first() {
        let q = search_query(None, UserSearchFields::Name, 0, 20);
        assert_eq!(q["query"], json!({ "match_all": {} }));
        assert_eq!(q["sort"][1], json!({ "created_at": "desc" }));
        assert_eq!(q["from"], 0);
        assert_eq!(q["size"], 20);

        assert_eq!(
            search_query(Some("  "), UserSearchFields::Name, 0, 20)["query"],
            json!({ "match_all": {} })
        );
    }

    #[test]
    fn non_empty_query_is_a_typo_tolerant_prefix_on_the_chosen_fields() {
        let q = search_query(Some("jam"), UserSearchFields::Name, 40, 20);
        assert_eq!(q["query"], typo_tolerant_prefix("jam", &["name"]));
        assert_eq!(q["from"], 40);
        let q = search_query(Some("jam"), UserSearchFields::NameAndEmail, 0, 20);
        assert_eq!(q["query"], typo_tolerant_prefix("jam", &["name", "email"]));
    }

    #[test]
    fn parse_reads_ids_in_hit_order_and_the_total() {
        let resp = json!({ "hits": {
            "total": { "value": 57 },
            "hits": [{ "_id": "u2" }, { "_id": "u1" }]
        } });
        let (ids, total) = parse_search(&resp);
        assert_eq!(ids, vec!["u2".to_string(), "u1".to_string()]);
        assert_eq!(total, 57);
    }

    #[test]
    fn parse_handles_no_hits() {
        let (ids, total) =
            parse_search(&json!({ "hits": { "total": { "value": 0 }, "hits": [] } }));
        assert!(ids.is_empty());
        assert_eq!(total, 0);
    }
}
