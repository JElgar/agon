//! Regression guard for the football `ScoreRecord` shape refactor — see
//! `netball_score_record_roundtrip.rs`'s doc comment for what these tests
//! prove, why, and how to run them locally against DynamoDB Local. Same
//! structure, football's shapes.

mod common;

use std::collections::HashMap;

use agon_core::dao::records::{MatchScoreRecord, ScoreRecord};
use agon_core::sports::football::{
    FootballCardColorRecord, FootballCardEventRecord, FootballGoalEventRecord, FootballScoreRecord,
    FootballSubstitutionEventRecord,
};
use common::{TestEnv, local_env};

#[tokio::test]
async fn football_score_record_round_trips_through_dynamodb() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!(
            "skipping: set AWS_ENDPOINT_URL (e.g. http://localhost:8002) to run this test \
             against DynamoDB Local — see netball_score_record_roundtrip.rs's header for setup."
        );
        return;
    };

    let match_id = format!(
        "test-football-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let mut score = HashMap::new();
    score.insert("side_a".to_string(), 2u32);
    score.insert("side_b".to_string(), 1u32);

    let mut period_times = HashMap::new();
    period_times.insert(
        agon_core::sports::football::FootballPeriodRecord::KickOff,
        "2026-01-10T15:00:00Z".to_string(),
    );

    let mut shootout_score = HashMap::new();
    shootout_score.insert("side_a".to_string(), 4u32);
    shootout_score.insert("side_b".to_string(), 3u32);

    let record = MatchScoreRecord {
        sport: "football".to_string(),
        score: ScoreRecord::Football(FootballScoreRecord {
            score,
            goals: Some(vec![
                FootballGoalEventRecord {
                    side_id: "side_a".to_string(),
                    scorer_player_id: Some("p1".to_string()),
                    assist_player_id: Some("p2".to_string()),
                    own_goal: false,
                    penalty: false,
                    minute: Some(23),
                    occurred_at: Some("2026-01-10T15:23:00Z".to_string()),
                },
                FootballGoalEventRecord {
                    side_id: "side_b".to_string(),
                    scorer_player_id: None,
                    assist_player_id: None,
                    own_goal: true,
                    penalty: false,
                    minute: None,
                    occurred_at: None,
                },
            ]),
            cards: Some(vec![FootballCardEventRecord {
                side_id: "side_a".to_string(),
                player_id: "p3".to_string(),
                color: FootballCardColorRecord::Yellow,
                minute: Some(58),
                occurred_at: None,
            }]),
            substitutions: Some(vec![FootballSubstitutionEventRecord {
                side_id: "side_a".to_string(),
                player_in_id: "p4".to_string(),
                player_out_id: "p1".to_string(),
                minute: Some(70),
                occurred_at: None,
            }]),
            period: Some(agon_core::sports::football::FootballPeriodRecord::FullTime),
            period_times: Some(period_times),
            penalty_shootout: Some(vec![
                agon_core::sports::football::FootballPenaltyShootoutKickRecord {
                    side_id: "side_a".to_string(),
                    scored: true,
                },
            ]),
            penalty_shootout_score: Some(shootout_score),
        }),
        last_seq: Some(9),
    };

    dao.put_match_score(&match_id, &record)
        .await
        .expect("put_match_score");

    let fetched = dao
        .get_match_score(&match_id, "football")
        .await
        .expect("get_match_score")
        .expect("record was written");

    assert_eq!(fetched, record, "round-tripped record must match exactly");

    // The "no live detail" (manually-entered, bare tally) shape — every
    // optional field `None`.
    let bare_match_id = format!("{match_id}-bare");
    let mut bare_score = HashMap::new();
    bare_score.insert("side_a".to_string(), 3u32);
    bare_score.insert("side_b".to_string(), 0u32);
    let bare_record = MatchScoreRecord {
        sport: "football".to_string(),
        score: ScoreRecord::Football(FootballScoreRecord {
            score: bare_score,
            goals: None,
            cards: None,
            substitutions: None,
            period: None,
            period_times: None,
            penalty_shootout: None,
            penalty_shootout_score: None,
        }),
        last_seq: None,
    };
    dao.put_match_score(&bare_match_id, &bare_record)
        .await
        .expect("put_match_score (bare)");
    let fetched_bare = dao
        .get_match_score(&bare_match_id, "football")
        .await
        .expect("get_match_score (bare)")
        .expect("bare record was written");
    assert_eq!(fetched_bare, bare_record);
}

/// Writes the *pre-refactor* wire shape directly (hand-built JSON, matching
/// exactly what `ScoreRecord::Football { .. }`'s inline struct-variant used
/// to serialize as — independent of any Rust type) and confirms today's code
/// reads it back correctly. Also covers the pre-existing "missing `score`
/// tally field" backward-compat case (`records.rs`'s
/// `football_score_missing_tally_field_deserializes` unit test covers the
/// same thing in-process; this proves it through a real DynamoDB round-trip
/// too).
#[tokio::test]
async fn pre_refactor_football_score_json_still_deserializes() {
    let Some(TestEnv { dao, client, table }) = local_env().await else {
        eprintln!("skipping: see netball_score_record_roundtrip.rs's header for setup.");
        return;
    };

    let match_id = format!(
        "test-football-legacy-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    // Exactly the JSON `ScoreRecord::Football { score, goals, cards, .. }`
    // (the inline struct-variant, pre-refactor) serialized as, tag included —
    // before `FootballScoreRecord` existed as a named type.
    let legacy_record_json = serde_json::json!({
        "sport": "football",
        "score": {
            "type": "football",
            "score": {"side_a": 2, "side_b": 1},
            "goals": [
                {
                    "side_id": "side_a",
                    "scorer_player_id": "p1",
                    "own_goal": false,
                    "penalty": false,
                    "minute": 23
                }
            ],
            "cards": [
                {
                    "side_id": "side_a",
                    "player_id": "p3",
                    "color": "yellow",
                    "minute": 58
                }
            ]
        },
        "last_seq": 4
    });

    use agon_core::dao::item::to_item;
    use agon_core::dao::keys::{Pk, Sk};
    use agon_core::dao::match_ops::TYPE_MATCH_SCORE;

    let item = to_item(
        &Pk::Match(match_id.clone()),
        &Sk::Score("football".to_string()),
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
        .get_match_score(&match_id, "football")
        .await
        .expect("get_match_score")
        .expect("legacy record was written");

    let ScoreRecord::Football(score) = &fetched.score else {
        panic!("expected ScoreRecord::Football, got {:?}", fetched.score);
    };
    assert_eq!(score.score.get("side_a"), Some(&2));
    assert_eq!(score.score.get("side_b"), Some(&1));
    assert_eq!(score.goals.as_ref().unwrap().len(), 1);
    assert_eq!(score.cards.as_ref().unwrap().len(), 1);
    assert_eq!(fetched.last_seq, Some(4));
}
