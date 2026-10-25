//! Regression guard for the cricket `ScoreRecord` shape refactor — see
//! `netball_score_record_roundtrip.rs`'s doc comment for what these tests
//! prove, why, and how to run them locally against DynamoDB Local. Same
//! structure, cricket's shapes (the biggest of the three — full innings,
//! batting/bowling entries, fall of wickets, deliveries, next-ball context).

mod common;

use agon_core::dao::records::{MatchScoreRecord, ScoreRecord};
use agon_core::sports::cricket::{
    CricketBattingEntryRecord, CricketBowlingEntryRecord, CricketDeliveryRecord,
    CricketDismissalKindRecord, CricketDismissalRecord, CricketExtrasRecord,
    CricketFallOfWicketRecord, CricketScoreInningsRecord, CricketScoreRecord,
    NextBallContextRecord, OversRecord,
};
use common::{TestEnv, local_env};

#[tokio::test]
async fn cricket_score_record_round_trips_through_dynamodb() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!(
            "skipping: set AWS_ENDPOINT_URL (e.g. http://localhost:8002) to run this test \
             against DynamoDB Local — see netball_score_record_roundtrip.rs's header for setup."
        );
        return;
    };

    let match_id = format!(
        "test-cricket-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let first_innings = CricketScoreInningsRecord {
        batting_side_id: "side_a".to_string(),
        bowling_side_id: "side_b".to_string(),
        runs: 165,
        wickets: 10,
        overs: OversRecord {
            overs: 19,
            balls: 3,
        },
        declared: false,
        batting: Some(vec![CricketBattingEntryRecord {
            player_id: "p1".to_string(),
            runs: 30,
            balls_faced: 20,
            fours: 4,
            sixes: 1,
            dismissal: Some(CricketDismissalRecord {
                kind: CricketDismissalKindRecord::Caught,
                bowler_player_id: Some("p5".to_string()),
                fielder_player_id: Some("p6".to_string()),
            }),
            batting_position: Some(1),
        }]),
        bowling: Some(vec![CricketBowlingEntryRecord {
            player_id: "p5".to_string(),
            overs: OversRecord { overs: 4, balls: 0 },
            maidens: 1,
            runs_conceded: 20,
            wickets: 1,
            wides: 0,
            no_balls: 0,
        }]),
        fall_of_wickets: Some(vec![CricketFallOfWicketRecord {
            wicket: 1,
            runs: 30,
            player_id: "p1".to_string(),
            overs: Some(OversRecord { overs: 7, balls: 4 }),
        }]),
        extras: Some(CricketExtrasRecord {
            byes: 1,
            leg_byes: 2,
            wides: 3,
            no_balls: 0,
            penalty: 0,
        }),
    };

    let record = MatchScoreRecord {
        sport: "cricket".to_string(),
        score: ScoreRecord::Cricket(CricketScoreRecord {
            innings: vec![first_innings],
            recent_deliveries: Some(vec![CricketDeliveryRecord {
                over: 8,
                ball: 3,
                bowler_player_id: "p5".to_string(),
                striker_player_id: "p1".to_string(),
                non_striker_player_id: "p2".to_string(),
                runs_off_bat: 1,
                extra: None,
                wicket: None,
                occurred_at: Some("2026-05-01T20:08:03.000Z".to_string()),
            }]),
            next_ball_context: Some(NextBallContextRecord {
                striker_player_id: Some("p2".to_string()),
                non_striker_player_id: Some("p1".to_string()),
                bowler_player_id: Some("p5".to_string()),
                over: 8,
                ball: 4,
                previous_over_bowler_player_id: None,
                runs_conceded_this_over: 1,
            }),
            awaiting_next_innings: Some(false),
        }),
        last_seq: Some(42),
    };

    dao.put_match_score(&match_id, &record)
        .await
        .expect("put_match_score");

    let fetched = dao
        .get_match_score(&match_id, "cricket")
        .await
        .expect("get_match_score")
        .expect("record was written");

    assert_eq!(fetched, record, "round-tripped record must match exactly");

    // The "no live detail" (manually-entered, totals-only) shape.
    let bare_match_id = format!("{match_id}-bare");
    let bare_record = MatchScoreRecord {
        sport: "cricket".to_string(),
        score: ScoreRecord::Cricket(CricketScoreRecord {
            innings: vec![CricketScoreInningsRecord {
                batting_side_id: "side_a".to_string(),
                bowling_side_id: "side_b".to_string(),
                runs: 200,
                wickets: 8,
                overs: OversRecord {
                    overs: 20,
                    balls: 0,
                },
                declared: false,
                batting: None,
                bowling: None,
                fall_of_wickets: None,
                extras: None,
            }],
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: None,
        }),
        last_seq: None,
    };
    dao.put_match_score(&bare_match_id, &bare_record)
        .await
        .expect("put_match_score (bare)");
    let fetched_bare = dao
        .get_match_score(&bare_match_id, "cricket")
        .await
        .expect("get_match_score (bare)")
        .expect("bare record was written");
    assert_eq!(fetched_bare, bare_record);
}

/// Writes the *pre-refactor* wire shape directly (hand-built JSON, matching
/// exactly what `ScoreRecord::Cricket { .. }`'s inline struct-variant used to
/// serialize as — independent of any Rust type) and confirms today's code
/// reads it back correctly.
#[tokio::test]
async fn pre_refactor_cricket_score_json_still_deserializes() {
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("skipping: see netball_score_record_roundtrip.rs's header for setup.");
        return;
    };

    let match_id = format!(
        "test-cricket-legacy-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    // Exactly the JSON `ScoreRecord::Cricket { innings, recent_deliveries,
    // next_ball_context, awaiting_next_innings }` (the inline struct-variant,
    // pre-refactor) serialized as, tag included — before `CricketScoreRecord`
    // existed as a named type.
    let legacy_record_json = serde_json::json!({
        "sport": "cricket",
        "score": {
            "type": "cricket",
            "innings": [
                {
                    "batting_side_id": "side_a",
                    "bowling_side_id": "side_b",
                    "runs": 165,
                    "wickets": 10,
                    "overs": {"overs": 19, "balls": 3},
                    "declared": false
                }
            ],
            "awaiting_next_innings": true
        },
        "last_seq": 7
    });

    use agon_core::dao::item::to_item;
    use agon_core::dao::keys::{Pk, Sk};
    use agon_core::dao::match_ops::TYPE_MATCH_SCORE;

    let item = to_item(
        &Pk::Match(match_id.clone()),
        &Sk::Score("cricket".to_string()),
        TYPE_MATCH_SCORE,
        &legacy_record_json,
    )
    .expect("build item from legacy JSON shape");

    client
        .put_item()
        .table_name(&table)
        .set_item(Some(item))
        .send()
        .await
        .expect("put legacy item");

    let fetched = dao
        .get_match_score(&match_id, "cricket")
        .await
        .expect("get_match_score")
        .expect("legacy record was written");

    let ScoreRecord::Cricket(score) = &fetched.score else {
        panic!("expected ScoreRecord::Cricket, got {:?}", fetched.score);
    };
    assert_eq!(score.innings.len(), 1);
    assert_eq!(score.innings[0].runs, 165);
    assert_eq!(score.innings[0].wickets, 10);
    assert_eq!(score.awaiting_next_innings, Some(true));
    assert_eq!(fetched.last_seq, Some(7));
}
