//! Regression guard for netball's stats shape: `UserStatsRecord::netball`
//! goes from the generic `GenericSportStatsRecord` to a dedicated
//! `NetballStatsRecord` (the common counters flattened in, plus `goals`).
//!
//! Written and run against the code *before* that change, then re-run after
//! it — see `netball_score_record_roundtrip.rs`'s doc comment for the method
//! and for how to run these against DynamoDB Local.
//!
//! The first test proves a profile written today (the generic shape — no
//! `goals` attribute at all) still reads back afterwards. The second drives
//! the real stats write path (`Dao::reconcile_match_contribution`) on top of
//! such a profile, so it also proves the sport-agnostic `ADD
//! stats.netball.<counter>` update lands a new counter into an existing
//! legacy map correctly.

mod common;

use std::collections::HashMap;

use agon_core::dao::item::to_item;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::stats::{MatchContribution, MatchOutcome};
use agon_core::dao::user::TYPE_USER;
use common::{TestEnv, local_env};

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

/// Write a user profile whose `stats.netball` is exactly what the service
/// stores today: only the counters that have ever been incremented (the map
/// is sparse — see `GenericSportStatsRecord`), and no `goals`.
async fn put_legacy_profile(env: &TestEnv, user_id: &str) {
    let profile = serde_json::json!({
        "id": user_id,
        "email": format!("{user_id}@example.com"),
        "name": "Legacy Netballer",
        "created_at": "2026-01-01T00:00:00.000Z",
        "stats": {
            "netball": { "matches_played": 3, "wins": 2, "losses": 1 }
        }
    });
    let item = to_item(
        &Pk::User(user_id.to_string()),
        &Sk::Profile,
        TYPE_USER,
        &profile,
    )
    .expect("build legacy profile item");
    env.client
        .put_item()
        .table_name(&env.table)
        .set_item(Some(item))
        .send()
        .await
        .expect("put legacy profile");
}

#[tokio::test]
async fn pre_existing_netball_stats_still_deserialize() {
    let Some(env) = local_env().await else {
        eprintln!("skipping: see netball_score_record_roundtrip.rs's header for setup.");
        return;
    };
    let user_id = unique("test-netball-stats-legacy");
    put_legacy_profile(&env, &user_id).await;

    let user = env
        .dao
        .get_user(&user_id)
        .await
        .expect("get_user")
        .expect("profile was written");
    let netball = user.stats.netball.expect("netball stats present");
    assert_eq!(netball.common.matches_played, 3);
    assert_eq!(netball.common.wins, 2);
    assert_eq!(netball.common.draws, 0, "absent counter reads as 0");
    assert_eq!(netball.common.losses, 1);
    assert_eq!(netball.goals, 0, "no `goals` attribute yet reads as 0");
}

#[tokio::test]
async fn reconciling_goals_onto_a_legacy_netball_profile() {
    let Some(env) = local_env().await else {
        eprintln!("skipping: see netball_score_record_roundtrip.rs's header for setup.");
        return;
    };
    let user_id = unique("test-netball-stats-reconcile");
    let match_id = unique("test-netball-stats-match");
    put_legacy_profile(&env, &user_id).await;

    let won_with_goals = MatchContribution {
        outcome: Some(MatchOutcome::Won),
        counters: HashMap::from([("goals".to_string(), 7)]),
        ..Default::default()
    };
    env.dao
        .reconcile_match_contribution(&match_id, &user_id, "netball", &won_with_goals)
        .await
        .expect("reconcile");

    let netball = env
        .dao
        .get_user(&user_id)
        .await
        .expect("get_user")
        .expect("profile exists")
        .stats
        .netball
        .expect("netball stats present");
    assert_eq!(netball.common.matches_played, 4, "3 legacy + this match");
    assert_eq!(netball.common.wins, 3);
    assert_eq!(netball.common.losses, 1);
    assert_eq!(netball.goals, 7);

    // A re-score to fewer goals is a delta, not a second add — the same
    // idempotent/self-correcting reconcile every other sport gets.
    let rescored = MatchContribution {
        counters: HashMap::from([("goals".to_string(), 5)]),
        ..won_with_goals
    };
    env.dao
        .reconcile_match_contribution(&match_id, &user_id, "netball", &rescored)
        .await
        .expect("reconcile rescore");
    let netball = env
        .dao
        .get_user(&user_id)
        .await
        .expect("get_user")
        .expect("profile exists")
        .stats
        .netball
        .expect("netball stats present");
    assert_eq!(
        netball.common.matches_played, 4,
        "re-score doesn't re-count"
    );
    assert_eq!(netball.goals, 5);
}
