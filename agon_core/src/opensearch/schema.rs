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
pub fn all_indices() -> [IndexDef; 2] {
    [MATCHES, MATCH_STATS]
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
    version: 1,
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
                "series_id": { "type": "keyword" },
                "starts_at": { "type": "date" },
                "year": { "type": "integer" },
                "team_ids": { "type": "keyword" },
                "participant_ids": { "type": "keyword" },
                "confirmed": { "type": "boolean" },
                "winner_side_id": { "type": "keyword" },
                "sides": {
                    "type": "nested",
                    "properties": {
                        "side_id": { "type": "keyword" },
                        "team_id": { "type": "keyword" },
                        "name": { "type": "keyword" },
                        "outcome": { "type": "keyword" },
                        "score": { "type": "integer" },
                        "conceded": { "type": "integer" },
                        "clean_sheet": { "type": "boolean" },
                        "player_ids": { "type": "keyword" },
                        "player_count": { "type": "integer" }
                    }
                },
                "indexed_at": { "type": "date" }
            }
        }
    })
}

/// One flat row per player per confirmed match. See
/// [`crate::opensearch::docs::PlayerStatDoc`].
pub const MATCH_STATS: IndexDef = IndexDef {
    name: "match_stats",
    version: 1,
    body: match_stats_body,
};

fn match_stats_body() -> Value {
    json!({
        "settings": settings(),
        "mappings": {
            "dynamic": "strict",
            // Sport counters (goals, assists, runs, wickets, ...) vary by sport
            // and grow over time, so `stats` takes any key, always as a long.
            "dynamic_templates": [
                { "stat_counters": { "path_match": "stats.*", "mapping": { "type": "long" } } }
            ],
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
                "starts_at": { "type": "date" },
                "year": { "type": "integer" },
                "played": { "type": "integer" },
                "won": { "type": "integer" },
                "drawn": { "type": "integer" },
                "lost": { "type": "integer" },
                "stats": { "type": "object", "dynamic": true },
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
        assert_eq!(MATCHES.physical(""), "matches_v1");
        assert_eq!(MATCHES.alias("staging_"), "staging_matches");
        assert_eq!(MATCH_STATS.physical("x_"), "x_match_stats_v1");
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
