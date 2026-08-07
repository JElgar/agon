//! One-off migration: move each match's sides off separate `SIDE#` items and
//! onto its `#META` record as the embedded `sides` map (see
//! `MatchRecord::sides`'s doc comment for why). Written for the transition
//! after that schema change landed — matches created before it still have
//! their sides as standalone `SIDE#` items and nothing else in the DAO reads
//! those anymore, so they need moving once, by hand, against the real table.
//! Not meant to stay useful forever; kept as a DAO method (rather than a
//! throwaway script hand-rolling its own Dynamo calls) purely so it can reuse
//! the same key/item helpers as everything else here. Driven by the
//! `migrate_sides` binary — see `src/bin/migrate_sides.rs`.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, ATTR_TYPE, Item, item_pk, s};
use super::keys::{Pk, Sk};
use super::match_ops::TYPE_MATCH;
use super::records::MatchSideRecord;

/// Tally of what [`Dao::migrate_sides`] did (or, in dry-run mode, would do),
/// one match id per bucket.
#[derive(Debug, Default)]
pub struct MigrationSummary {
    /// Had `SIDE#` items with no (or an empty) `sides` map yet — embedded.
    pub migrated: Vec<String>,
    /// Already had a populated `sides` map — left untouched.
    pub already_migrated: Vec<String>,
    /// No `SIDE#` items found for a match with no `sides` map either — logged
    /// rather than treated as an error, since it shouldn't block the rest of
    /// the run, but it's worth a human looking at (every real match should
    /// have at least one side).
    pub no_sides: Vec<String>,
}

impl Dao {
    /// Scan the table for every match, and for any that predates the
    /// embedded-`sides` change, read its `SIDE#` items and write them onto
    /// the `#META` record's `sides` map. Idempotent: a match that already has
    /// a populated `sides` map is left alone, so re-running after a partial
    /// or interrupted pass is safe.
    ///
    /// `dry_run` reports what *would* change without writing anything.
    /// `delete_old_items` additionally deletes the old `SIDE#` items once
    /// their data is embedded (ignored under `dry_run`) — run once without it
    /// first to confirm the embedded data looks right before cleaning up.
    ///
    /// This is the one place in the DAO that does a full-table `Scan` — every
    /// other read is a `Query`/`GetItem`/`BatchGetItem` scoped to a known
    /// partition or key set. That's fine for a migration meant to run once,
    /// locally, against the real table; nothing else in the service does
    /// this, and it isn't meant to run again after the rollout completes.
    pub async fn migrate_sides(
        &self,
        dry_run: bool,
        delete_old_items: bool,
    ) -> DaoResult<MigrationSummary> {
        let mut summary = MigrationSummary::default();

        for item in self.scan_match_metas().await? {
            let Pk::Match(match_id) = item_pk(&item)? else {
                return Err(DaoError::Malformed(
                    "scan_match_metas returned a non-match item".into(),
                ));
            };

            if has_populated_sides(&item) {
                summary.already_migrated.push(match_id);
                continue;
            }

            let old_sides: Vec<MatchSideRecord> = self
                .query_match_collection(&match_id, &Sk::side_prefix())
                .await?;
            if old_sides.is_empty() {
                summary.no_sides.push(match_id);
                continue;
            }

            if !dry_run {
                let sides_map: HashMap<String, MatchSideRecord> = old_sides
                    .iter()
                    .cloned()
                    .map(|side| (side.side_id.clone(), side))
                    .collect();

                self.client
                    .update_item()
                    .table_name(self.table())
                    .key(ATTR_PK, s(Pk::Match(match_id.clone()).to_string()))
                    .key(ATTR_SK, s(Sk::Meta.to_string()))
                    .update_expression("SET sides = :sides")
                    .expression_attribute_values(":sides", to_attr(&sides_map)?)
                    .send()
                    .await
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;

                if delete_old_items {
                    for side in &old_sides {
                        self.client
                            .delete_item()
                            .table_name(self.table())
                            .key(ATTR_PK, s(Pk::Match(match_id.clone()).to_string()))
                            .key(ATTR_SK, s(Sk::Side(side.side_id.clone()).to_string()))
                            .send()
                            .await
                            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                    }
                }
            }

            summary.migrated.push(match_id);
        }

        Ok(summary)
    }

    /// Every match `#META` item in the table, draining all scan pages.
    async fn scan_match_metas(&self) -> DaoResult<Vec<Item>> {
        let mut items = Vec::new();
        let mut start_key = None;
        loop {
            let out = self
                .client
                .scan()
                .table_name(self.table())
                .filter_expression("#sk = :meta AND #type = :t")
                .expression_attribute_names("#sk", ATTR_SK)
                .expression_attribute_names("#type", ATTR_TYPE)
                .expression_attribute_values(":meta", s(Sk::Meta.to_string()))
                .expression_attribute_values(":t", s(TYPE_MATCH))
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            items.extend(out.items.unwrap_or_default());

            match out.last_evaluated_key {
                Some(k) => start_key = Some(k),
                None => break,
            }
        }
        Ok(items)
    }
}

/// True if a scanned match item already has a non-empty `sides` map — the
/// "already migrated" check. A missing attribute or an empty map (matches
/// created between the code deploying and this migration running, before
/// `create_match` started populating it — shouldn't happen given deploy
/// ordering, but tolerated) both count as not-yet-migrated.
fn has_populated_sides(item: &Item) -> bool {
    matches!(item.get("sides"), Some(AttributeValue::M(m)) if !m.is_empty())
}

fn to_attr<T: serde::Serialize>(value: &T) -> DaoResult<AttributeValue> {
    Ok(serde_dynamo::to_attribute_value(value)?)
}
