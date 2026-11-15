//! The entity index (GSI4) against DynamoDB Local: a match created through the
//! DAO is listed straight away, newest first, and an item written without index
//! keys (as every item was before the index existed) is not listed. The index
//! is sparse: only items carrying `GSI4PK` appear.
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

use agon_services_core::dao::entity_index::EntityKind;
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
async fn created_matches_are_listed_newest_first() {
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let older = format!("eidx_older_{run}");
    let newer = format!("eidx_newer_{run}");
    let legacy = format!("eidx_legacy_{run}");

    dao.create_match(&match_record(&older, "2026-11-01T12:00:00Z"), &[])
        .await
        .unwrap();
    dao.create_match(&match_record(&newer, "2026-11-15T12:00:00Z"), &[])
        .await
        .unwrap();
    // An item without index keys, the way items written before the index look.
    client
        .put_item()
        .table_name(&table)
        .item("PK", AttributeValue::S(format!("MATCH#{legacy}")))
        .item("SK", AttributeValue::S("#META".into()))
        .item("type", AttributeValue::S("match".into()))
        .item(
            "created_at",
            AttributeValue::S("2025-01-01T00:00:00Z".into()),
        )
        .send()
        .await
        .unwrap();

    let listed = all_listed(&dao).await;
    let pos = |id: &str| listed.iter().position(|x| x == id);
    assert!(pos(&newer).unwrap() < pos(&older).unwrap(), "newest first");
    assert_eq!(pos(&legacy), None, "items without index keys aren't listed");

    for id in [&older, &newer, &legacy] {
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
