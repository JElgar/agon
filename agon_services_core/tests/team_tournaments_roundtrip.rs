//! A team's tournament entries against DynamoDB Local: an entry into a
//! `Teams`-audience tournament is listed under the team (GSI1
//! `TEAMTOURNS#<teamId>`), newest first, until it's withdrawn; an entry into
//! a `Players`-audience tournament (the entrant is a user) is not.
//!
//! Skipped unless `AWS_ENDPOINT_URL` is set (see `common::local_env`):
//!
//! ```text
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret cargo test -p agon_services_core --test team_tournaments_roundtrip
//! ```

mod common;

use agon_services_core::dao::records::{TournamentEntryAudienceRecord, TournamentEntryRecord};
use common::{TestEnv, local_env};

fn entry(tournament_id: &str, entrant_id: &str, created_at: &str) -> TournamentEntryRecord {
    TournamentEntryRecord {
        tournament_id: tournament_id.into(),
        entrant_id: entrant_id.into(),
        status: "accepted".into(),
        seed: None,
        squad: None,
        added_by_user_id: "organiser".into(),
        created_at: created_at.into(),
    }
}

#[tokio::test]
async fn team_entries_are_listed_by_team() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let team = format!("tt_team_{run}");
    let (league, cup, teamless) = (
        format!("tt_league_{run}"),
        format!("tt_cup_{run}"),
        format!("tt_players_{run}"),
    );

    dao.create_tournament_entry(
        &entry(&league, &team, "2026-11-01T10:00:00Z"),
        TournamentEntryAudienceRecord::Teams,
    )
    .await
    .unwrap();
    dao.create_tournament_entry(
        &entry(&cup, &team, "2026-11-02T10:00:00Z"),
        TournamentEntryAudienceRecord::Teams,
    )
    .await
    .unwrap();
    // A players-audience entry whose entrant id happens to equal the team
    // id: still not a team entry, so not projected.
    dao.create_tournament_entry(
        &entry(&teamless, &team, "2026-11-03T10:00:00Z"),
        TournamentEntryAudienceRecord::Players,
    )
    .await
    .unwrap();

    let listed: Vec<String> = dao
        .list_all_team_tournament_entries(&team)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.tournament_id)
        .collect();
    assert_eq!(listed, vec![cup.clone(), league.clone()], "newest first");

    // Paginates.
    let first = dao
        .list_team_tournament_entries(&team, None, 1)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].tournament_id, cup);
    let second = dao
        .list_team_tournament_entries(&team, first.next_cursor.as_deref(), 1)
        .await
        .unwrap();
    assert_eq!(second.items[0].tournament_id, league);

    // Another team sees nothing.
    assert!(
        dao.list_all_team_tournament_entries(&format!("tt_other_{run}"))
            .await
            .unwrap()
            .is_empty()
    );

    // Withdrawing removes it from the team's list.
    dao.delete_tournament_entry(&cup, &team).await.unwrap();
    let listed: Vec<String> = dao
        .list_all_team_tournament_entries(&team)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.tournament_id)
        .collect();
    assert_eq!(listed, vec![league]);
}
