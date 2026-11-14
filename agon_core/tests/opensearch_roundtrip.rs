//! Round trip against a real OpenSearch: create the indices, write two matches'
//! documents, and run the two query shapes the team pages will use (a
//! leaderboard over `match_stats`, a team record over the nested `matches`
//! sides). Uses a random index prefix and deletes its indices afterwards.
//!
//! Skipped unless `OPENSEARCH_URL` is set:
//!
//! ```text
//! docker compose up -d opensearch
//! OPENSEARCH_URL=http://localhost:9200 cargo test -p agon_core --test opensearch_roundtrip -- --nocapture
//! ```

use agon_core::dao::match_ops::MatchAggregate;
use agon_core::dao::records::{
    ConfirmedScoreRecord, MatchPlayerRecord, MatchRecord, MatchSideRecord, ScoreRecord,
};
use agon_core::opensearch::docs::player_doc_id;
use agon_core::opensearch::{
    BulkOp, MATCH_STATS, MATCHES, OpenSearchClient, all_indices, build_match_docs,
};
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

fn confirmed(
    id: &str,
    sides: Vec<MatchSideRecord>,
    players: Vec<MatchPlayerRecord>,
    pts: &[(&str, u32)],
    winner: Option<&str>,
) -> MatchAggregate {
    let mut m: MatchRecord = serde_json::from_value(json!({
        "id": id, "name": id, "description": "", "match_type": "football", "status": "completed",
        "starts_at": "2026-11-08T10:30:00Z", "sides": {}, "created_at": "2026-11-01T00:00:00Z"
    }))
    .unwrap();
    m.sides = sides
        .iter()
        .map(|s| (s.side_id.clone(), s.clone()))
        .collect();
    m.confirmed_score = Some(ConfirmedScoreRecord {
        score: ScoreRecord::Simple {
            entries: pts.iter().map(|(s, p)| (s.to_string(), *p)).collect(),
        },
        winner_side_id: winner.map(Into::into),
    });
    MatchAggregate {
        match_: m,
        sides,
        players,
    }
}

#[tokio::test]
async fn index_and_aggregate_round_trip() {
    let Ok(url) = std::env::var("OPENSEARCH_URL") else {
        eprintln!("OPENSEARCH_URL unset; skipping");
        return;
    };
    let prefix = format!("itest_{}_", rand::random::<u32>());
    let os = OpenSearchClient::new(&url, None, prefix.clone()).unwrap();

    for def in all_indices() {
        assert!(os.ensure_index(&def).await.unwrap(), "fresh index created");
        assert_eq!(
            os.alias_targets(&def.alias(&prefix)).await.unwrap(),
            vec![def.physical(&prefix)]
        );
    }

    // A friendly HW 3-1 Leyton, and an HW squad game drawn 2-2.
    let friendly = confirmed(
        "m_friendly",
        vec![side("a", "hw"), side("b", "leyton")],
        vec![
            player("p1", "u1", "a"),
            player("p2", "u2", "a"),
            player("p3", "u3", "b"),
        ],
        &[("a", 3), ("b", 1)],
        Some("a"),
    );
    let squad = confirmed(
        "m_squad",
        vec![side("l", "hw"), side("d", "hw")],
        vec![player("p1", "u1", "l"), player("p2", "u2", "d")],
        &[("l", 2), ("d", 2)],
        None,
    );
    let mut ops = Vec::new();
    for agg in [&friendly, &squad] {
        let docs = build_match_docs(agg, None, "2026-11-14T00:00:00Z");
        ops.push(
            BulkOp::index(
                &os.write_index(&MATCHES),
                &docs.match_doc.match_id,
                &docs.match_doc,
            )
            .unwrap(),
        );
        for row in &docs.players {
            ops.push(
                BulkOp::index(
                    &os.write_index(&MATCH_STATS),
                    &player_doc_id(&row.match_id, &row.user_id),
                    row,
                )
                .unwrap(),
            );
        }
    }
    os.bulk_with(&ops, true).await.unwrap();

    // Leaderboard: HW players, all games. u1 played both (won the friendly, drew the squad game).
    let lb = os
        .search(
            &MATCH_STATS.alias(&prefix),
            json!({
                "size": 0,
                "query": { "bool": { "filter": [{ "term": { "team_id": "hw" } }] } },
                "aggs": { "players": {
                    "terms": { "field": "user_id", "size": 500 },
                    "aggs": { "played": { "sum": { "field": "played" } }, "won": { "sum": { "field": "won" } } }
                } }
            }),
        )
        .await
        .unwrap();
    let buckets = lb["aggregations"]["players"]["buckets"]
        .as_array()
        .unwrap()
        .clone();
    let u1 = buckets
        .iter()
        .find(|b| b["key"] == "u1")
        .expect("u1 bucket");
    assert_eq!(u1["played"]["value"], 2.0);
    assert_eq!(u1["won"]["value"], 1.0);
    assert!(
        buckets.iter().all(|b| b["key"] != "u3"),
        "Leyton player not on the HW board"
    );

    // Team record: HW's side in non-squad matches. One win, 3 for, 1 against.
    let rec = os
        .search(
            &MATCHES.alias(&prefix),
            json!({
                "size": 0,
                "query": { "bool": {
                    "filter": [{ "term": { "team_ids": "hw" } }, { "term": { "confirmed": true } }],
                    "must_not": [{ "term": { "kind": "squad" } }]
                } },
                "aggs": { "sides": {
                    "nested": { "path": "sides" },
                    "aggs": { "ours": {
                        "filter": { "term": { "sides.team_id": "hw" } },
                        "aggs": {
                            "outcome": { "terms": { "field": "sides.outcome" } },
                            "gf": { "sum": { "field": "sides.score" } },
                            "ga": { "sum": { "field": "sides.conceded" } }
                        }
                    } }
                } }
            }),
        )
        .await
        .unwrap();
    let ours = &rec["aggregations"]["sides"]["ours"];
    assert_eq!(
        ours["doc_count"], 1,
        "only the friendly counts towards the record"
    );
    assert_eq!(ours["gf"]["value"], 3.0);
    assert_eq!(ours["ga"]["value"], 1.0);
    assert_eq!(ours["outcome"]["buckets"][0]["key"], "won");

    // Stale-row lookup used by sync: both of the squad game's rows are found.
    let ids = os
        .ids_matching(
            &os.write_index(&MATCH_STATS),
            json!({ "term": { "match_id": "m_squad" } }),
            100,
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 2);

    for def in all_indices() {
        os.delete_index(&def.physical(&prefix)).await.unwrap();
    }
}
