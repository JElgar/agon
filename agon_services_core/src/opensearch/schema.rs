//! Index definitions: name, version and the settings + mappings each version is
//! created with.
//!
//! **Changing a mapping means bumping `version`.** OpenSearch can't change the
//! type of an existing field in place, so a new version is a new physical index
//! (`<name>_v<version>`), filled by the backfill and then swapped in behind the
//! alias `<name>`. Adding a brand-new optional field is the one change that could
//! skip a bump, but bump anyway: the backfill is cheap and it keeps "what's in
//! the index" equal to "what this version's builder writes".

use serde_json::{Value, json};

use crate::sports::{MatchStatsRecord, SideResultRecord};

/// One logical index.
#[derive(Debug, Clone, Copy)]
pub struct IndexDef {
    /// Logical name, also the read alias (before the deployment prefix).
    pub name: &'static str,
    /// Schema version. The worker writes to `<name>_v<version>`.
    pub version: u32,
    /// Settings + mappings this version is created with.
    pub body: fn() -> Value,
}

impl IndexDef {
    /// The physical index name for this version, e.g. `matches_v1`.
    pub fn physical(&self, prefix: &str) -> String {
        format!("{prefix}{}_v{}", self.name, self.version)
    }

    /// The read alias, e.g. `matches`.
    pub fn alias(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.name)
    }
}

/// Every index the worker keeps in sync, in creation order.
pub fn all_indices() -> [IndexDef; 3] {
    [MATCHES, MATCH_STATS, USERS]
}

/// Single node: one shard (exact aggregations) and no replicas (there's nowhere
/// to put one; a replica would leave the index permanently yellow).
fn settings() -> Value {
    json!({
        "number_of_shards": 1,
        "number_of_replicas": 0,
        "refresh_interval": "1s"
    })
}

/// One document per match. See [`crate::opensearch::docs::MatchDoc`].
pub const MATCHES: IndexDef = IndexDef {
    name: "matches",
    version: 4,
    body: matches_body,
};

fn matches_body() -> Value {
    json!({
        "settings": settings(),
        "mappings": {
            "dynamic": "strict",
            "properties": {
                "match_id": { "type": "keyword" },
                "name": { "type": "text", "fields": { "raw": { "type": "keyword", "ignore_above": 256 } } },
                "sport": { "type": "keyword" },
                "status": { "type": "keyword" },
                "kind": { "type": "keyword" },
                "ranked": { "type": "boolean" },
                "tournament_id": { "type": "keyword" },
                "tournament_round": { "type": "integer" },
                "tournament_slot": { "type": "integer" },
                "series_id": { "type": "keyword" },
                "starts_at": { "type": "date" },
                "year": { "type": "integer" },
                "team_ids": { "type": "keyword" },
                "participant_ids": { "type": "keyword" },
                "roster_ids": { "type": "keyword" },
                "winning_participant_ids": { "type": "keyword" },
                "losing_participant_ids": { "type": "keyword" },
                "drawing_participant_ids": { "type": "keyword" },
                "confirmed": { "type": "boolean" },
                "winner_side_id": { "type": "keyword" },
                "open": { "type": "boolean" },
                "open_has_space": { "type": "boolean" },
                "sides": {
                    "type": "nested",
                    "properties": {
                        "side_id": { "type": "keyword" },
                        "team_id": { "type": "keyword" },
                        "name": { "type": "keyword" },
                        "outcome": { "type": "keyword" },
                        "result": per_sport_mapping(SideResultRecord::all_fields()),
                        "player_ids": { "type": "keyword" },
                        "player_count": { "type": "integer" },
                        "stats": per_sport_mapping(MatchStatsRecord::all_fields())
                    }
                },
                "indexed_at": { "type": "date" }
            }
        }
    })
}

/// One flat row per player per confirmed match. See
/// [`crate::opensearch::docs::PlayerStatDoc`].
///
/// v3: adds `series_id`, so a future series-scoped slice (the per-player
/// analogue of `tournament_id`'s `LeaderboardScope::Tournament`) can filter
/// on it. New field on an already-deployed strict mapping (v2's per-sport
/// stats rework), so this needs its own version bump even though nothing
/// else about the shape changed: `ensure_index` never updates an existing
/// index's mapping, so skipping the bump would repeat the exact
/// `strict_dynamic_mapping_exception` that `matches` hit before it got its
/// own bump.
pub const MATCH_STATS: IndexDef = IndexDef {
    name: "match_stats",
    version: 3,
    body: match_stats_body,
};

fn match_stats_body() -> Value {
    json!({
        "settings": settings(),
        "mappings": {
            "dynamic": "strict",
            "properties": {
                "match_id": { "type": "keyword" },
                "user_id": { "type": "keyword" },
                "side_id": { "type": "keyword" },
                "team_id": { "type": "keyword" },
                "opponent_team_ids": { "type": "keyword" },
                "in_house": { "type": "boolean" },
                "kind": { "type": "keyword" },
                "sport": { "type": "keyword" },
                "ranked": { "type": "boolean" },
                "tournament_id": { "type": "keyword" },
                "tournament_round": { "type": "integer" },
                "series_id": { "type": "keyword" },
                "starts_at": { "type": "date" },
                "year": { "type": "integer" },
                "played": { "type": "integer" },
                "won": { "type": "integer" },
                "drawn": { "type": "integer" },
                "lost": { "type": "integer" },
                "stats": per_sport_mapping(MatchStatsRecord::all_fields()),
                "indexed_at": { "type": "date" }
            }
        }
    })
}

/// A per-sport object ([`MatchStatsRecord`], [`SideResultRecord`]): one
/// sub-object per modeled sport, each with that sport's own fields, all
/// counters.
fn per_sport_mapping(sports: Vec<(&'static str, &'static [&'static str])>) -> Value {
    let sports: serde_json::Map<String, Value> = sports
        .into_iter()
        .map(|(sport, fields)| {
            let counters: serde_json::Map<String, Value> = fields
                .iter()
                .map(|f| (f.to_string(), json!({ "type": "long" })))
                .collect();
            (sport.to_string(), json!({ "properties": counters }))
        })
        .collect();
    json!({ "properties": sports })
}

/// One document per user. See [`crate::opensearch::docs::UserDoc`]. Backs
/// the admin panel's "total users" count and its `GET /admin/users`
/// search/browse (`agon_services_core::opensearch::users`) — the admin
/// panel reads only this, not Meilisearch. `name`/`email` are analyzed text
/// (not just keyword) so a `multi_match` can match a search term as a
/// prefix against either.
pub const USERS: IndexDef = IndexDef {
    name: "users",
    version: 2,
    body: users_body,
};

fn users_body() -> Value {
    json!({
        "settings": settings(),
        "mappings": {
            "dynamic": "strict",
            "properties": {
                "user_id": { "type": "keyword" },
                "name": { "type": "text", "fields": { "raw": { "type": "keyword", "ignore_above": 256 } } },
                "email": { "type": "text", "fields": { "raw": { "type": "keyword", "ignore_above": 256 } } },
                "created_at": { "type": "date" },
                "is_admin": { "type": "boolean" },
                "indexed_at": { "type": "date" }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_and_alias_names_include_prefix_and_version() {
        assert_eq!(MATCHES.physical(""), "matches_v4");
        assert_eq!(MATCHES.alias("staging_"), "staging_matches");
        assert_eq!(MATCH_STATS.physical("x_"), "x_match_stats_v3");
    }

    #[test]
    fn stats_are_mapped_per_sport_with_each_sports_own_counters() {
        let stats = &(MATCH_STATS.body)()["mappings"]["properties"]["stats"]["properties"];
        assert_eq!(stats["football"]["properties"]["assists"]["type"], "long");
        assert_eq!(stats["cricket"]["properties"]["wickets"]["type"], "long");
        assert!(stats["cricket"]["properties"].get("assists").is_none());
        let sides = &(MATCHES.body)()["mappings"]["properties"]["sides"]["properties"];
        assert_eq!(
            sides["stats"]["properties"]["football"]["properties"]["clean_sheets"]["type"],
            "long"
        );
        assert_eq!(
            sides["result"]["properties"]["cricket"]["properties"]["wickets_taken"]["type"],
            "long"
        );
        assert!(sides.get("score").is_none());
    }

    #[test]
    fn every_mapping_is_strict_with_single_shard_settings() {
        for def in all_indices() {
            let body = (def.body)();
            assert_eq!(body["mappings"]["dynamic"], "strict", "{}", def.name);
            assert_eq!(body["settings"]["number_of_shards"], 1, "{}", def.name);
            assert_eq!(body["settings"]["number_of_replicas"], 0, "{}", def.name);
        }
    }
}
