//! Guards netball's stored score shape: writes a fully populated netball
//! `MatchScoreRecord` through `Dao::put_match_score` and reads it back through
//! `Dao::get_match_score`, against a real DynamoDB (Local) table rather than
//! an in-process serde round-trip, so it also catches anything `serde_dynamo`'s
//! `AttributeValue` mapping does differently (e.g. the enum-keyed `HashMap`s
//! below). It also checks a hand-written item in the stored JSON shape still
//! deserializes, so a change to the record types can't make existing data
//! unreadable.
//!
//! ## Running locally
//!
//! Needs a real DynamoDB endpoint. Either the project's own local stack:
//! ```sh
//! docker compose up -d dynamodb-local dynamodb-local-init
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 \
//!   AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=localsecret \
//!   cargo test -p agon_core --test netball_score_record_roundtrip
//! ```
//! or a bare `DynamoDBLocal.jar` on the same port, with no docker daemon
//! needed — same env vars, pointed at wherever it's listening. Either way the
//! test creates the `agon` table itself (idempotent) if it doesn't exist yet,
//! so no separate `create-table.sh` run is required.
//!
//! Guarded on `AWS_ENDPOINT_URL` being set, so a plain `cargo test --workspace`
//! run (no local stack, no env override) never risks touching a real table.

mod common;

use std::collections::HashMap;

use agon_core::dao::records::{MatchScoreRecord, ScoreRecord};
use agon_core::sports::netball::{
    NetballFoulEventRecord, NetballFoulKindRecord, NetballGoalEventRecord, NetballPeriodRecord,
    NetballPositionRecord, NetballScoreRecord,
};
use common::{TestEnv, local_env};

#[tokio::test]
async fn netball_score_record_round_trips_through_dynamodb() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!(
            "skipping: set AWS_ENDPOINT_URL (e.g. http://localhost:8002) to run this test \
             against DynamoDB Local — see this file's header for setup."
        );
        return;
    };

    let match_id = format!(
        "test-netball-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let mut score = HashMap::new();
    score.insert("side_a".to_string(), 12u32);
    score.insert("side_b".to_string(), 9u32);

    let mut period_times = HashMap::new();
    period_times.insert(
        NetballPeriodRecord::Start,
        "2026-01-10T10:00:00Z".to_string(),
    );
    period_times.insert(
        NetballPeriodRecord::QuarterOneEnd,
        "2026-01-10T10:15:00Z".to_string(),
    );

    let mut q1_score = HashMap::new();
    q1_score.insert("side_a".to_string(), 5u32);
    q1_score.insert("side_b".to_string(), 4u32);
    let mut period_scores = HashMap::new();
    period_scores.insert(NetballPeriodRecord::QuarterOneEnd, q1_score);

    let record = MatchScoreRecord {
        sport: "netball".to_string(),
        score: ScoreRecord::Netball(NetballScoreRecord {
            score,
            goals: Some(vec![
                NetballGoalEventRecord {
                    side_id: "side_a".to_string(),
                    scorer_player_id: Some("p1".to_string()),
                    scorer_position: Some(NetballPositionRecord::GoalShooter),
                    two_points: false,
                    minute: Some(3),
                    occurred_at: Some("2026-01-10T10:03:00Z".to_string()),
                },
                NetballGoalEventRecord {
                    side_id: "side_b".to_string(),
                    scorer_player_id: None,
                    scorer_position: None,
                    two_points: true,
                    minute: None,
                    occurred_at: None,
                },
            ]),
            fouls: Some(vec![NetballFoulEventRecord {
                side_id: "side_a".to_string(),
                player_id: Some("p2".to_string()),
                foul_kind: NetballFoulKindRecord::Contact,
                minute: Some(7),
                occurred_at: None,
            }]),
            period: Some(NetballPeriodRecord::QuarterOneEnd),
            period_times: Some(period_times),
            period_scores: Some(period_scores),
        }),
        last_seq: Some(5),
    };

    dao.put_match_score(&match_id, &record)
        .await
        .expect("put_match_score");

    let fetched = dao
        .get_match_score(&match_id, "netball")
        .await
        .expect("get_match_score")
        .expect("record was written");

    assert_eq!(fetched, record, "round-tripped record must match exactly");

    // Also exercise the "no live detail" (manually-entered, bare tally)
    // shape — every optional field `None`, since that's the other end of
    // what production actually stores for this variant.
    let bare_match_id = format!("{match_id}-bare");
    let mut bare_score = HashMap::new();
    bare_score.insert("side_a".to_string(), 20u32);
    bare_score.insert("side_b".to_string(), 18u32);
    let bare_record = MatchScoreRecord {
        sport: "netball".to_string(),
        score: ScoreRecord::Netball(NetballScoreRecord {
            score: bare_score,
            goals: None,
            fouls: None,
            period: None,
            period_times: None,
            period_scores: None,
        }),
        last_seq: None,
    };
    dao.put_match_score(&bare_match_id, &bare_record)
        .await
        .expect("put_match_score (bare)");
    let fetched_bare = dao
        .get_match_score(&bare_match_id, "netball")
        .await
        .expect("get_match_score (bare)")
        .expect("bare record was written");
    assert_eq!(fetched_bare, bare_record);
}

/// Writes the stored wire shape directly as hand-built JSON, independent of
/// any Rust type, and confirms the code reads it back. The round-trip test
/// above only proves the code agrees with itself; this pins the shape items
/// already in the table were written in.
#[tokio::test]
async fn stored_netball_score_json_still_deserializes() {
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("skipping: see this file's header for setup.");
        return;
    };

    let match_id = format!(
        "test-netball-stored-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    // The JSON a netball score is stored as, tag included.
    let stored_record_json = serde_json::json!({
        "sport": "netball",
        "score": {
            "type": "netball",
            "score": {"side_a": 12, "side_b": 9},
            "goals": [
                {
                    "side_id": "side_a",
                    "scorer_player_id": "p1",
                    "scorer_position": "goal_shooter",
                    "two_points": false,
                    "minute": 3,
                    "occurred_at": "2026-01-10T10:03:00Z"
                }
            ],
            "period": "quarter_one_end",
            "period_times": {"start": "2026-01-10T10:00:00Z"}
        },
        "last_seq": 3
    });

    use agon_core::dao::item::to_item;
    use agon_core::dao::keys::{Pk, Sk};
    use agon_core::dao::match_ops::TYPE_MATCH_SCORE;

    let item = to_item(
        &Pk::Match(match_id.clone()),
        &Sk::Score("netball".to_string()),
        TYPE_MATCH_SCORE,
        &stored_record_json,
    )
    .expect("build item from stored JSON shape");

    client
        .put_item()
        .table_name(&table)
        .set_item(Some(item))
        .send()
        .await
        .expect("put stored item");

    let fetched = dao
        .get_match_score(&match_id, "netball")
        .await
        .expect("get_match_score")
        .expect("stored record was written");

    let ScoreRecord::Netball(score) = &fetched.score else {
        panic!("expected ScoreRecord::Netball, got {:?}", fetched.score);
    };
    assert_eq!(score.score.get("side_a"), Some(&12));
    assert_eq!(score.score.get("side_b"), Some(&9));
    assert_eq!(score.goals.as_ref().unwrap().len(), 1);
    assert_eq!(score.goals.as_ref().unwrap()[0].side_id, "side_a");
    assert_eq!(fetched.last_seq, Some(3));
}
