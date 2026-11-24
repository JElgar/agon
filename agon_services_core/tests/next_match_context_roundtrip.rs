//! `Dao::batch_get_feed_entries_and_team_metas` against DynamoDB Local: one
//! batch read returns the viewer's own feed entries (where a team's "next
//! match" card reads `viewer_is_going`, as the feed does) and the opponent
//! teams' metas, each keyed by id, with missing ones simply absent and
//! duplicate keys tolerated.
//!
//! Skipped unless `AWS_ENDPOINT_URL` is set (see `common::local_env`), so it
//! never touches a real table:
//!
//! ```text
//! docker compose up -d dynamodb-local dynamodb-local-init
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret cargo test -p agon_services_core --test next_match_context_roundtrip
//! ```

mod common;

use agon_services_core::dao::audience::AudienceMember;
use agon_services_core::dao::feed::FeedViewer;
use agon_services_core::dao::records::{TeamMemberRecord, TeamRecord};
use common::{TestEnv, local_env};

const NOW: &str = "2026-11-24T10:00:00Z";

fn team(id: &str, logo: Option<&str>) -> TeamRecord {
    TeamRecord {
        id: id.into(),
        name: id.into(),
        logo_url: logo.map(Into::into),
        invite_token: None,
        follower_count: 0,
        created_at: NOW.into(),
        colour: None,
        tournament_id: None,
        sports: Vec::new(),
        members_join_default: false,
    }
}

fn owner(team_id: &str, user_id: &str) -> TeamMemberRecord {
    TeamMemberRecord {
        team_id: team_id.into(),
        membership_id: format!("{team_id}-{user_id}"),
        user_id: Some(user_id.into()),
        display_name: None,
        role: "owner".into(),
        invitation: None,
        created_at: NOW.into(),
    }
}

#[tokio::test]
async fn feed_entries_and_team_metas_come_back_in_one_read() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let viewer = format!("viewer_{run}");
    let other = format!("other_{run}");
    let (leyton, bow) = (format!("leyton_{run}"), format!("bow_{run}"));
    dao.create_team(
        &team(&leyton, Some("https://x/leyton.png")),
        &owner(&leyton, &other),
    )
    .await
    .unwrap();
    dao.create_team(&team(&bow, None), &owner(&bow, &other))
        .await
        .unwrap();

    // The viewer plays in m1, only follows someone in m2, and has no entry
    // for m3 at all.
    let (m1, m2, m3) = (
        format!("m1_{run}"),
        format!("m2_{run}"),
        format!("m3_{run}"),
    );
    let starts_at = "2026-12-01T10:00:00Z";
    let entry = |going: bool| FeedViewer {
        viewer_id: viewer.clone(),
        audience: AudienceMember {
            viewer_is_going: going,
            ..Default::default()
        },
    };
    dao.write_feed_items(&[entry(true)], &m1, starts_at, NOW)
        .await
        .unwrap();
    dao.write_feed_items(&[entry(false)], &m2, starts_at, NOW)
        .await
        .unwrap();

    let matches = vec![
        (m1.clone(), starts_at.to_string()),
        (m2.clone(), starts_at.to_string()),
        (m3.clone(), starts_at.to_string()),
        // A duplicate key mustn't fail the batch.
        (m1.clone(), starts_at.to_string()),
    ];
    let teams = vec![
        leyton.clone(),
        bow.clone(),
        leyton.clone(),
        format!("gone_{run}"),
    ];
    let (feed, metas) = dao
        .batch_get_feed_entries_and_team_metas(&viewer, &matches, &teams)
        .await
        .unwrap();

    assert_eq!(feed.len(), 2);
    assert!(feed[&m1].viewer_is_going);
    assert!(!feed[&m2].viewer_is_going);
    assert!(!feed.contains_key(&m3));
    assert_eq!(metas.len(), 2);
    assert_eq!(
        metas[&leyton].logo_url.as_deref(),
        Some("https://x/leyton.png")
    );
    assert_eq!(metas[&bow].logo_url, None);

    // Another viewer's feed is theirs alone.
    let (feed, _) = dao
        .batch_get_feed_entries_and_team_metas(&other, &matches, &[])
        .await
        .unwrap();
    assert!(feed.is_empty());

    // Nothing to read is no request at all.
    let (feed, metas) = dao
        .batch_get_feed_entries_and_team_metas(&viewer, &[], &[])
        .await
        .unwrap();
    assert!(feed.is_empty() && metas.is_empty());
}
