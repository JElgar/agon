//! Drives cricket's personal bests through the real stats write path
//! (`Cricket::contribution` into `Dao::reconcile_match_contribution`) and
//! reads them back as `CricketStatsRecord`, proving the generic best writer
//! stores `best_bowling` as a `BestBowlingFiguresRecord` and only ratchets
//! it up. See `netball_score_record_roundtrip.rs` for how to run these
//! against DynamoDB Local.

mod common;

use agon_core::dao::stats::{MatchContribution, MatchOutcome};
use agon_core::sports::cricket::{
    BestBowlingFiguresRecord, Cricket, CricketBattingEntryRecord, CricketBowlingEntryRecord,
    CricketScoreInningsRecord, CricketScoreRecord, CricketStatsRecord, OversRecord,
};
use agon_core::sports::{SportContribution, SportRecord};
use common::{TestEnv, local_env};

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

/// One innings in which "me" scores `runs` and bowls `overs` taking `wickets`.
fn score(runs: u32, wickets: u32, runs_conceded: u32, overs: OversRecord) -> CricketScoreRecord {
    CricketScoreRecord {
        innings: vec![CricketScoreInningsRecord {
            batting_side_id: "a".into(),
            bowling_side_id: "b".into(),
            runs: 150,
            wickets: 6,
            overs: OversRecord {
                overs: 20,
                balls: 0,
            },
            declared: false,
            batting: Some(vec![CricketBattingEntryRecord {
                player_id: "me".into(),
                runs,
                balls_faced: 30,
                fours: 2,
                sixes: 1,
                dismissal: None,
                batting_position: None,
            }]),
            bowling: Some(vec![CricketBowlingEntryRecord {
                player_id: "me".into(),
                overs,
                maidens: 0,
                runs_conceded,
                wickets,
                wides: 0,
                no_balls: 0,
            }]),
            fall_of_wickets: None,
            extras: None,
        }],
        recent_deliveries: None,
        next_ball_context: None,
        awaiting_next_innings: None,
    }
}

async fn reconcile(env: &TestEnv, user_id: &str, match_id: &str, score: &CricketScoreRecord) {
    let c = SportContribution::from_stats(&Cricket::contribution(score, &["me"], None, match_id));
    let contribution = MatchContribution {
        outcome: Some(MatchOutcome::Won),
        counters: c.counters,
        bests: c.bests,
    };
    env.dao
        .reconcile_match_contribution(match_id, user_id, "cricket", &contribution)
        .await
        .expect("reconcile");
}

async fn cricket_stats(env: &TestEnv, user_id: &str) -> CricketStatsRecord {
    env.dao
        .get_user(user_id)
        .await
        .expect("get_user")
        .expect("profile exists")
        .stats
        .cricket
        .expect("cricket stats present")
}

#[tokio::test]
async fn cricket_bests_are_stored_in_their_record_shape_and_only_ratchet_up() {
    let Some(env) = local_env().await else {
        eprintln!("skipping: see netball_score_record_roundtrip.rs's header for setup.");
        return;
    };
    let user_id = unique("test-cricket-bests");
    let first = unique("test-cricket-bests-m1");
    let worse = unique("test-cricket-bests-m2");
    let better = unique("test-cricket-bests-m3");
    let profile = serde_json::json!({
        "id": user_id,
        "email": format!("{user_id}@example.com"),
        "name": "Bowler",
        "stats": {},
        "created_at": "2026-01-01T00:00:00.000Z",
    });
    let item = agon_core::dao::item::to_item(
        &agon_core::dao::keys::Pk::User(user_id.clone()),
        &agon_core::dao::keys::Sk::Profile,
        agon_core::dao::user::TYPE_USER,
        &profile,
    )
    .expect("build profile item");
    env.client
        .put_item()
        .table_name(&env.table)
        .set_item(Some(item))
        .send()
        .await
        .expect("put profile");

    // 3 for 24 off 3.4 overs, 40 runs.
    let three_four = OversRecord { overs: 3, balls: 4 };
    reconcile(&env, &user_id, &first, &score(40, 3, 24, three_four)).await;
    let stats = cricket_stats(&env, &user_id).await;
    assert_eq!(
        stats.best_bowling,
        Some(BestBowlingFiguresRecord {
            wickets: 3,
            runs_conceded: 24,
            overs: three_four,
            match_id: first.clone(),
        })
    );
    let best_runs = stats.best_runs.expect("best_runs set");
    assert_eq!(
        (best_runs.value, best_runs.match_id.as_str()),
        (40, first.as_str())
    );

    // Fewer wickets and runs: neither best moves.
    let four_overs = OversRecord { overs: 4, balls: 0 };
    reconcile(&env, &user_id, &worse, &score(10, 1, 12, four_overs)).await;
    let stats = cricket_stats(&env, &user_id).await;
    assert_eq!(
        stats.best_bowling.as_ref().map(|b| b.match_id.as_str()),
        Some(first.as_str())
    );
    assert_eq!(stats.best_runs.as_ref().map(|b| b.value), Some(40));

    // More wickets: best bowling moves, career wickets keep summing.
    reconcile(&env, &user_id, &better, &score(5, 5, 30, four_overs)).await;
    let stats = cricket_stats(&env, &user_id).await;
    assert_eq!(
        stats.best_bowling,
        Some(BestBowlingFiguresRecord {
            wickets: 5,
            runs_conceded: 30,
            overs: four_overs,
            match_id: better,
        })
    );
    assert_eq!(stats.wickets, 9);
    assert_eq!(stats.best_runs.map(|b| b.value), Some(40));
}
