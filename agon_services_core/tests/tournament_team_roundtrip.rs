//! A teamless tournament's squad team against DynamoDB Local: it's created
//! with its members in one go, a second create is a no-op, a rename updates
//! its name and colour, and it stays out of every listing a normal team
//! appears in ("my teams", followed teams, the entity index).
//!
//! Skipped unless `AWS_ENDPOINT_URL` is set (see `common::local_env`), so it
//! never touches a real table:
//!
//! ```text
//! docker compose up -d dynamodb-local dynamodb-local-init
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret cargo test -p agon_services_core --test tournament_team_roundtrip
//! ```

mod common;

use agon_services_core::dao::entity_index::EntityKind;
use agon_services_core::dao::records::{TeamMemberRecord, TeamRecord, squad_team_id};
use common::{TestEnv, local_env};

fn member(team_id: &str, user_id: &str) -> TeamMemberRecord {
    TeamMemberRecord {
        team_id: team_id.into(),
        membership_id: format!("{team_id}-{user_id}"),
        user_id: Some(user_id.into()),
        display_name: None,
        role: "member".into(),
        invitation: None,
        created_at: "2026-11-16T10:00:00Z".into(),
    }
}

#[tokio::test]
async fn squad_team_is_created_once_and_stays_hidden() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let tournament_id = format!("tteam_{run}");
    let team_id = squad_team_id(&tournament_id, 2);
    assert_eq!(team_id, format!("{tournament_id}.squad2"));
    let (u1, u2) = (format!("u1_{run}"), format!("u2_{run}"));

    let team = TeamRecord {
        id: team_id.clone(),
        name: "Reds".into(),
        logo_url: None,
        invite_token: None,
        follower_count: 0,
        created_at: "2026-11-16T10:00:00Z".into(),
        colour: Some("#d33".into()),
        tournament_id: Some(tournament_id.clone()),
    };
    let members = [member(&team_id, &u1), member(&team_id, &u2)];
    assert!(dao.create_tournament_team(&team, &members).await.unwrap());

    // A retried start finds it already there and changes nothing.
    let renamed = TeamRecord {
        name: "Should not overwrite".into(),
        ..team.clone()
    };
    assert!(!dao.create_tournament_team(&renamed, &[]).await.unwrap());

    let stored = dao.get_team(&team_id).await.unwrap().expect("team stored");
    assert_eq!(stored.team.name, "Reds");
    assert_eq!(
        stored.team.tournament_id.as_deref(),
        Some(tournament_id.as_str())
    );
    assert!(stored.team.is_tournament_team());
    let mut users: Vec<_> = stored
        .members
        .iter()
        .filter_map(|m| m.user_id.clone())
        .collect();
    users.sort();
    assert_eq!(users, vec![u1.clone(), u2.clone()]);

    // Hidden: not in either member's "my teams", nobody follows it, and it
    // isn't listed in the entity index.
    for u in [&u1, &u2] {
        let mine = dao.list_user_teams(u, None, 50).await.unwrap();
        assert!(mine.items.is_empty(), "squad team must not be in my teams");
        assert!(!dao.is_following_team(u, &team_id).await.unwrap());
    }
    assert_eq!(stored.team.follower_count, 0);
    let mut cursor = None;
    loop {
        let page = dao
            .list_entity_ids(EntityKind::Team, true, cursor.as_deref(), 100)
            .await
            .unwrap();
        assert!(
            !page.items.contains(&team_id),
            "squad team must not be in the entity index"
        );
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    // Renaming the squad after start renames the team.
    dao.set_tournament_team_label(&team_id, "Crimson", "#a00")
        .await
        .unwrap();
    let meta = dao.get_team_meta(&team_id).await.unwrap().unwrap();
    assert_eq!(
        (meta.name.as_str(), meta.colour.as_deref()),
        ("Crimson", Some("#a00"))
    );
    assert!(
        dao.set_tournament_team_label(&format!("{tournament_id}.squad9"), "x", "#000")
            .await
            .is_err(),
        "renaming a squad that has no team is NotFound"
    );
}

/// A knockout winner's squad moves into the next round's placeholder side as
/// a reference to its team: the placeholder name and any colour are cleared
/// (the side shows the team's), team join stays off, and a second fill is a
/// no-op.
#[tokio::test]
async fn bracket_fill_references_the_squad_team() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let match_id = format!("tteam_final_{run}");
    let m: agon_services_core::dao::records::MatchRecord = serde_json::from_value(serde_json::json!({
        "id": match_id, "name": "Final", "description": "", "match_type": "football",
        "status": "scheduled", "starts_at": "2026-12-01T10:00:00Z", "created_at": "2026-11-16T10:00:00Z",
        "sides": {
            "a": { "side_id": "a", "name": "TBD", "colour": "#999999" },
            "b": { "side_id": "b", "name": "TBD" }
        }
    }))
    .unwrap();
    dao.create_match(&m, &[]).await.unwrap();

    let team_id = squad_team_id(&format!("tteam_{run}"), 1);
    assert!(
        dao.fill_tournament_bracket_squad_side(&match_id, "a", &team_id)
            .await
            .unwrap()
    );
    assert!(
        !dao.fill_tournament_bracket_squad_side(&match_id, "a", "someone-else")
            .await
            .unwrap(),
        "an already-filled slot is left alone"
    );
    let stored = dao.get_match(&match_id).await.unwrap().unwrap();
    let side = &stored.match_.sides["a"];
    assert_eq!(side.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(side.name, None);
    assert_eq!(side.colour, None);
    assert!(!side.team_join_enabled);
    assert_eq!(stored.match_.sides["b"].name.as_deref(), Some("TBD"));
}
