//! Integration tests for the auto team builder (preview / apply a split of
//! a pool into a teamless tournament's squads or a match's sides).
//!
//! Same setup as `api.rs` (a live service at `AGON_SERVICE_URL`, ES256 test
//! tokens from `AGON_TEST_JWT_PRIVATE_KEY`), in its own file with its own
//! small helpers so it builds independently of the rest of the suite.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openapi::apis::configuration::Configuration;
use openapi::apis::default_api::*;
use openapi::models;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
struct JwtData {
    sub: String,
    exp: usize,
    aud: String,
    email: Option<String>,
}

fn config_for(subject: &str) -> Configuration {
    let private_key_pem =
        std::env::var("AGON_TEST_JWT_PRIVATE_KEY").expect("AGON_TEST_JWT_PRIVATE_KEY must be set");
    let kid = std::env::var("AGON_TEST_JWT_KID").unwrap_or_else(|_| "agon-test".to_string());
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(kid);
    let token = encode(
        &header,
        &JwtData {
            sub: subject.to_string(),
            exp: 9_999_999_999,
            aud: "authenticated".to_string(),
            email: Some(format!("{subject}@example.com")),
        },
        &EncodingKey::from_ec_pem(private_key_pem.as_bytes()).expect("valid EC PEM"),
    )
    .expect("sign test jwt");
    Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(token),
        ..Default::default()
    }
}

async fn new_user() -> (Configuration, models::User) {
    let config = config_for(&Uuid::new_v4().to_string());
    let user = users_post(
        &config,
        models::CreateUserInput {
            name: "Test User".to_string(),
        },
    )
    .await
    .expect("create user");
    (config, user)
}

fn iso_offset_hours(hours: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(hours))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn assert_status<T, E: std::fmt::Debug>(
    response: Result<T, openapi::apis::Error<E>>,
    expected: reqwest::StatusCode,
) {
    match response {
        Ok(_) => panic!("expected {expected}, got success"),
        Err(openapi::apis::Error::ResponseError(rc)) => assert_eq!(rc.status, expected),
        Err(e) => panic!("expected {expected} response error, got: {e:?}"),
    }
}

fn assert_status_with_content<T, E: std::fmt::Debug>(
    response: Result<T, openapi::apis::Error<E>>,
    expected: reqwest::StatusCode,
    expected_content: &str,
) {
    match response {
        Ok(_) => panic!("expected {expected} ({expected_content}), got success"),
        Err(openapi::apis::Error::ResponseError(rc)) => {
            assert_eq!(rc.status, expected);
            assert!(
                rc.content.contains(expected_content),
                "got: {:?}",
                rc.content
            );
        }
        Err(e) => panic!("expected {expected} response error, got: {e:?}"),
    }
}

/// Two empty, capped colour sides on a scheduled football match.
fn two_side_match() -> models::CreateMatchInput {
    let side = |id: &str, name: &str| {
        let mut s = models::CreateMatchSideInput::new(id.to_string());
        s.name = Some(name.to_string());
        s.colour = Some("#2952D9".to_string());
        s.max_players = Some(10);
        s
    };
    let mut input = models::CreateMatchInput::new(
        "Wednesday training".to_string(),
        "split by the team builder".to_string(),
        models::MatchType::Football,
        iso_offset_hours(24),
        vec![side("a", "Blues"), side("b", "Whites")],
        vec![],
    );
    input.allow_unassigned = Some(true);
    input
}

// ---------------------------------------------------------------------------
// Auto team builder
// ---------------------------------------------------------------------------

/// A teamless football league (players enter, squads of `squad_size`)
/// with `entrants` fresh users entered by the organiser.
async fn teamless_tournament(
    squad_size: i32,
    entrants: usize,
) -> (Configuration, models::Tournament, Vec<String>) {
    let (organiser, _) = new_user().await;
    let mut input = models::CreateTournamentInput::new(
        models::MatchType::Football,
        "Auto-assign League".to_string(),
        "squads drawn by the team builder".to_string(),
        models::TournamentFormat::League,
        models::TournamentEntryAudience::Players,
        2,
        iso_offset_hours(48),
        "UTC".to_string(),
        models::TournamentSeeding::Random,
    );
    input.team_formation = Some("manual".to_string());
    input.squad_size = Some(squad_size);
    let tournament = tournaments_post(&organiser, input)
        .await
        .expect("create tournament");
    let mut ids = Vec::new();
    for _ in 0..entrants {
        let (_, user) = new_user().await;
        tournaments_tournament_id_entries_post(
            &organiser,
            &tournament.id,
            models::AddTournamentEntryInput::new(user.profile.id.clone()),
        )
        .await
        .expect("add entry");
        ids.push(user.profile.id);
    }
    (organiser, tournament, ids)
}

fn squad_assignments(preview: &models::AutoAssignPreview) -> Vec<models::SquadAssignment> {
    preview
        .sides
        .iter()
        .flat_map(|side| {
            let squad = side.squad.expect("tournament sides carry a squad");
            side.players
                .iter()
                .map(move |p| models::SquadAssignment::new(p.id.clone(), squad))
        })
        .collect()
}

#[tokio::test]
async fn tournament_auto_assign_previews_then_applies_exactly_that_split() {
    let (organiser, tournament, ids) = teamless_tournament(4, 8).await;

    let mut input = models::TournamentAutoAssignInput::new(models::AutoAssignMethod::AutoBalanced);
    input.seed = Some(42);
    let preview = tournaments_tournament_id_squads_auto_assign_post(
        &organiser,
        &tournament.id,
        input.clone(),
    )
    .await
    .expect("preview");
    assert_eq!(preview.seed, 42);
    assert_eq!(preview.sides.len(), 2, "8 entrants in squads of 4");
    let placed: usize = preview.sides.iter().map(|s| s.players.len()).sum();
    assert_eq!(placed, ids.len());
    assert!(preview.sides.iter().all(|s| s.players.len() == 4));
    // Fresh accounts: no history, so everyone is "new" and unsure.
    let players = preview.sides.iter().flat_map(|s| &s.players);
    assert!(
        players
            .clone()
            .all(|p| p.signal == models::StrengthSignal::New && p.certainty == 1)
    );
    assert_eq!(preview.unsure_count, 8);

    // Same seed, same split.
    let again =
        tournaments_tournament_id_squads_auto_assign_post(&organiser, &tournament.id, input)
            .await
            .expect("preview again");
    assert_eq!(squad_assignments(&again).len(), 8);
    let key = |p: &models::AutoAssignPreview| {
        let mut v: Vec<(String, i32)> = squad_assignments(p)
            .into_iter()
            .map(|a| (a.id, a.squad))
            .collect();
        v.sort();
        v
    };
    assert_eq!(key(&preview), key(&again));

    let applied = tournaments_tournament_id_squads_assignments_put(
        &organiser,
        &tournament.id,
        models::ApplySquadAssignmentsInput::new(
            preview.roster_version.clone(),
            squad_assignments(&preview),
        ),
    )
    .await
    .expect("apply");
    assert_eq!(applied.updated, 8);

    let entries = tournaments_tournament_id_entries_get(&organiser, &tournament.id, None, Some(50))
        .await
        .expect("entries");
    let mut stored: Vec<(String, i32)> = entries
        .items
        .iter()
        .map(|e| {
            (
                e.entrant_id.clone(),
                e.squad.expect("every entry has a squad now"),
            )
        })
        .collect();
    stored.sort();
    assert_eq!(stored, key(&preview));

    // The pool changed (everyone moved), so the old preview is stale.
    assert_status(
        tournaments_tournament_id_squads_assignments_put(
            &organiser,
            &tournament.id,
            models::ApplySquadAssignmentsInput::new(
                preview.roster_version.clone(),
                squad_assignments(&preview),
            ),
        )
        .await,
        reqwest::StatusCode::CONFLICT,
    );
}

#[tokio::test]
async fn tournament_auto_assign_only_unassigned_keeps_placed_entrants() {
    let (organiser, tournament, ids) = teamless_tournament(3, 6).await;
    tournaments_tournament_id_entries_entrant_id_squad_post(
        &organiser,
        &tournament.id,
        &ids[0],
        models::AssignTournamentEntrySquadInput::new(1),
    )
    .await
    .expect("assign one by hand");

    let preview = tournaments_tournament_id_squads_auto_assign_post(
        &organiser,
        &tournament.id,
        models::TournamentAutoAssignInput::new(models::AutoAssignMethod::Random),
    )
    .await
    .expect("preview");
    let me = preview
        .sides
        .iter()
        .find(|s| s.players.iter().any(|p| p.id == ids[0]))
        .expect("placed somewhere");
    assert_eq!(me.squad, Some(1), "already-placed entrant stays put");
    let p = me.players.iter().find(|p| p.id == ids[0]).unwrap();
    assert!(p.pinned && !p.moved);
}

#[tokio::test]
async fn tournament_auto_assign_is_organiser_only_and_validates_input() {
    let (organiser, tournament, _ids) = teamless_tournament(2, 4).await;
    let (stranger, _) = new_user().await;
    assert_status(
        tournaments_tournament_id_squads_auto_assign_post(
            &stranger,
            &tournament.id,
            models::TournamentAutoAssignInput::new(models::AutoAssignMethod::Random),
        )
        .await,
        reqwest::StatusCode::FORBIDDEN,
    );

    let mut bad_pin = models::TournamentAutoAssignInput::new(models::AutoAssignMethod::Random);
    bad_pin.pins = Some(vec![models::SquadPin::new("not-an-entrant".to_string(), 0)]);
    assert_status_with_content(
        tournaments_tournament_id_squads_auto_assign_post(&organiser, &tournament.id, bad_pin)
            .await,
        reqwest::StatusCode::BAD_REQUEST,
        "isn't in the pool",
    );

    let preview = tournaments_tournament_id_squads_auto_assign_post(
        &organiser,
        &tournament.id,
        models::TournamentAutoAssignInput::new(models::AutoAssignMethod::Random),
    )
    .await
    .expect("preview");
    assert_status(
        tournaments_tournament_id_squads_assignments_put(
            &stranger,
            &tournament.id,
            models::ApplySquadAssignmentsInput::new(
                preview.roster_version.clone(),
                squad_assignments(&preview),
            ),
        )
        .await,
        reqwest::StatusCode::FORBIDDEN,
    );
}

/// A scheduled football match with two empty colour sides (no cap) and
/// `players` ad-hoc players added unassigned by the organiser.
async fn match_with_unassigned_pool(players: &[&str]) -> (Configuration, models::Match) {
    let (organiser, _) = new_user().await;
    let created = matches_post(&organiser, two_side_match())
        .await
        .expect("create match");
    let mut patch = models::UpdateMatchInput::new();
    patch.added_players = Some(
        players
            .iter()
            .map(|name| {
                let mut p = models::AddMatchPlayerInput::new();
                p.display_name = Some(name.to_string());
                p
            })
            .collect(),
    );
    let updated = matches_match_id_patch(&organiser, &created.id, patch)
        .await
        .expect("add players");
    (organiser, updated)
}

#[tokio::test]
async fn match_auto_assign_fills_unassigned_guests_into_both_sides() {
    let names = ["Kai", "Ana", "Ben", "Cleo", "Dan", "Eve"];
    let (organiser, m) = match_with_unassigned_pool(&names).await;

    let preview = matches_match_id_sides_auto_assign_post(
        &organiser,
        &m.id,
        models::MatchAutoAssignInput::new(models::AutoAssignMethod::AutoBalanced),
    )
    .await
    .expect("preview");
    assert_eq!(preview.sides.len(), 2);
    assert!(preview.sides.iter().all(|s| s.players.len() == 3));
    assert!(
        preview
            .sides
            .iter()
            .flat_map(|s| &s.players)
            .all(|p| p.signal == models::StrengthSignal::Guest),
        "ad-hoc players have no account"
    );
    let w = preview
        .win_probability
        .expect("two sides report a win probability");
    assert!(w.low <= w.expected && w.expected <= w.high);
    assert_eq!(
        preview.verdict,
        models::BalanceVerdict::Even,
        "six identical unknowns"
    );

    let assignments: Vec<models::SideAssignment> = preview
        .sides
        .iter()
        .flat_map(|s| {
            let side_id = s.side_id.clone().expect("match sides carry an id");
            s.players
                .iter()
                .map(move |p| models::SideAssignment::new(p.id.clone(), side_id.clone()))
        })
        .collect();
    let applied = matches_match_id_sides_assignments_put(
        &organiser,
        &m.id,
        models::ApplySideAssignmentsInput::new(preview.roster_version.clone(), assignments.clone()),
    )
    .await
    .expect("apply");
    assert_eq!(applied.updated, 6);

    let after = matches_match_id_get(&organiser, &m.id)
        .await
        .expect("match");
    for side in &after.sides {
        assert_eq!(side.player_count, 3, "headcounts follow the bulk move");
    }

    assert_status(
        matches_match_id_sides_assignments_put(
            &organiser,
            &m.id,
            models::ApplySideAssignmentsInput::new(preview.roster_version, assignments),
        )
        .await,
        reqwest::StatusCode::CONFLICT,
    );
}

#[tokio::test]
async fn match_auto_assign_is_organiser_only_and_rejects_overflow() {
    let (organiser, m) = match_with_unassigned_pool(&["A", "B", "C"]).await;
    let (stranger, _) = new_user().await;
    assert_status(
        matches_match_id_sides_auto_assign_post(
            &stranger,
            &m.id,
            models::MatchAutoAssignInput::new(models::AutoAssignMethod::Random),
        )
        .await,
        reqwest::StatusCode::FORBIDDEN,
    );

    let mut level = models::MatchAutoAssignInput::new(models::AutoAssignMethod::AutoBalanced);
    let guest = m
        .players
        .iter()
        .find(|p| p.side_id.is_none())
        .expect("unassigned");
    let guest_id = match guest.member.as_ref() {
        models::Member::External(g) => g.id.clone(),
        models::Member::User(u) => u.id.clone(),
    };
    level.levels = Some(vec![models::PlayerLevelInput::new(
        guest_id.clone(),
        models::PlayerLevel::Stronger,
    )]);
    let preview = matches_match_id_sides_auto_assign_post(&organiser, &m.id, level)
        .await
        .expect("preview");
    let p = preview
        .sides
        .iter()
        .flat_map(|s| &s.players)
        .find(|p| p.id == guest_id)
        .unwrap();
    assert_eq!(p.level, Some(models::PlayerLevel::Stronger));
}
