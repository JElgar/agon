//! One-off backfill: rewrite `Score::Simple`/`Score::Sets` records still
//! stored in the pre-migration `entries: Vec<{side_id, ...}>` shape into the
//! current side_id-keyed map shape (see `records::de_simple_entries`/
//! `de_sets_entries`, the compat deserializer that lets both shapes still be
//! *read* — this only rewrites storage, so a fresh scan stops finding any
//! left and that shim can eventually be deleted).
//!
//! Touches the only two item kinds that ever carry a `ScoreRecord`:
//!
//! - `MATCH#<id>` / `#META` — `confirmed_score`/`pending_score`
//! - `MATCH#<id>` / `SCORESUB#<ts>#<subId>` — `score` (submission history)
//!
//! There's no index listing "every match" or "every submission" in this
//! single-table design, so finding candidates means a full-table `Scan`
//! (filtered server-side to just those two SK shapes to cut response size,
//! though not RCU cost — DynamoDB filters after paying for the read either
//! way).
//!
//! Each attribute is rewritten with a guarded `UpdateItem`
//! (`ConditionExpression` on the exact value just read), never a blind
//! whole-item `PutItem` — so a concurrent write to something else on the same
//! item (a new like, a new response landing on a pending submission) can't be
//! clobbered. A lost race is simply skipped and picked up by the next run.
//! Idempotent and safe to re-run: an item already in the new shape is left
//! untouched, so re-running after a full pass should find nothing left to do.

use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, Item, item_sk, s};
use super::keys::Sk;
use super::match_ops::{is_update_conditional_failure, to_attr};
use super::records::{ConfirmedScoreRecord, PendingScoreRecord, ScoreRecord};

/// Tallies from one pass. A `dry_run` pass only ever populates `items_scanned`
/// and `needs_migration`; a real pass also populates `migrated`/`conflicted`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScoreEntriesMigrationStats {
    /// `#META`/`SCORESUB#` items examined.
    pub items_scanned: u64,
    /// Score-bearing attributes found in the legacy `Vec` shape.
    pub needs_migration: u64,
    /// Successfully rewritten to the map shape.
    pub migrated: u64,
    /// Lost an optimistic-lock race (another write landed between the read
    /// and this write) — re-run to pick these up.
    pub conflicted: u64,
}

impl Dao {
    /// Scan the whole table for `#META` and `SCORESUB#*` items whose
    /// `confirmed_score`/`pending_score`/`score` is still in the legacy `Vec`
    /// shape, and rewrite them to the current map shape. Pass `dry_run: true`
    /// to only count what *would* change.
    pub async fn migrate_legacy_score_entries(
        &self,
        dry_run: bool,
    ) -> DaoResult<ScoreEntriesMigrationStats> {
        let mut stats = ScoreEntriesMigrationStats::default();
        let mut start_key = None;

        loop {
            let out = self
                .client
                .scan()
                .table_name(self.table())
                .filter_expression("SK = :meta OR begins_with(SK, :scoresub)")
                .expression_attribute_values(":meta", s(Sk::Meta.to_string()))
                .expression_attribute_values(":scoresub", s(Sk::score_submission_prefix()))
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            for item in out.items.unwrap_or_default() {
                stats.items_scanned += 1;
                // A row that fails to parse here isn't this migration's
                // problem to fix; skip rather than abort the whole pass.
                let Ok(sk) = item_sk(&item) else { continue };
                match sk {
                    Sk::Meta => {
                        self.migrate_score_attr(&item, "confirmed_score", dry_run, &mut stats)
                            .await?;
                        self.migrate_score_attr(&item, "pending_score", dry_run, &mut stats)
                            .await?;
                    }
                    Sk::ScoreSubmission(_) => {
                        self.migrate_score_attr(&item, "score", dry_run, &mut stats)
                            .await?;
                    }
                    _ => {}
                }
            }

            start_key = out.last_evaluated_key;
            if start_key.is_none() {
                break;
            }
        }

        Ok(stats)
    }

    /// Rewrite one score-bearing attribute if it's still in the legacy
    /// `entries: Vec<...>` shape. A no-op if the attribute is absent (e.g. no
    /// `pending_score`) or already the current shape.
    async fn migrate_score_attr(
        &self,
        item: &Item,
        attr: &str,
        dry_run: bool,
        stats: &mut ScoreEntriesMigrationStats,
    ) -> DaoResult<()> {
        let Some(old) = item.get(attr) else {
            return Ok(());
        };
        if !has_legacy_entries_shape(attr, old) {
            return Ok(());
        }
        stats.needs_migration += 1;
        if dry_run {
            return Ok(());
        }

        // Round-trip through the current record types: the compat
        // deserializer normalizes the legacy Vec shape to a map on the way
        // in, and the serializer only ever writes the map shape on the way
        // back out.
        let new = re_encode_score_attr(attr, old)?;

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, item[ATTR_PK].clone())
            .key(ATTR_SK, item[ATTR_SK].clone())
            .update_expression("SET #attr = :new")
            .condition_expression("#attr = :old")
            .expression_attribute_names("#attr", attr)
            .expression_attribute_values(":new", new)
            .expression_attribute_values(":old", old.clone())
            .send()
            .await;

        match result {
            Ok(_) => stats.migrated += 1,
            Err(e) if is_update_conditional_failure(&e) => stats.conflicted += 1,
            Err(e) => return Err(DaoError::Dynamo(e.to_string())),
        }
        Ok(())
    }
}

/// The nested `ScoreRecord`-shaped `AttributeValue` within a score-bearing
/// attribute: `score` (submission history) *is* the `ScoreRecord`, while
/// `confirmed_score`/`pending_score` wrap it one level down under `.score`.
fn nested_score_av<'a>(attr: &str, wrapper: &'a AttributeValue) -> Option<&'a AttributeValue> {
    match attr {
        "score" => Some(wrapper),
        "confirmed_score" | "pending_score" => match wrapper {
            AttributeValue::M(m) => m.get("score"),
            _ => None,
        },
        _ => None,
    }
}

/// True if the score-bearing attribute's `entries` field is stored as a
/// DynamoDB List (the pre-migration `Vec<{side_id, ...}>` shape) rather than
/// a Map. Only `Simple`/`Sets` ever had `entries`; anything else (including
/// an already-migrated Map) is left alone.
fn has_legacy_entries_shape(attr: &str, wrapper: &AttributeValue) -> bool {
    let Some(AttributeValue::M(score_m)) = nested_score_av(attr, wrapper) else {
        return false;
    };
    let Some(AttributeValue::S(ty)) = score_m.get("type") else {
        return false;
    };
    if ty != "simple" && ty != "sets" {
        return false;
    }
    matches!(score_m.get("entries"), Some(AttributeValue::L(_)))
}

/// Deserialize a score-bearing attribute through its record type (accepting
/// either shape) and re-serialize it (always producing the current shape).
fn re_encode_score_attr(attr: &str, old: &AttributeValue) -> DaoResult<AttributeValue> {
    Ok(match attr {
        "confirmed_score" => {
            let rec: ConfirmedScoreRecord = serde_dynamo::from_attribute_value(old.clone())?;
            to_attr(&rec)?
        }
        "pending_score" => {
            let rec: PendingScoreRecord = serde_dynamo::from_attribute_value(old.clone())?;
            to_attr(&rec)?
        }
        "score" => {
            let rec: ScoreRecord = serde_dynamo::from_attribute_value(old.clone())?;
            to_attr(&rec)?
        }
        _ => unreachable!("migrate_score_attr only calls this with known attribute names"),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn legacy_simple_confirmed_score() -> AttributeValue {
        AttributeValue::M(HashMap::from([(
            "score".to_string(),
            AttributeValue::M(HashMap::from([
                ("type".to_string(), AttributeValue::S("simple".into())),
                (
                    "entries".to_string(),
                    AttributeValue::L(vec![AttributeValue::M(HashMap::from([
                        ("side_id".to_string(), AttributeValue::S("sideA".into())),
                        ("points".to_string(), AttributeValue::N("6".into())),
                    ]))]),
                ),
            ])),
        )]))
    }

    #[test]
    fn detects_legacy_shape_on_confirmed_score() {
        assert!(has_legacy_entries_shape(
            "confirmed_score",
            &legacy_simple_confirmed_score()
        ));
    }

    #[test]
    fn does_not_flag_an_already_migrated_map_shape() {
        let current = AttributeValue::M(HashMap::from([(
            "score".to_string(),
            AttributeValue::M(HashMap::from([
                ("type".to_string(), AttributeValue::S("simple".into())),
                (
                    "entries".to_string(),
                    AttributeValue::M(HashMap::from([(
                        "sideA".to_string(),
                        AttributeValue::N("6".into()),
                    )])),
                ),
            ])),
        )]));
        assert!(!has_legacy_entries_shape("confirmed_score", &current));
    }

    #[test]
    fn does_not_flag_cricket_or_football_scores() {
        let cricket = AttributeValue::M(HashMap::from([(
            "score".to_string(),
            AttributeValue::M(HashMap::from([
                ("type".to_string(), AttributeValue::S("cricket".into())),
                ("innings".to_string(), AttributeValue::L(vec![])),
            ])),
        )]));
        assert!(!has_legacy_entries_shape("confirmed_score", &cricket));
    }

    #[test]
    fn does_not_flag_a_non_map_value() {
        // Defensive: an attribute that isn't an M at all (shouldn't happen for
        // confirmed_score/pending_score/score, but shape-checking must not
        // panic on it) is just not a legacy match.
        assert!(!has_legacy_entries_shape(
            "confirmed_score",
            &AttributeValue::Null(true)
        ));
    }

    #[test]
    fn re_encodes_a_legacy_confirmed_score_to_the_map_shape() {
        let new = re_encode_score_attr("confirmed_score", &legacy_simple_confirmed_score())
            .expect("re-encode");
        assert!(!has_legacy_entries_shape("confirmed_score", &new));
        let rec: ConfirmedScoreRecord =
            serde_dynamo::from_attribute_value(new).expect("deserialize migrated value");
        match rec.score {
            ScoreRecord::Simple { entries } => assert_eq!(entries.get("sideA"), Some(&6)),
            _ => panic!("expected simple"),
        }
    }

    #[test]
    fn re_encoding_an_already_current_score_is_a_no_op_shape() {
        let current: AttributeValue = to_attr(&ScoreRecord::Simple {
            entries: HashMap::from([("sideA".to_string(), 6)]),
        })
        .unwrap();
        let re_encoded = re_encode_score_attr("score", &current).expect("re-encode");
        assert_eq!(current, re_encoded);
    }
}
