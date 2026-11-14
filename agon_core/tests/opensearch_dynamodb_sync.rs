//! The shadow write's real path, end to end: matches written to DynamoDB
//! (Local) through the DAO, then `opensearch::sync_match` reading them back
//! and bringing OpenSearch in line. `opensearch_roundtrip.rs` covers the
//! documents and queries from in-memory aggregates; this covers the read from
//! DynamoDB, the stale-row delete when a player leaves, and the removal when
//! the match itself is gone.
//!
//! Skipped unless both `AWS_ENDPOINT_URL` (see `common::local_env`) and
//! `OPENSEARCH_URL` are set:
//!
//! ```text
//! docker compose up -d dynamodb-local dynamodb-local-init opensearch
//! make test-opensearch-local
//! ```
//!
//! Uses unique match ids and a random index prefix, and deletes both
//! afterwards.

mod common;

use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{
    ConfirmedScoreRecord, MatchPlayerRecord, MatchRecord, MatchSideRecord, ScoreRecord,
};
use agon_core::opensearch::{
    MATCH_STATS, MATCHES, OpenSearchClient, SyncOutcome, all_indices, sync_match,
};
use aws_sdk_dynamodb::types::AttributeValue;
use common::{TestEnv, local_env};
use serde_json::json;

fn side(id: &str, team: &str) -> MatchSideRecord {
    serde_json::from_value(json!({ "side_id": id, "team_id": team })).unwrap()
}

fn player(pid: &str, user: &str, side: &str) -> MatchPlayerRecord {
    MatchPlayerRecord {
        player_id: pid.into(),
        user_id: Some(user.into()),
        display_name: None,
        side_id: Some(side.into()),
        is_member_of_team: None,
        invitation: None,
        joined_via: None,
    }
}

/// A confirmed football friendly: side `a` (team `hw`) beat side `b` 3-1.
fn friendly(id: &str) -> MatchRecord {
    let mut m: MatchRecord = serde_json::from_value(json!({
        "id": id, "name": id, "description": "", "match_type": "football", "status": "completed",
        "starts_at": "2026-11-08T10:30:00Z", "sides": {}, "created_at": "2026-11-01T00:00:00Z"
    }))
    .unwrap();
    m.sides = [side("a", "hw"), side("b", "leyton")]
        .into_iter()
        .map(|s| (s.side_id.clone(), s))
        .collect();
    m.confirmed_score = Some(ConfirmedScoreRecord {
        score: ScoreRecord::Simple {
            entries: [("a".to_string(), 3), ("b".to_string(), 1)]
                .into_iter()
                .collect(),
        },
        winner_side_id: Some("a".into()),
    });
    m
}

async fn stats_rows(os: &OpenSearchClient, match_id: &str) -> usize {
    os.ids_matching(
        &os.write_index(&MATCH_STATS),
        json!({ "term": { "match_id": match_id } }),
        100,
    )
    .await
    .unwrap()
    .len()
}

async fn match_doc_hits(os: &OpenSearchClient, match_id: &str) -> usize {
    os.ids_matching(
        &os.write_index(&MATCHES),
        json!({ "ids": { "values": [match_id] } }),
        10,
    )
    .await
    .unwrap()
    .len()
}

#[tokio::test]
async fn sync_follows_dynamodb_through_index_roster_change_and_removal() {
    let Ok(url) = std::env::var("OPENSEARCH_URL") else {
        eprintln!("OPENSEARCH_URL unset; skipping");
        return;
    };
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping (DynamoDB Local only)");
        return;
    };
    let prefix = format!("itest_{}_", rand::random::<u32>());
    let os = OpenSearchClient::new(&url, None, prefix.clone()).unwrap();
    for def in all_indices() {
        os.ensure_index(&def).await.unwrap();
    }

    let id = format!(
        "m_ossync_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let now = "2026-11-14T00:00:00Z";
    let players = vec![
        player("p1", "u1", "a"),
        player("p2", "u2", "a"),
        player("p3", "u3", "b"),
    ];
    dao.create_match(&friendly(&id), &players).await.unwrap();

    // 1. Indexed from DynamoDB: the match document plus one row per player.
    assert_eq!(
        sync_match(&os, &dao, &id, now, true).await.unwrap(),
        SyncOutcome::Indexed { player_rows: 3 }
    );
    assert_eq!(match_doc_hits(&os, &id).await, 1);
    assert_eq!(stats_rows(&os, &id).await, 3);

    // 2. A player leaves: the re-sync deletes their now-stale row.
    dao.remove_match_players(&id, &players[2..]).await.unwrap();
    assert_eq!(
        sync_match(&os, &dao, &id, now, true).await.unwrap(),
        SyncOutcome::Indexed { player_rows: 2 }
    );
    assert_eq!(stats_rows(&os, &id).await, 2);

    // 3. The match is gone from DynamoDB: every document of it is removed.
    for sk in [Sk::Meta, Sk::Player("p1".into()), Sk::Player("p2".into())] {
        client
            .delete_item()
            .table_name(&table)
            .key("PK", AttributeValue::S(Pk::Match(id.clone()).to_string()))
            .key("SK", AttributeValue::S(sk.to_string()))
            .send()
            .await
            .unwrap();
    }
    assert_eq!(
        sync_match(&os, &dao, &id, now, true).await.unwrap(),
        SyncOutcome::Removed
    );
    assert_eq!(match_doc_hits(&os, &id).await, 0);
    assert_eq!(stats_rows(&os, &id).await, 0);

    for def in all_indices() {
        os.delete_index(&def.physical(&prefix)).await.unwrap();
    }
}
