//! The entity index (GSI4) against DynamoDB Local:
//! - a match created through the DAO is listed straight away;
//! - a legacy match item (written without index keys, as every item was before
//!   the index existed) is found by the migration scan, gets its keys once, and
//!   is then listed too.
//!
//! Skipped unless `AWS_ENDPOINT_URL` is set (see `common::local_env`), so it
//! never touches a real table:
//!
//! ```text
//! docker compose up -d dynamodb-local dynamodb-local-init
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret cargo test -p agon_services_core --test entity_index_roundtrip
//! ```

mod common;

use agon_services_core::dao::entity_index::{EntityKind, MissingEntityKeys};
use agon_services_core::dao::records::MatchRecord;
use aws_sdk_dynamodb::types::AttributeValue;
use common::{TestEnv, local_env};
use serde_json::json;

fn match_record(id: &str, created_at: &str) -> MatchRecord {
    serde_json::from_value(json!({
        "id": id, "name": id, "description": "", "match_type": "tennis",
        "status": "scheduled", "starts_at": "2026-11-20T10:00:00Z", "sides": {},
        "created_at": created_at
    }))
    .unwrap()
}

/// Page through every listed match id, newest first.
async fn all_listed(dao: &agon_services_core::dao::Dao) -> Vec<String> {
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = dao
            .list_entity_ids(EntityKind::Match, true, cursor.as_deref(), 100)
            .await
            .unwrap();
        ids.extend(page.items);
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return ids,
        }
    }
}

#[tokio::test]
async fn created_and_migrated_matches_are_listed() {
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let created_id = format!("eidx_new_{run}");
    let legacy_id = format!("eidx_old_{run}");

    // Created through the DAO: indexed on write.
    dao.create_match(&match_record(&created_id, "2026-11-15T12:00:00Z"), &[])
        .await
        .unwrap();
    assert!(all_listed(&dao).await.contains(&created_id));

    // A legacy item with no index keys: written raw, the way old items look.
    client
        .put_item()
        .table_name(&table)
        .item("PK", AttributeValue::S(format!("MATCH#{legacy_id}")))
        .item("SK", AttributeValue::S("#META".into()))
        .item("type", AttributeValue::S("match".into()))
        .item("id", AttributeValue::S(legacy_id.clone()))
        .item(
            "created_at",
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        )
        .send()
        .await
        .unwrap();
    assert!(!all_listed(&dao).await.contains(&legacy_id));

    // The migration scan finds it (and not the already-indexed match).
    let mut missing: Vec<MissingEntityKeys> = Vec::new();
    let mut cursor = None;
    loop {
        let page = dao
            .scan_entity_index_missing(cursor.as_deref(), 500)
            .await
            .unwrap();
        missing.extend(page.items);
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let legacy = missing
        .iter()
        .find(|m| m.id == legacy_id)
        .expect("legacy match found by the migration scan")
        .clone();
    assert_eq!(legacy.kind, EntityKind::Match);
    assert_eq!(legacy.created_at, "2025-01-01T00:00:00Z");
    assert!(missing.iter().all(|m| m.id != created_id));

    // Set once; a second run is a no-op.
    assert!(dao.set_entity_index_keys(&legacy).await.unwrap());
    assert!(!dao.set_entity_index_keys(&legacy).await.unwrap());

    // Now listed, and the newer match comes before it (newest first).
    let listed = all_listed(&dao).await;
    let pos = |id: &str| listed.iter().position(|x| x == id).unwrap();
    assert!(pos(&created_id) < pos(&legacy_id));

    // The migration never creates an item that isn't there.
    let ghost = MissingEntityKeys {
        kind: EntityKind::Match,
        id: format!("eidx_ghost_{run}"),
        created_at: String::new(),
    };
    assert!(!dao.set_entity_index_keys(&ghost).await.unwrap());

    for id in [&created_id, &legacy_id] {
        client
            .delete_item()
            .table_name(&table)
            .key("PK", AttributeValue::S(format!("MATCH#{id}")))
            .key("SK", AttributeValue::S("#META".into()))
            .send()
            .await
            .unwrap();
    }
}
