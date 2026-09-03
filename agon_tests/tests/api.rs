//! Integration tests for the Agon API.
//!
//! These run against a live service (`AGON_SERVICE_URL`) backed by real
//! DynamoDB, using the generated OpenAPI client. Each test authenticates as a
//! freshly-created user, signing an ES256 JWT with the test private key
//! (`AGON_TEST_JWT_PRIVATE_KEY`) that the service trusts via its static JWK set,
//! so tests are independent and can run against a shared environment without
//! colliding.
//!
//! Scope: the synchronously-working surface — users, teams, matches, comments,
//! likes, follows, invitations, notifications. Search and feed depend on the
//! async worker populating indexes / fan-out, so they're only smoke-tested for
//! shape (see the `search` and `feed` tests), not for eventual-consistency
//! content.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openapi::apis::configuration::Configuration;
use openapi::apis::default_api::*;
use openapi::models;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Auth / configuration helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
struct JwtData {
    sub: String,
    exp: usize,
    /// Audience. The service enforces this matches its expected audience
    /// (`authenticated`, mirroring Supabase), so tokens must carry it.
    aud: String,
    /// The identity provider's email claim. The service reads the user's email
    /// from here (not the request body), so tokens must carry it for signup.
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}

/// The deterministic email for a test subject. Signup reads the email from the
/// token, so the JWT and any expectations use this same derivation.
fn email_for(subject: &str) -> String {
    format!("{subject}@example.com")
}

/// Sign a token with the ES256 test private key. The service trusts the matching
/// public JWK (via `AGON_STATIC_JWKS`), so this is the asymmetric equivalent of
/// the old shared-secret signing — an isolated, test-only key. `AGON_TEST_JWT_KID`
/// must match the `kid` of that public JWK (defaults to `agon-test`).
fn sign_es256(claims: &JwtData) -> String {
    let private_key_pem =
        std::env::var("AGON_TEST_JWT_PRIVATE_KEY").expect("AGON_TEST_JWT_PRIVATE_KEY must be set");
    let kid = std::env::var("AGON_TEST_JWT_KID").unwrap_or_else(|_| "agon-test".to_string());

    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(kid);

    encode(
        &header,
        claims,
        &EncodingKey::from_ec_pem(private_key_pem.as_bytes())
            .expect("AGON_TEST_JWT_PRIVATE_KEY is not a valid EC PEM"),
    )
    .expect("failed to generate test jwt")
}

fn generate_jwt(user_id: &str) -> String {
    sign_es256(&JwtData {
        sub: user_id.to_string(),
        exp: 9999999999,
        aud: "authenticated".to_string(),
        email: Some(email_for(user_id)),
    })
}

/// A client configured to authenticate as the given subject (JWT `sub` + a
/// matching `email` claim).
fn config_for(subject: &str) -> Configuration {
    Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(generate_jwt(subject)),
        ..Default::default()
    }
}

/// A client for a specific subject with an explicit `email` claim — used to test
/// two distinct identities presenting the same authenticated email.
fn config_with_email(subject: &str, email: &str) -> Configuration {
    let token = sign_es256(&JwtData {
        sub: subject.to_string(),
        exp: 9999999999,
        aud: "authenticated".to_string(),
        email: Some(email.to_string()),
    });
    Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(token),
        ..Default::default()
    }
}

/// Create a brand-new user and return (their client config, their profile). The
/// JWT subject is the created user id, and the user's email comes from the
/// token's `email` claim (not the request body).
async fn new_user() -> (Configuration, models::User) {
    let subject = Uuid::new_v4().to_string();
    let config = config_for(&subject);
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

// ---------------------------------------------------------------------------
// Small builders for the more elaborate inputs
// ---------------------------------------------------------------------------

/// An RFC-3339 UTC timestamp `hours` from now (negative => in the past). Used to
/// build match times that satisfy the server's scheduled-in-future /
/// completed-in-past rule without hard-coding a date that eventually goes stale.
fn iso_offset_hours(hours: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(hours))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A minimal two-sided match: sides "a" and "b", one invited user on side "a".
/// Scheduled in the future (no score), so it passes create-time validation.
fn create_match_input(invited_user_id: &str) -> models::CreateMatchInput {
    models::CreateMatchInput {
        name: "Test Match".to_string(),
        description: "A test match".to_string(),
        match_type: models::MatchType::Tennis,
        starts_at: iso_offset_hours(24),
        location: None,
        sides: vec![
            models::CreateMatchSideInput {
                client_id: "a".to_string(),
                team_id: None,
                name: Some("Side A".to_string()),
            },
            models::CreateMatchSideInput {
                client_id: "b".to_string(),
                team_id: None,
                name: Some("Side B".to_string()),
            },
        ],
        invites: vec![models::CreateMatchInviteInput {
            side_client_id: Some("a".to_string()),
            invited_user_ids: vec![invited_user_id.to_string()],
            invited_externals: vec![],
        }],
        creator_side_client_id: None,
        score: None,
        winner_side_id: None,
        header_photo_asset_ids: None,
        format: None,
        ranked: None,
    }
}

/// A two-sided match wiring specific users onto each side as invited players.
/// Every invited user carries a `user_id` from creation, so all of them are
/// fan-out participants immediately (before any acceptance) — which is what the
/// feed-scenario tests rely on. `side_a`/`side_b` are the user ids to put on
/// sides "a" and "b" respectively.
///
/// A creator wanting their *own* followers to receive the match simply includes
/// their own id in one of the sides (self-invite): on this surface a participant
/// is any player with a linked user id, and there's no self-invite guard.
fn match_between(name: &str, side_a: &[&str], side_b: &[&str]) -> models::CreateMatchInput {
    let invite_side = |client_id: &str, ids: &[&str]| models::CreateMatchInviteInput {
        side_client_id: Some(client_id.to_string()),
        invited_user_ids: ids.iter().map(|id| id.to_string()).collect(),
        invited_externals: vec![],
    };
    models::CreateMatchInput {
        name: name.to_string(),
        description: "A test match".to_string(),
        match_type: models::MatchType::Tennis,
        starts_at: iso_offset_hours(24),
        location: None,
        sides: vec![
            models::CreateMatchSideInput {
                client_id: "a".to_string(),
                team_id: None,
                name: Some("Side A".to_string()),
            },
            models::CreateMatchSideInput {
                client_id: "b".to_string(),
                team_id: None,
                name: Some("Side B".to_string()),
            },
        ],
        invites: vec![invite_side("a", side_a), invite_side("b", side_b)],
        creator_side_client_id: None,
        score: None,
        winner_side_id: None,
        header_photo_asset_ids: None,
        format: None,
        ranked: None,
    }
}

/// Invite one or more Agon users onto a side.
fn invite_users(side_client_id: &str, ids: &[&str]) -> models::CreateMatchInviteInput {
    models::CreateMatchInviteInput {
        side_client_id: Some(side_client_id.to_string()),
        invited_user_ids: ids.iter().map(|id| id.to_string()).collect(),
        invited_externals: vec![],
    }
}

/// Invite one or more external (unaccounted) people by name onto a side. Each
/// gets a minted invite token, surfaced on the created match's external player —
/// the credential the by-token accept flow (invite link) uses. `client_id` is
/// just `name` here (unique enough within a test), mirroring how a real
/// client would generate one per tagged guest before the match exists.
fn invite_externals(side_client_id: &str, names: &[&str]) -> models::CreateMatchInviteInput {
    models::CreateMatchInviteInput {
        side_client_id: Some(side_client_id.to_string()),
        invited_user_ids: vec![],
        invited_externals: names
            .iter()
            .map(|n| models::CreateMatchExternalInviteInput {
                client_id: n.to_string(),
                name: n.to_string(),
            })
            .collect(),
    }
}

/// A completed (already-played) tennis match: the creator plays on side "a" and
/// submits a final score (creator wins), with `invites` placing the opponent(s)
/// on side "b". A past `starts_at` + a score => status Completed, so accepting an
/// invitation into it credits the accepter's stats (a scheduled match wouldn't).
fn completed_match(invites: Vec<models::CreateMatchInviteInput>) -> models::CreateMatchInput {
    models::CreateMatchInput {
        name: "Completed Match".to_string(),
        description: "already played".to_string(),
        match_type: models::MatchType::Tennis,
        starts_at: iso_offset_hours(-2),
        location: None,
        sides: vec![
            models::CreateMatchSideInput {
                client_id: "a".to_string(),
                team_id: None,
                name: Some("Side A".to_string()),
            },
            models::CreateMatchSideInput {
                client_id: "b".to_string(),
                team_id: None,
                name: Some("Side B".to_string()),
            },
        ],
        invites,
        creator_side_client_id: Some("a".to_string()),
        score: Some(Box::new(simple_score("a", "b", 6, 3))),
        winner_side_id: Some("a".to_string()),
        header_photo_asset_ids: None,
        format: None,
        ranked: None,
    }
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_user_and_get_me() {
    let (config, user) = new_user().await;

    let me = users_me_get(&config).await.expect("get me");
    assert_eq!(me.profile.id, user.profile.id);
    assert_eq!(me.email, user.email);
}

#[tokio::test]
async fn signup_email_comes_from_the_token() {
    let subject = Uuid::new_v4().to_string();
    let claimed_email = format!("claimed-{subject}@example.com");
    let config = config_with_email(&subject, &claimed_email);

    let user = users_post(
        &config,
        models::CreateUserInput {
            name: "Token Email".to_string(),
        },
    )
    .await
    .expect("create user");

    assert_eq!(
        user.email, claimed_email,
        "account email must be the token's email claim"
    );
}

/// A token with no `email` claim can't sign up (there's no trusted email to use).
#[tokio::test]
async fn signup_without_an_email_claim_is_rejected() {
    // Build a token deliberately missing the email claim.
    let subject = Uuid::new_v4().to_string();
    let token = sign_es256(&JwtData {
        sub: subject.clone(),
        exp: 9999999999,
        aud: "authenticated".to_string(),
        email: None,
    });
    let config = Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(token),
        ..Default::default()
    };

    let response = users_post(
        &config,
        models::CreateUserInput {
            name: "No Email".to_string(),
        },
    )
    .await;
    assert_status_with_content(response, reqwest::StatusCode::BAD_REQUEST, "email");
}

#[tokio::test]
async fn get_user_profile_by_id() {
    let (_creator, subject) = new_user().await;
    // A second user views the first's public profile.
    let (viewer_config, _viewer) = new_user().await;

    let profile = users_user_id_get(&viewer_config, &subject.profile.id)
        .await
        .expect("get profile");
    assert_eq!(profile.id, subject.profile.id);
    assert!(!profile.is_followed_by_me);
}

#[tokio::test]
async fn update_me_changes_name() {
    let (config, _user) = new_user().await;

    let updated = users_me_patch(
        &config,
        models::UpdateUserInput {
            name: Some("New Name".to_string()),
            profile_image_asset_id: None,
            rating_visibility: None,
        },
    )
    .await
    .expect("patch me");

    assert_eq!(updated.profile.name, "New Name");
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_get_team() {
    let (config, _user) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Surrey".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    assert_eq!(team.name, "Surrey");

    let fetched = teams_team_id_get(&config, &team.id)
        .await
        .expect("get team");
    assert_eq!(fetched.id, team.id);
    assert_eq!(fetched.name, "Surrey");
    // The creator is a member — members are listed separately (paginated),
    // not embedded on the team itself.
    let members = teams_team_id_members_get(&config, &team.id, None, None)
        .await
        .expect("list members");
    assert!(!members.items.is_empty());
}

#[tokio::test]
async fn team_appears_in_my_teams() {
    let (config, _user) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "My Team".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let page = users_me_teams_get(&config, None, None)
        .await
        .expect("my teams");
    assert!(page.items.iter().any(|t| t.id == team.id));
}

#[tokio::test]
async fn add_and_remove_team_member() {
    let (config, _owner) = new_user().await;
    let (_other_config, member) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Roster Test".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    teams_team_id_members_post(
        &config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member.profile.id.clone()],
        },
    )
    .await
    .expect("add member");
    let with_member = all_team_members(&config, &team.id).await;
    assert!(member_ids(&with_member).contains(&member.profile.id));

    // Find the membership id for the added user to remove them.
    let member_id = membership_id_for(&with_member, &member.profile.id).expect("membership id");
    teams_team_id_members_member_id_delete(&config, &team.id, &member_id)
        .await
        .expect("remove member");
    let after_remove = all_team_members(&config, &team.id).await;
    assert!(!member_ids(&after_remove).contains(&member.profile.id));
}

/// A `User`-kind team member's `name` (and `avatar_url`, when set) is
/// hydrated from their account — `team_member_from_record` is a pure DAO->API
/// mapping with no DB access, so this only holds if the handler routes its
/// result through `hydrate_team_members` first. Covers both the member added
/// at creation (the admin) and one added afterward, so a regression in either
/// path shows up here.
#[tokio::test]
async fn team_members_have_hydrated_names() {
    let (config, owner) = new_user().await;
    let (_member_config, member) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Hydration FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    teams_team_id_members_post(
        &config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member.profile.id.clone()],
        },
    )
    .await
    .expect("add member");
    let members = all_team_members(&config, &team.id).await;

    for (user_id, expected_name) in [
        (&owner.profile.id, &owner.profile.name),
        (&member.profile.id, &member.profile.name),
    ] {
        let found = members
            .iter()
            .find_map(|m| match &*m.member {
                models::Member::User(u) if &u.user_id == user_id => Some(u),
                _ => None,
            })
            .unwrap_or_else(|| panic!("membership row for {user_id}"));
        assert_eq!(
            &found.name, expected_name,
            "member's name should be hydrated from their account, not left blank"
        );
    }
}

/// `GET /teams/:id/members` actually paginates (not just accepts the params)
/// — the whole point of splitting members out of the embedded `Team.members`
/// this endpoint replaced — and 404s for a team that doesn't exist, same as
/// every other team-scoped endpoint.
#[tokio::test]
async fn list_team_members_paginates() {
    let (config, owner) = new_user().await;
    let (_a_config, member_a) = new_user().await;
    let (_b_config, member_b) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Paginated FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    teams_team_id_members_post(
        &config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member_a.profile.id.clone(), member_b.profile.id.clone()],
        },
    )
    .await
    .expect("add members");
    // Creator + 2 added = 3 members total.

    let first_page = teams_team_id_members_get(&config, &team.id, None, Some(2))
        .await
        .expect("first page");
    assert_eq!(first_page.items.len(), 2);
    let cursor = first_page
        .next_cursor
        .expect("a 3rd member remains, so there should be a next page");

    let second_page = teams_team_id_members_get(&config, &team.id, Some(&cursor), Some(2))
        .await
        .expect("second page");
    assert_eq!(second_page.items.len(), 1, "the remaining member");
    assert!(
        second_page.next_cursor.is_none(),
        "no more pages after the 3rd member"
    );

    // Together, both pages cover every member exactly once.
    let all_ids: Vec<String> = member_ids(&first_page.items)
        .into_iter()
        .chain(member_ids(&second_page.items))
        .collect();
    for expected in [owner.profile.id, member_a.profile.id, member_b.profile.id] {
        assert!(
            all_ids.contains(&expected),
            "{expected} should appear exactly once across both pages"
        );
    }

    let missing = teams_team_id_members_get(&config, "no-such-team", None, None).await;
    assert_not_found(missing);
}

/// Removing a member that's already gone (or never existed) returns 404 on a
/// real team, rather than silently succeeding with the roster unchanged —
/// membership removal is delete-by-id, not a toggle.
#[tokio::test]
async fn removing_an_already_removed_team_member_returns_not_found() {
    let (config, _owner) = new_user().await;
    let (_other_config, member) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Roster Test".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    teams_team_id_members_post(
        &config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member.profile.id.clone()],
        },
    )
    .await
    .expect("add member");
    let with_member = all_team_members(&config, &team.id).await;
    let member_id = membership_id_for(&with_member, &member.profile.id).expect("membership id");

    teams_team_id_members_member_id_delete(&config, &team.id, &member_id)
        .await
        .expect("first remove");

    let response = teams_team_id_members_member_id_delete(&config, &team.id, &member_id).await;
    assert_not_found(response);
}

// ---------------------------------------------------------------------------
// Matches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_get_match() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    assert_eq!(created.name, "Test Match");
    assert_eq!(created.sides.len(), 2);

    let fetched = matches_match_id_get(&config, &created.id)
        .await
        .expect("get match");
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.match_type, models::MatchType::Tennis);
    // Sides must survive the round-trip through get_match (a DAO query-bound bug
    // once dropped them, which broke score validation — guard against regressing).
    assert_eq!(fetched.sides.len(), 2, "get_match returns both sides");
    assert_eq!(
        fetched.players.len(),
        1,
        "get_match returns the invited player"
    );
}

#[tokio::test]
async fn patch_match_updates_name() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let updated = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            name: Some("Renamed Match".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("patch match");
    assert_eq!(updated.name, "Renamed Match");
}

/// A match's ad-hoc side names (given at create time) can be edited afterwards
/// via `side_names`, and clearing one (`name: None`) falls back to the next
/// entry in the priority chain rather than staying stuck on the old custom
/// name.
///
/// Side "a" carries both a custom name *and* the sole invited player: an
/// explicit name always wins over the sole player's name (see
/// `Api::resolve_side_names`'s priority chain), so it resolves to "Side A"
/// throughout, and clearing that name falls through to reveal the player's
/// name underneath. Side "b" has no players, so its resolved name is its
/// custom name; clearing *that* one is exercised separately (see
/// `clearing_the_name_of_an_empty_side_is_rejected`) since an empty,
/// teamless side can't be left without a name at all.
#[tokio::test]
async fn patch_match_renames_sides() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    // Response side order isn't creation order (sides sort by their
    // generated id), so resolve "a"/"b" by roster rather than by index.
    let side_a = side_id_for_user(&created, &invitee.profile.id);
    let side_b = created
        .sides
        .iter()
        .find(|s| s.id != side_a)
        .expect("a second side")
        .id
        .clone();
    assert_eq!(
        created
            .sides
            .iter()
            .find(|s| s.id == side_a)
            .unwrap()
            .name
            .as_deref(),
        Some("Side A"),
        "a custom name wins over the sole player's name"
    );
    assert_eq!(
        created
            .sides
            .iter()
            .find(|s| s.id == side_b)
            .unwrap()
            .name
            .as_deref(),
        Some("Side B")
    );

    let renamed = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: side_b.clone(),
                name: Some("The Champions".to_string()),
            }]),
            ..Default::default()
        },
    )
    .await
    .expect("patch side name");
    let renamed_b = renamed.sides.iter().find(|s| s.id == side_b).unwrap();
    let untouched_a = renamed.sides.iter().find(|s| s.id == side_a).unwrap();
    assert_eq!(renamed_b.name.as_deref(), Some("The Champions"));
    assert_eq!(
        untouched_a.name.as_deref(),
        Some("Side A"),
        "a side not named in the request is left alone"
    );

    // Clearing side "a"'s custom name is safe (it still has a player to fall
    // back on) and reveals the priority chain's next entry: the sole
    // player's name.
    let cleared = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: side_a.clone(),
                name: None,
            }]),
            ..Default::default()
        },
    )
    .await
    .expect("clear side name");
    let cleared_a = cleared.sides.iter().find(|s| s.id == side_a).unwrap();
    assert_eq!(
        cleared_a.name.as_deref(),
        Some("Test User"),
        "clearing the custom name falls back to the sole player's name"
    );
}

/// The counterpart to `patch_match_renames_sides`'s side "a" case: side "b"
/// has no players and no team, so its custom name is the only thing keeping
/// it identifiable — clearing it (rather than replacing it) is rejected,
/// same as at create time.
#[tokio::test]
async fn clearing_the_name_of_an_empty_side_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = side_id_for_user(&created, &invitee.profile.id);
    let side_b = created
        .sides
        .iter()
        .find(|s| s.id != side_a)
        .expect("a second side")
        .id
        .clone();

    let response = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: side_b,
                name: None,
            }]),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "would have no players",
    );
}

/// Renaming a side that isn't part of the match is rejected.
#[tokio::test]
async fn patch_match_rename_unknown_side_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let response = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: "not-a-real-side".to_string(),
                name: Some("Ghost Team".to_string()),
            }]),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "not part of this match",
    );
}

/// The same team/name exclusivity rule enforced at create time
/// (`match_with_a_team_side_fans_out_to_team_followers`'s sibling case) also
/// applies to a post-creation rename: a side linked to a team can't be given
/// a custom name unless another side shares that team.
#[tokio::test]
async fn patch_match_rename_team_side_without_shared_team_is_rejected() {
    let (owner_config, owner) = new_user().await;
    let (_opponent_config, opponent) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Rename FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let mut input = match_between(
        "Team Rename Match",
        &[&owner.profile.id],
        &[&opponent.profile.id],
    );
    input.sides[0].team_id = Some(team.id.clone());
    input.sides[0].name = None;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    // Sides come back sorted by their server-assigned id, not input order, so
    // find the team-linked side by its `team_id` rather than assuming index 0.
    let team_side = created
        .sides
        .iter()
        .find(|s| s.team_id.as_deref() == Some(team.id.as_str()))
        .expect("team side present")
        .id
        .clone();

    let response = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: team_side,
                name: Some("Not Allowed".to_string()),
            }]),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "can't have both a name and a team",
    );
}

// ---------------------------------------------------------------------------
// Comments & likes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn comment_reply_edit_and_tombstone() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    // Top-level comment.
    let comment = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Great game".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");
    assert_eq!(comment.text.as_deref(), Some("Great game"));

    // Reply to it.
    let reply = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Agreed".to_string(),
            parent_id: Some(comment.id.clone()),
        },
    )
    .await
    .expect("create reply");
    assert_eq!(reply.parent_id.as_deref(), Some(comment.id.as_str()));

    // Edit the top-level comment.
    let edited = matches_match_id_comments_comment_id_patch(
        &config,
        &match_.id,
        &comment.id,
        models::UpdateCommentInput {
            text: "Great game!".to_string(),
        },
    )
    .await
    .expect("edit comment");
    assert_eq!(edited.text.as_deref(), Some("Great game!"));
    assert!(edited.edited_at.is_some());

    // Delete (tombstone) it.
    matches_match_id_comments_comment_id_delete(&config, &match_.id, &comment.id)
        .await
        .expect("delete comment");

    // The comment list still surfaces the tombstone (text cleared, deleted_at set).
    let list = matches_match_id_comments_get(&config, &match_.id, None, None)
        .await
        .expect("list comments");
    let tombstoned = list.items.iter().find(|c| c.id == comment.id);
    if let Some(c) = tombstoned {
        assert!(c.deleted_at.is_some());
        assert!(c.text.is_none());
    }
}

#[tokio::test]
async fn like_and_unlike_match() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    matches_match_id_likes_post(&config, &match_.id)
        .await
        .expect("like");

    let after_like = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert!(after_like.social.i_liked);
    assert_eq!(after_like.social.like_count, 1);

    matches_match_id_likes_delete(&config, &match_.id)
        .await
        .expect("unlike");

    let after_unlike = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert!(!after_unlike.social.i_liked);
    assert_eq!(after_unlike.social.like_count, 0);
}

/// Liking is idempotent and `like_count` is per-distinct-user: a repeated like by
/// the same user doesn't inflate the count, a repeated unlike doesn't drive it
/// negative, and each viewer sees their own `i_liked` independent of the total.
#[tokio::test]
async fn like_count_is_idempotent_and_per_user() {
    let (owner_config, _owner) = new_user().await;
    let (other_config, _other) = new_user().await;
    let (invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    // The owner likes twice — the second is a no-op (idempotent), so count == 1.
    matches_match_id_likes_post(&owner_config, &match_.id)
        .await
        .expect("like");
    matches_match_id_likes_post(&owner_config, &match_.id)
        .await
        .expect("like again (idempotent)");
    let after = matches_match_id_get(&owner_config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(
        after.social.like_count, 1,
        "a repeated like doesn't inflate"
    );
    assert!(after.social.i_liked);

    // A second, distinct user likes → count becomes 2.
    matches_match_id_likes_post(&other_config, &match_.id)
        .await
        .expect("other likes");
    let after_two = matches_match_id_get(&owner_config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(after_two.social.like_count, 2);

    // `i_liked` is per-viewer: the invitee (who hasn't liked) sees the same
    // count but i_liked == false.
    let invitee_view = matches_match_id_get(&invitee_config, &match_.id)
        .await
        .expect("invitee view");
    assert_eq!(invitee_view.social.like_count, 2);
    assert!(!invitee_view.social.i_liked, "invitee hasn't liked");

    // The owner unlikes twice — the second is a no-op and must not underflow.
    matches_match_id_likes_delete(&owner_config, &match_.id)
        .await
        .expect("unlike");
    matches_match_id_likes_delete(&owner_config, &match_.id)
        .await
        .expect("unlike again (idempotent)");
    let after_unlike = matches_match_id_get(&owner_config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(
        after_unlike.social.like_count, 1,
        "only the other user's like remains; count didn't underflow"
    );
    assert!(!after_unlike.social.i_liked);
}

/// `comment_count` tracks the total of top-level comments AND replies (both are
/// comments), while a parent's `reply_count` tracks only its replies. Posting a
/// top-level comment and a reply moves both counters.
#[tokio::test]
async fn comment_and_reply_counts_track_the_thread() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    assert_eq!(
        match_.social.comment_count, 0,
        "fresh match has no comments"
    );

    // Top-level comment → comment_count == 1.
    let parent = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Top level".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");
    assert_eq!(parent.reply_count, 0);
    let after_comment = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(after_comment.social.comment_count, 1);

    // Reply → comment_count == 2 (a reply is also a comment) and the parent's
    // reply_count == 1.
    matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "A reply".to_string(),
            parent_id: Some(parent.id.clone()),
        },
    )
    .await
    .expect("create reply");
    let after_reply = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(
        after_reply.social.comment_count, 2,
        "comment_count counts replies too"
    );

    // The parent's reply_count is reflected in the comment list.
    let comments = matches_match_id_comments_get(&config, &match_.id, None, None)
        .await
        .expect("list comments");
    let listed_parent = comments
        .items
        .iter()
        .find(|c| c.id == parent.id)
        .expect("parent in list");
    assert_eq!(listed_parent.reply_count, 1, "parent tracks its one reply");
}

/// Deleting a reply-less comment hard-deletes it: it leaves the list and
/// `comment_count` decrements back.
#[tokio::test]
async fn deleting_a_reply_less_comment_removes_it_and_decrements_count() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let comment = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Delete me".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");
    assert_eq!(
        matches_match_id_get(&config, &match_.id)
            .await
            .expect("get match")
            .social
            .comment_count,
        1
    );

    matches_match_id_comments_comment_id_delete(&config, &match_.id, &comment.id)
        .await
        .expect("delete comment");

    // Gone from the list entirely (no tombstone, since it had no replies).
    let comments = matches_match_id_comments_get(&config, &match_.id, None, None)
        .await
        .expect("list comments");
    assert!(
        !comments.items.iter().any(|c| c.id == comment.id),
        "a reply-less deleted comment is removed, not tombstoned"
    );
    // And the count is back to zero.
    assert_eq!(
        matches_match_id_get(&config, &match_.id)
            .await
            .expect("get match")
            .social
            .comment_count,
        0
    );
}

/// Deleting an already-deleted comment returns 404 rather than silently
/// succeeding — comments are delete-by-id (creating one always mints a new
/// id, never idempotent), unlike a follow/like toggle.
#[tokio::test]
async fn deleting_an_already_deleted_comment_returns_not_found() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let comment = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Delete me".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");

    matches_match_id_comments_comment_id_delete(&config, &match_.id, &comment.id)
        .await
        .expect("first delete");

    let response =
        matches_match_id_comments_comment_id_delete(&config, &match_.id, &comment.id).await;
    assert_not_found(response);
}

/// Deleting a comment that HAS replies tombstones it: the row is kept (so its
/// replies stay reachable) with text/author cleared, `comment_count` is
/// unchanged, and the replies remain listable under it.
#[tokio::test]
async fn deleting_a_comment_with_replies_tombstones_and_keeps_the_thread() {
    let (config, _owner) = new_user().await;
    let (replier_config, _replier) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let parent = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "Parent to be deleted".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");
    // A different user replies, so the parent has a reply from someone else.
    let reply = matches_match_id_comments_post(
        &replier_config,
        &match_.id,
        models::CreateCommentInput {
            text: "I'm a reply".to_string(),
            parent_id: Some(parent.id.clone()),
        },
    )
    .await
    .expect("create reply");

    // comment_count == 2 (parent + reply) before the delete.
    let before = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(before.social.comment_count, 2);

    // The author deletes the parent → tombstone (it has a reply).
    matches_match_id_comments_comment_id_delete(&config, &match_.id, &parent.id)
        .await
        .expect("delete parent");

    // Count is unchanged (tombstone keeps the row).
    let after = matches_match_id_get(&config, &match_.id)
        .await
        .expect("get match");
    assert_eq!(
        after.social.comment_count, 2,
        "tombstoning doesn't change comment_count"
    );

    // The parent is still listed, but as a tombstone: text/author cleared,
    // deleted_at set, and its reply_count preserved.
    let comments = matches_match_id_comments_get(&config, &match_.id, None, None)
        .await
        .expect("list comments");
    let tombstone = comments
        .items
        .iter()
        .find(|c| c.id == parent.id)
        .expect("tombstoned parent still listed");
    assert!(tombstone.deleted_at.is_some(), "parent is a tombstone");
    assert!(tombstone.text.is_none(), "tombstone text is cleared");
    assert!(tombstone.author.is_none(), "tombstone author is cleared");
    assert_eq!(tombstone.reply_count, 1, "reply_count survives tombstoning");

    // The reply is still reachable under the tombstoned parent.
    let replies = matches_match_id_comments_comment_id_replies_get(
        &config, &match_.id, &parent.id, None, None,
    )
    .await
    .expect("list replies");
    assert!(
        replies.items.iter().any(|c| c.id == reply.id),
        "the reply outlives its tombstoned parent"
    );
}

// ---------------------------------------------------------------------------
// Follows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn follow_and_unfollow_user() {
    let (follower_config, follower) = new_user().await;
    let (_followee_config, followee) = new_user().await;

    users_user_id_follow_post(&follower_config, &followee.profile.id)
        .await
        .expect("follow");

    // The followee's profile now reads as followed-by-me for the follower.
    let profile = users_user_id_get(&follower_config, &followee.profile.id)
        .await
        .expect("get profile");
    assert!(profile.is_followed_by_me);
    assert_eq!(profile.follower_count, 1);

    // The followee lists the follower among its followers.
    let followers = users_user_id_followers_get(&follower_config, &followee.profile.id, None, None)
        .await
        .expect("followers");
    assert!(followers.items.iter().any(|u| u.id == follower.profile.id));

    users_user_id_follow_delete(&follower_config, &followee.profile.id)
        .await
        .expect("unfollow");

    let profile = users_user_id_get(&follower_config, &followee.profile.id)
        .await
        .expect("get profile");
    assert!(!profile.is_followed_by_me);
    assert_eq!(profile.follower_count, 0);
}

#[tokio::test]
async fn follow_and_unfollow_team() {
    let (owner_config, _owner) = new_user().await;
    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Followable".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let (follower_config, _follower) = new_user().await;
    teams_team_id_follow_post(&follower_config, &team.id)
        .await
        .expect("follow team");

    let fetched = teams_team_id_get(&follower_config, &team.id)
        .await
        .expect("get team");
    assert!(fetched.is_followed_by_me);
    assert_eq!(fetched.follower_count, 1);

    teams_team_id_follow_delete(&follower_config, &team.id)
        .await
        .expect("unfollow team");

    let fetched = teams_team_id_get(&follower_config, &team.id)
        .await
        .expect("get team");
    assert!(!fetched.is_followed_by_me);
}

// ---------------------------------------------------------------------------
// Invitations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn match_invitation_appears_in_inbox_and_can_be_accepted() {
    let (owner_config, _owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;

    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    // The invitee sees the invitation in their inbox.
    let inbox = users_me_invitations_get(&invitee_config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| {
            matches!(&*i.context,
            models::InvitationContext::Match(ctx) if ctx.match_id == match_.id)
        })
        .expect("match invitation in inbox");

    // The invitee accepts it.
    let responded = invitations_invitation_id_respond_post(
        &invitee_config,
        &detail.invitation.id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept invitation");
    assert!(matches!(
        responded.status,
        models::InvitationStatus::Accepted
    ));
}

/// End-to-end for a *normal* (user-addressed) invite acceptance into an
/// already-completed match: after the invitee accepts, the match must land on
/// their feed and their stats must credit the played match.
///
/// - Feed row is written synchronously inside the accept transaction, so it's
///   present essentially immediately (polled anyway to avoid read races).
/// - Stats are reconciled asynchronously by the accept saga (a roster link
///   doesn't touch match `#META`, so only the saga credits the newly-linked
///   player) — hence the eventual assertion.
#[tokio::test]
async fn accepting_a_normal_invite_updates_feed_and_stats() {
    let (owner_config, _owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;

    // A completed match with the invitee invited (by user id) onto the losing
    // side "b". Until they accept, they didn't "play", so no stat is credited.
    let created = matches_post(
        &owner_config,
        completed_match(vec![invite_users("b", &[&invitee.profile.id])]),
    )
    .await
    .expect("create match");
    assert!(matches!(created.status, models::MatchStatus::Completed));

    // Find the invitation in the invitee's inbox and accept it.
    let inbox = users_me_invitations_get(&invitee_config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| {
            matches!(&*i.context,
            models::InvitationContext::Match(ctx) if ctx.match_id == created.id)
        })
        .expect("match invitation in inbox");
    let responded = invitations_invitation_id_respond_post(
        &invitee_config,
        &detail.invitation.id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept invitation");
    assert!(matches!(
        responded.status,
        models::InvitationStatus::Accepted
    ));

    // The match is now on the accepter's feed...
    assert_match_reaches_feed(&invitee_config, &created.id, "invitee's feed").await;

    // Stats aren't credited yet: the creator's score is still an unconfirmed
    // submission, and an unconfirmed game doesn't count as played.
    assert_eq!(
        my_matches_played(&invitee_config, models::MatchType::Tennis).await,
        0,
        "an unconfirmed score shouldn't count toward matches played"
    );

    // Now the invitee (side "b") confirms the creator's submitted score.
    let submission_id = created
        .pending_score
        .as_ref()
        .expect("pending score at create time")
        .submission_id
        .clone();
    matches_match_id_score_submissions_submission_id_respond_post(
        &invitee_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm score");

    // ...and their stats now credit the confirmed match they played in (they
    // were on the losing side, so one played, zero wins, one loss).
    assert_matches_played_reaches(&invitee_config, models::MatchType::Tennis, 1, "invitee").await;
    let stats = users_me_get(&invitee_config)
        .await
        .expect("get me")
        .profile
        .stats;
    let tennis = stats.tennis.expect("a tennis stat row");
    assert_eq!(
        tennis.win_percentage,
        Some(0.0),
        "invitee was on the losing side"
    );
    assert_eq!(tennis.losses, 1, "invitee was on the losing side");
}

/// End-to-end for the *invite-link* (bearer token) acceptance flow into an
/// already-completed match: an external invitee is created with a minted token,
/// a real account then accepts by token, and afterwards the match is on that
/// account's feed and their stats credit the played match.
#[tokio::test]
async fn accepting_a_link_invite_updates_feed_and_stats() {
    let (owner_config, _owner) = new_user().await;

    // A completed match with an EXTERNAL invitee (by name) on side "b". The
    // external player carries a minted invite token — the link credential.
    let created = matches_post(
        &owner_config,
        completed_match(vec![invite_externals("b", &["Ringer Rita"])]),
    )
    .await
    .expect("create match");
    let token = external_invite_token(&created);

    // A brand-new real account accepts the invitation by its token. This binds
    // the account onto the previously-userless invitation and links the roster
    // entry, writing their feed row synchronously.
    let (accepter_config, _accepter) = new_user().await;
    let responded = invitations_respond_by_token_post(
        &accepter_config,
        models::RespondByTokenInput {
            invite_token: token,
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept by token");
    assert!(matches!(
        responded.status,
        models::InvitationStatus::Accepted
    ));

    // The match is on the accepter's feed, but their stats aren't credited
    // yet: the creator's score is still an unconfirmed submission.
    assert_match_reaches_feed(&accepter_config, &created.id, "token-accepter's feed").await;
    assert_eq!(
        my_matches_played(&accepter_config, models::MatchType::Tennis).await,
        0,
        "an unconfirmed score shouldn't count toward matches played"
    );

    // The accepter (side "b") confirms the creator's submitted score, and only
    // then does it credit the match.
    let submission_id = created
        .pending_score
        .as_ref()
        .expect("pending score at create time")
        .submission_id
        .clone();
    matches_match_id_score_submissions_submission_id_respond_post(
        &accepter_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm score");
    assert_matches_played_reaches(
        &accepter_config,
        models::MatchType::Tennis,
        1,
        "token-accepter",
    )
    .await;
}

// ---------------------------------------------------------------------------
// Sport-specific lifetime stats (cricket/football box-score counters, best
// figures, draws)
// ---------------------------------------------------------------------------
//
// `accepting_a_normal_invite_updates_feed_and_stats`/
// `accepting_a_link_invite_updates_feed_and_stats` above already cover the
// core matches_played/win_percentage path for a simple (tennis) score. These
// cover what's specific to this feature: cricket/football's per-player box
// score counters, derived rates, personal-best figures (including that they
// only ever ratchet up), and draws.

/// A cricket match's confirmed score credits every batting/bowling/fielding
/// counter, the derived rates, and both best-figures — all from one
/// scorecard, so a single reconcile is enough to exercise the full
/// `CricketPlayerStats` shape.
#[tokio::test]
async fn cricket_stats_track_batting_bowling_fielding_and_derived_rates() {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    let mut input = match_between(
        "Cricket Match",
        &[&owner.profile.id],
        &[&opponent.profile.id],
    );
    input.match_type = models::MatchType::Cricket;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    accept_match_invitation(&opponent_config, &created.id).await;

    let side_a = side_id_for_user(&created, &owner.profile.id);
    let side_b = side_id_for_user(&created, &opponent.profile.id);
    let owner_pid = player_id_for_user(&created, &owner.profile.id);
    let opponent_pid = player_id_for_user(&created, &opponent.profile.id);

    // Innings 1: owner's side bats. Owner scores a half-century (4 fours, 2
    // sixes, 40 balls) and is bowled out.
    let mut innings1 = models::CricketScoreInnings::new(
        side_a.clone(),
        side_b.clone(),
        140,
        4,
        models::Overs::new(20, 0),
        false,
    );
    let mut owner_batting = models::CricketBattingEntry::new(owner_pid.clone(), 50, 40, 4, 2);
    owner_batting.dismissal = Some(Box::new(models::CricketDismissal::new(
        models::CricketDismissalKind::Bowled,
    )));
    innings1.batting = Some(vec![owner_batting]);

    // Innings 2: opponent's side bats, owner bowls and fields. Owner takes 3
    // wickets for 25 off 8.4 overs, plus a catch off a wicket bowled by
    // someone else (a catch credits the fielder regardless of who bowled it).
    let mut innings2 = models::CricketScoreInnings::new(
        side_b.clone(),
        side_a.clone(),
        100,
        5,
        models::Overs::new(20, 0),
        false,
    );
    innings2.bowling = Some(vec![models::CricketBowlingEntry::new(
        owner_pid.clone(),
        models::Overs::new(8, 4),
        0,
        25,
        3,
        0,
        0,
    )]);
    let mut opponent_batting = models::CricketBattingEntry::new(opponent_pid.clone(), 10, 15, 1, 0);
    let mut caught_by_owner = models::CricketDismissal::new(models::CricketDismissalKind::Caught);
    caught_by_owner.fielder_player_id = Some(owner_pid.clone());
    opponent_batting.dismissal = Some(Box::new(caught_by_owner));
    innings2.batting = Some(vec![opponent_batting]);

    let cricket_score = models::Score::Cricket(Box::new(models::ScoreCricketScore::new(
        vec![innings1, innings2],
        std::collections::HashMap::new(),
        Default::default(),
    )));

    let updated = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(cricket_score)),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch cricket score");
    let submission_id = updated
        .pending_score
        .expect("pending score after patch")
        .submission_id;
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm cricket score");

    assert_matches_played_reaches(&owner_config, models::MatchType::Cricket, 1, "owner").await;
    let profile = users_me_get(&owner_config).await.expect("get me").profile;
    let cricket = profile.stats.cricket.expect("cricket stats");

    assert_eq!(cricket.matches_played, 1);
    assert_eq!(cricket.wins, 1);
    assert_eq!(cricket.draws, 0);
    assert_eq!(cricket.losses, 0);

    assert_eq!(cricket.runs, 50, "batting runs");
    assert_eq!(cricket.fours, 4);
    assert_eq!(cricket.sixes, 2);
    assert_eq!(cricket.balls_faced, 40);
    assert_eq!(cricket.dismissals, 1, "out once, in innings 1");
    assert_eq!(cricket.catches, 1, "the catch taken in innings 2");

    assert_eq!(cricket.wickets, 3, "bowling wickets");
    assert_eq!(cricket.runs_conceded, 25);
    assert_eq!(cricket.overs_bowled.overs, 8);
    assert_eq!(cricket.overs_bowled.balls, 4);

    assert_eq!(cricket.strike_rate, Some(125.0), "50 runs off 40 balls");
    assert_eq!(cricket.batting_average, Some(50.0), "50 runs, 1 dismissal");
    let economy = cricket
        .economy
        .expect("economy computed with balls bowled > 0");
    assert!(
        (economy - 2.884_615).abs() < 0.01,
        "25 runs off 52 balls (8.667 overs) ~= 2.88 economy, got {economy}"
    );

    let best_runs = cricket.best_runs.expect("best runs recorded");
    assert_eq!(best_runs.value, 50);
    assert_eq!(best_runs.match_id, created.id);

    let best_bowling = cricket.best_bowling.expect("best bowling recorded");
    assert_eq!(best_bowling.wickets, 3);
    assert_eq!(best_bowling.runs_conceded, 25);
    assert_eq!(best_bowling.overs.overs, 8);
    assert_eq!(best_bowling.overs.balls, 4);
    assert_eq!(best_bowling.match_id, created.id);
}

/// `best_bowling` only ever ratchets up: a worse spell in a later match must
/// not overwrite an existing record, and a better one must — while the
/// career `wickets` total keeps accumulating regardless either way. This is
/// the behavior `Dao::update_best_bowling_figures` exists for, so it's worth
/// covering across more than one match rather than just a single snapshot.
#[tokio::test]
async fn best_bowling_figures_only_ratchet_up_across_matches() {
    let (bowler_config, bowler) = new_user().await;

    // Match 1: 3/25 off 8.4 overs — the first record.
    play_cricket_bowling_match(
        &bowler_config,
        &bowler.profile.id,
        3,
        25,
        models::Overs::new(8, 4),
    )
    .await;
    assert_matches_played_reaches(&bowler_config, models::MatchType::Cricket, 1, "bowler").await;
    let cricket = users_me_get(&bowler_config)
        .await
        .expect("get me")
        .profile
        .stats
        .cricket
        .expect("cricket stats after match 1");
    let best = cricket.best_bowling.expect("best bowling after match 1");
    assert_eq!(
        (
            best.wickets,
            best.runs_conceded,
            best.overs.overs,
            best.overs.balls
        ),
        (3, 25, 8, 4)
    );

    // Match 2: a WORSE spell (1/40) must not overwrite the record, even
    // though the career wickets total still accumulates.
    play_cricket_bowling_match(
        &bowler_config,
        &bowler.profile.id,
        1,
        40,
        models::Overs::new(10, 0),
    )
    .await;
    assert_matches_played_reaches(&bowler_config, models::MatchType::Cricket, 2, "bowler").await;
    let cricket = users_me_get(&bowler_config)
        .await
        .expect("get me")
        .profile
        .stats
        .cricket
        .expect("cricket stats after match 2");
    let best = cricket.best_bowling.expect("best bowling still set");
    assert_eq!(
        (
            best.wickets,
            best.runs_conceded,
            best.overs.overs,
            best.overs.balls
        ),
        (3, 25, 8, 4),
        "a worse spell must not overwrite the existing record"
    );
    assert_eq!(
        cricket.wickets, 4,
        "career wickets keep accumulating: 3 + 1"
    );

    // Match 3: a BETTER spell (5/10) must overwrite the record.
    play_cricket_bowling_match(
        &bowler_config,
        &bowler.profile.id,
        5,
        10,
        models::Overs::new(6, 0),
    )
    .await;
    assert_matches_played_reaches(&bowler_config, models::MatchType::Cricket, 3, "bowler").await;
    let cricket = users_me_get(&bowler_config)
        .await
        .expect("get me")
        .profile
        .stats
        .cricket
        .expect("cricket stats after match 3");
    let best = cricket.best_bowling.expect("best bowling after match 3");
    assert_eq!(
        (
            best.wickets,
            best.runs_conceded,
            best.overs.overs,
            best.overs.balls
        ),
        (5, 10, 6, 0),
        "a better spell must overwrite the existing record"
    );
    assert_eq!(cricket.wickets, 9, "career wickets: 3 + 1 + 5");
}

/// A football match's goal log credits goals and assists separately, excludes
/// own goals from the scorer's own tally, and tracks both `best_goals` and
/// `best_goal_contributions` (goals + assists in the same match).
#[tokio::test]
async fn football_stats_track_goals_assists_and_best_goal_contributions() {
    let (owner_config, owner) = new_user().await;
    let (teammate_config, teammate) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    let mut input = match_between(
        "Football Match",
        &[&owner.profile.id, &teammate.profile.id],
        &[&opponent.profile.id],
    );
    input.match_type = models::MatchType::Football;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    accept_match_invitation(&teammate_config, &created.id).await;
    accept_match_invitation(&opponent_config, &created.id).await;

    let side_a = side_id_for_user(&created, &owner.profile.id);
    let side_b = side_id_for_user(&created, &opponent.profile.id);
    let owner_pid = player_id_for_user(&created, &owner.profile.id);
    let teammate_pid = player_id_for_user(&created, &teammate.profile.id);
    let opponent_pid = player_id_for_user(&created, &opponent.profile.id);

    // Owner scores twice, assists the teammate's goal, and the opponent puts
    // one in their own net (which must NOT count toward the opponent's own
    // "goals").
    let mut goal1 = models::FootballGoalEvent::new(side_a.clone(), false, false);
    goal1.scorer_player_id = Some(owner_pid.clone());
    let mut goal2 = models::FootballGoalEvent::new(side_a.clone(), false, false);
    goal2.scorer_player_id = Some(owner_pid.clone());
    let mut goal3 = models::FootballGoalEvent::new(side_a.clone(), false, false);
    goal3.scorer_player_id = Some(teammate_pid.clone());
    goal3.assist_player_id = Some(owner_pid.clone());
    let mut own_goal = models::FootballGoalEvent::new(side_a.clone(), true, false);
    own_goal.scorer_player_id = Some(opponent_pid.clone());

    let score_tally = std::collections::HashMap::from([(side_a.clone(), 4), (side_b.clone(), 0)]);
    let mut football_score = models::ScoreFootballScore::new(
        score_tally,
        std::collections::HashMap::new(),
        Default::default(),
    );
    football_score.goals = Some(vec![goal1, goal2, goal3, own_goal]);

    let updated = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(models::Score::Football(Box::new(football_score)))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch football score");
    let submission_id = updated
        .pending_score
        .expect("pending score after patch")
        .submission_id;
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm football score");

    assert_matches_played_reaches(&owner_config, models::MatchType::Football, 1, "owner").await;
    let profile = users_me_get(&owner_config).await.expect("get me").profile;
    let football = profile.stats.football.expect("football stats");
    assert_eq!(football.wins, 1);
    assert_eq!(football.goals, 2);
    assert_eq!(football.assists, 1);

    let best_goals = football.best_goals.expect("best goals recorded");
    assert_eq!(best_goals.value, 2);
    let best_contributions = football
        .best_goal_contributions
        .expect("best goal contributions recorded");
    assert_eq!(best_contributions.value, 3, "2 goals + 1 assist");

    assert_matches_played_reaches(&opponent_config, models::MatchType::Football, 1, "opponent")
        .await;
    let opponent_profile = users_me_get(&opponent_config)
        .await
        .expect("get me")
        .profile;
    let opponent_football = opponent_profile.stats.football.expect("football stats");
    assert_eq!(opponent_football.losses, 1);
    assert_eq!(
        opponent_football.goals, 0,
        "an own goal must not credit the scoring-against player's own goals tally"
    );
}

/// A match confirmed with no winner is a draw for every participant: neither
/// side's win nor loss is credited, only `draws`.
#[tokio::test]
async fn a_drawn_match_credits_draws_not_wins_or_losses() {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    let created = matches_post(
        &owner_config,
        match_between("Drawn Match", &[&owner.profile.id], &[&opponent.profile.id]),
    )
    .await
    .expect("create match");
    accept_match_invitation(&opponent_config, &created.id).await;

    let side_a = side_id_for_user(&created, &owner.profile.id);
    let side_b = side_id_for_user(&created, &opponent.profile.id);

    let updated = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 2, 2))),
            winner_side_id: None,
            ..Default::default()
        },
    )
    .await
    .expect("patch drawn score");
    let submission_id = updated
        .pending_score
        .expect("pending score after patch")
        .submission_id;
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm drawn score");

    for (config, whose) in [(&owner_config, "owner"), (&opponent_config, "opponent")] {
        assert_matches_played_reaches(config, models::MatchType::Tennis, 1, whose).await;
        let profile = users_me_get(config).await.expect("get me").profile;
        let tennis = profile.stats.tennis.expect("tennis stats");
        assert_eq!(tennis.matches_played, 1, "{whose}");
        assert_eq!(tennis.wins, 0, "{whose} drew, so no win");
        assert_eq!(tennis.draws, 1, "{whose} drew");
        assert_eq!(tennis.losses, 0, "{whose} drew, so no loss");
    }
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

#[tokio::test]
async fn notifications_endpoints_respond() {
    let (config, _user) = new_user().await;

    // A fresh user has an empty, well-formed notifications page and zero unread.
    let page = notifications_get(&config, None, None)
        .await
        .expect("list notifications");
    assert!(page.items.is_empty());

    let unread = notifications_unread_count_get(&config)
        .await
        .expect("unread count");
    assert_eq!(unread.unread_count, 0);

    // Mark-all-read is idempotent on an empty inbox.
    notifications_read_post(&config)
        .await
        .expect("mark all read");
}

// ---------------------------------------------------------------------------
// Search & feed (shape smoke tests — content depends on the async worker)
// ---------------------------------------------------------------------------

/// A created match fans out into its participants' feeds. Exercises the full
/// async path: match write → DynamoDB stream → SQS → worker → Temporal
/// `FanOutMatch` workflow → `write_feed_items` → the match appears in the feed.
///
/// We assert on the *invitee's* feed: their player record carries `user_id` from
/// creation (before acceptance), so the fan-out audience includes them and the
/// match should surface without any further action. Eventual — polls until the
/// pipeline delivers, matching the other async tests.
#[tokio::test]
async fn creating_a_match_fans_out_to_a_participants_feed() {
    let (owner_config, _owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let found = eventually("match to fan out into the invitee's feed", || {
        let config = &invitee_config;
        let match_id = &created.id;
        async move {
            let page = feed_get(config, None, None, None, None).await.ok()?;
            page.items.into_iter().find(|item| &item.id == match_id)
        }
    })
    .await;
    assert_eq!(found.id, created.id);
    assert_eq!(found.name, "Test Match");
}

// ---------------------------------------------------------------------------
// Feed fan-out scenarios (multi-user end-to-end)
//
// The fan-out audience for a match is the deduplicated union of (see
// docs/async-design.md / agon_core `resolve_fanout_audience`):
//   - the participants themselves (players with a linked user id),
//   - every follower of each participating user,
//   - every follower of each involved team (a side with a team id).
// So a match lands in a viewer's feed iff they participate, follow a
// participant, or follow an involved team. These tests drive that whole path
// through the real stream -> SQS -> worker -> feed pipeline and assert both the
// users who SHOULD receive the match and those who should NOT.
//
// Note on the creator: on this surface the creator is only a participant if they
// are themselves a player (a self-invite via `match_between`). A creator who
// merely invites others is not in the audience, and neither are their followers
// — the tests reflect that.
// ---------------------------------------------------------------------------

/// The scenario from the ask: two users follow a third (the poster), the poster
/// creates a match against a fourth user, and a fifth user follows nobody.
///
/// Expected feed membership:
///   - poster (participant via self-invite): yes
///   - opponent (participant): yes
///   - the poster's two followers: yes
///   - the unrelated fifth user (follows nobody involved): no
#[tokio::test]
async fn match_fans_out_to_poster_and_their_followers_but_not_strangers() {
    let (poster_config, poster) = new_user().await;
    let (opponent_config, opponent) = new_user().await;
    let (follower1_config, _follower1) = new_user().await;
    let (follower2_config, _follower2) = new_user().await;
    let (stranger_config, _stranger) = new_user().await;

    // follower1 and follower2 follow the poster; the stranger follows no one.
    users_user_id_follow_post(&follower1_config, &poster.profile.id)
        .await
        .expect("follower1 follows poster");
    users_user_id_follow_post(&follower2_config, &poster.profile.id)
        .await
        .expect("follower2 follows poster");

    // Poster creates a match, putting themselves on side "a" (self-invite makes
    // them a participant so their followers are fanned out to) and the opponent
    // on side "b".
    let created = matches_post(
        &poster_config,
        match_between(
            "Followers Feed Match",
            &[&poster.profile.id],
            &[&opponent.profile.id],
        ),
    )
    .await
    .expect("create match");

    // Everyone in the audience should eventually see it.
    assert_match_reaches_feed(&poster_config, &created.id, "poster's own feed").await;
    assert_match_reaches_feed(&opponent_config, &created.id, "opponent's feed").await;
    assert_match_reaches_feed(&follower1_config, &created.id, "follower1's feed").await;
    assert_match_reaches_feed(&follower2_config, &created.id, "follower2's feed").await;

    // The stranger follows nobody involved, so it must never reach their feed.
    // We only assert absence AFTER the fan-out has demonstrably completed (the
    // participants above have received it), so this isn't just racing the
    // pipeline.
    assert_match_absent_from_feed(&stranger_config, &created.id, "stranger's feed").await;
}

/// Fan-out is the union across *all* participants' followers, not just the
/// creator's: a user who follows the opponent (and not the poster) still
/// receives the match, while a user who follows neither does not.
#[tokio::test]
async fn match_fans_out_to_followers_of_any_participant() {
    let (poster_config, poster) = new_user().await;
    let (opponent_config, opponent) = new_user().await;
    let (opp_follower_config, _opp_follower) = new_user().await;
    let (unrelated_config, _unrelated) = new_user().await;

    // This follower follows the OPPONENT, not the poster.
    users_user_id_follow_post(&opp_follower_config, &opponent.profile.id)
        .await
        .expect("follow opponent");

    let created = matches_post(
        &poster_config,
        match_between(
            "Union Fanout Match",
            &[&poster.profile.id],
            &[&opponent.profile.id],
        ),
    )
    .await
    .expect("create match");

    // The opponent's follower receives it (union of all participants' followers).
    assert_match_reaches_feed(&opp_follower_config, &created.id, "opponent-follower feed").await;
    // And the opponent themselves, as a participant.
    assert_match_reaches_feed(&opponent_config, &created.id, "opponent feed").await;
    // A user following neither participant does not.
    assert_match_absent_from_feed(&unrelated_config, &created.id, "unrelated feed").await;
}

/// Following the poster only feeds you their *future* matches, not ones created
/// before you followed — feed fan-out happens at creation time against the
/// then-current follower set. A late follower doesn't retroactively receive it.
#[tokio::test]
async fn following_after_a_match_is_created_does_not_backfill_the_feed() {
    let (poster_config, poster) = new_user().await;
    let (opponent_config, opponent) = new_user().await;
    let (late_follower_config, _late_follower) = new_user().await;

    // Match is created BEFORE the late follower follows.
    let created = matches_post(
        &poster_config,
        match_between(
            "Pre-Follow Match",
            &[&poster.profile.id],
            &[&opponent.profile.id],
        ),
    )
    .await
    .expect("create match");

    // Confirm fan-out completed by waiting for a participant to receive it.
    assert_match_reaches_feed(&opponent_config, &created.id, "opponent feed").await;

    // Now they follow the poster — after the fact.
    users_user_id_follow_post(&late_follower_config, &poster.profile.id)
        .await
        .expect("late follow");

    // The already-created match is not backfilled into the late follower's feed.
    assert_match_absent_from_feed(&late_follower_config, &created.id, "late-follower feed").await;
}

/// Unfollowing before the match is created removes you from the fan-out
/// audience: a former follower does not receive the poster's new match.
#[tokio::test]
async fn unfollowing_removes_you_from_future_fan_out() {
    let (poster_config, poster) = new_user().await;
    let (opponent_config, opponent) = new_user().await;
    let (ex_follower_config, _ex_follower) = new_user().await;

    // Follow, then unfollow, before any match exists.
    users_user_id_follow_post(&ex_follower_config, &poster.profile.id)
        .await
        .expect("follow");
    users_user_id_follow_delete(&ex_follower_config, &poster.profile.id)
        .await
        .expect("unfollow");

    let created = matches_post(
        &poster_config,
        match_between(
            "Post-Unfollow Match",
            &[&poster.profile.id],
            &[&opponent.profile.id],
        ),
    )
    .await
    .expect("create match");

    // The participant receives it (fan-out ran)...
    assert_match_reaches_feed(&opponent_config, &created.id, "opponent feed").await;
    // ...but the ex-follower, no longer following at creation time, does not.
    assert_match_absent_from_feed(&ex_follower_config, &created.id, "ex-follower feed").await;
}

/// Team fan-out: a match with a team on one side reaches that team's followers,
/// even when they don't follow any of the individual players.
#[tokio::test]
async fn match_with_a_team_side_fans_out_to_team_followers() {
    let (owner_config, owner) = new_user().await;
    let (_opponent_config, opponent) = new_user().await;
    let (team_follower_config, _team_follower) = new_user().await;
    let (stranger_config, _stranger) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Fanout FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    // team_follower follows the TEAM, not any player.
    teams_team_id_follow_post(&team_follower_config, &team.id)
        .await
        .expect("follow team");

    // A match with the team on side "a" and an individual opponent on side "b".
    // The owner self-invites onto the team side so the match has a real player,
    // and side "a" carries the team id so team followers are in the audience.
    let mut input = match_between(
        "Team Fanout Match",
        &[&owner.profile.id],
        &[&opponent.profile.id],
    );
    // A side can't carry both a client-supplied name and a team_id unless
    // another side shares that team (disambiguation) — neither applies here,
    // so drop the placeholder name `match_between` set for side "a".
    input.sides[0].team_id = Some(team.id.clone());
    input.sides[0].name = None;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");

    // The team's follower receives it via team fan-out.
    assert_match_reaches_feed(&team_follower_config, &created.id, "team-follower feed").await;
    // A stranger following neither the team nor any player does not.
    assert_match_absent_from_feed(&stranger_config, &created.id, "stranger feed").await;
}

// ---------------------------------------------------------------------------
// Async pipeline (eventual consistency) — exercises the real stream -> SQS ->
// worker path. These assert effects that land *after* the synchronous write.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_created_user_becomes_searchable() {
    // Give the user a unique, searchable name so the query can't match anyone
    // else in a shared staging environment.
    let subject = Uuid::new_v4().to_string();
    let config = config_for(&subject);
    let unique = format!("Zephyrine{}", subject.replace('-', ""));
    let user = users_post(
        &config,
        models::CreateUserInput {
            name: unique.clone(),
        },
    )
    .await
    .expect("create user");

    // The worker indexes the user into Meilisearch off the DynamoDB stream —
    // eventually. Poll until the search finds them by their unique name.
    let found = eventually("created user to be searchable", || {
        let config = &config;
        let unique = &unique;
        let target = &user.profile.id;
        async move {
            let results = users_search_get(config, unique).await.ok()?;
            results.into_iter().find(|u| &u.id == target)
        }
    })
    .await;
    assert_eq!(found.id, user.profile.id);
}

#[tokio::test]
async fn following_a_user_eventually_notifies_them() {
    let (follower_config, follower) = new_user().await;
    let (followee_config, followee) = new_user().await;

    users_user_id_follow_post(&follower_config, &followee.profile.id)
        .await
        .expect("follow");

    // The worker generates a Follow notification for the followee off the stream.
    // Poll the followee's notifications until it appears, with the follower as
    // the actor.
    let notif = eventually("follow notification to be generated", || {
        let config = &followee_config;
        let follower_id = &follower.profile.id;
        async move {
            let page = notifications_get(config, None, None).await.ok()?;
            page.items.into_iter().find(|n| match &*n.kind {
                models::NotificationKind::Follow(f) => &f.follower.id == follower_id,
                _ => false,
            })
        }
    })
    .await;
    assert!(!notif.is_read, "a fresh notification is unread");

    // And the unread badge count reflects it.
    let unread = notifications_unread_count_get(&followee_config)
        .await
        .expect("unread count");
    assert!(unread.unread_count >= 1);
}

#[tokio::test]
async fn being_invited_to_a_match_eventually_notifies_you() {
    let (owner_config, owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;

    // Owner creates a match inviting the other user (create_match_input invites
    // `invitee` onto side "a").
    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    // The worker generates a MatchInvitation notification for the invitee off the
    // invitation write. Poll until it appears, referencing this match with the
    // owner as the inviter.
    let notif = eventually("match invitation notification to be generated", || {
        let config = &invitee_config;
        let match_id = &match_.id;
        let inviter_id = &owner.profile.id;
        async move {
            let page = notifications_get(config, None, None).await.ok()?;
            page.items.into_iter().find(|n| match &*n.kind {
                models::NotificationKind::MatchInvitation(m) => {
                    &m.match_id == match_id && &m.inviter.id == inviter_id
                }
                _ => false,
            })
        }
    })
    .await;
    assert!(!notif.is_read, "a fresh notification is unread");
}

// ---------------------------------------------------------------------------
// Match scoring (PATCH a score -> completes match + records a submission)
// ---------------------------------------------------------------------------

/// A simple score for the two default sides ("a"/"b" client ids map to real
/// side ids on the created match, so we read them off the created match).
fn simple_score(side_a: &str, side_b: &str, a: i32, b: i32) -> models::Score {
    models::Score::Simple(Box::new(models::ScoreSimpleScore {
        entries: std::collections::HashMap::from([
            (side_a.to_string(), a),
            (side_b.to_string(), b),
        ]),
        r#type: Default::default(),
    }))
}

/// Extract `(side_id, points)` pairs from a simple score, sorted by side id so
/// the result is stable regardless of the underlying `HashMap`'s iteration
/// order — callers should compare against a [`sorted_points`] expectation.
fn simple_score_points(score: &models::Score) -> Vec<(String, i32)> {
    let mut points: Vec<(String, i32)> = match score {
        models::Score::Simple(s) => s
            .entries
            .iter()
            .map(|(side_id, points)| (side_id.clone(), *points))
            .collect(),
        models::Score::Sets(_) => panic!("expected a simple score"),
        models::Score::Cricket(_) => panic!("expected a simple score"),
        models::Score::Football(_) => panic!("expected a simple score"),
        models::Score::Netball(_) => panic!("expected a simple score"),
    };
    points.sort_by(|a, b| a.0.cmp(&b.0));
    points
}

/// Sort a literal `(side_id, points)` expectation the same way
/// [`simple_score_points`] sorts its output, so the two can be compared with
/// `assert_eq!` regardless of which side id happens to sort first.
fn sorted_points(mut points: Vec<(String, i32)>) -> Vec<(String, i32)> {
    points.sort_by(|a, b| a.0.cmp(&b.0));
    points
}

#[tokio::test]
async fn scoring_a_match_completes_it_and_records_a_pending_submission() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    // The scorer must be an assigned participant, so put the owner on side "a".
    let mut input = create_match_input(&invitee.profile.id);
    input.creator_side_client_id = Some("a".to_string());
    let created = matches_post(&config, input).await.expect("create match");
    assert!(matches!(created.status, models::MatchStatus::Scheduled));
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // PATCH a score: this completes the match and records a submission, but the
    // score itself is PENDING (not confirmed) until the other side confirms it —
    // same as a create-time score.
    let updated = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 3))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch score");
    assert!(matches!(updated.status, models::MatchStatus::Completed));
    assert!(
        updated.confirmed_score.is_none(),
        "a PATCHed score awaits the other side's confirmation, not confirmed immediately"
    );
    assert!(updated.pending_score.is_some());

    // The submission is visible in the match's submission history, as pending.
    let submissions = matches_match_id_score_submissions_get(&config, &created.id)
        .await
        .expect("list submissions");
    assert_eq!(submissions.len(), 1, "a submission was recorded");
    assert!(matches!(
        submissions[0].status,
        models::ScoreSubmissionStatus::Pending
    ));
}

#[tokio::test]
async fn rejecting_a_score_and_resubmitting_requires_approval_again() {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    // Owner plays on side "a", opponent is invited (and accepts) onto side "b".
    let mut input = create_match_input(&opponent.profile.id);
    input.invites = vec![models::CreateMatchInviteInput {
        side_client_id: Some("b".to_string()),
        invited_user_ids: vec![opponent.profile.id.clone()],
        invited_externals: vec![],
    }];
    input.starts_at = iso_offset_hours(-2);
    input.creator_side_client_id = Some("a".to_string());
    input.score = Some(Box::new(simple_score("a", "b", 6, 3)));
    input.winner_side_id = Some("a".to_string());

    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();
    assert!(created.confirmed_score.is_none(), "score starts pending");

    let first_submission_id = created
        .pending_score
        .as_ref()
        .expect("a pending score was recorded at create time")
        .submission_id
        .clone();

    // The opponent rejects (disputes) the submitted score.
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &first_submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Dispute,
        },
    )
    .await
    .expect("dispute score");

    let after_dispute = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    assert!(after_dispute.confirmed_score.is_none());
    assert!(
        after_dispute.pending_score.is_none(),
        "a disputed submission clears the pending score"
    );

    // The owner submits a new score via PATCH. It must NOT show up as confirmed
    // immediately — the opponent still has to approve/reject it, just like the
    // first submission.
    let resubmitted = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 4))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch new score");
    assert!(
        resubmitted.confirmed_score.is_none(),
        "a resubmitted score must await confirmation, not appear confirmed immediately"
    );
    let pending = resubmitted
        .pending_score
        .expect("the resubmission is pending");
    assert_ne!(
        pending.submission_id, first_submission_id,
        "the resubmission is a fresh submission, not the disputed one"
    );

    // The submission history now has both the disputed original and the new
    // pending one.
    let submissions = matches_match_id_score_submissions_get(&owner_config, &created.id)
        .await
        .expect("list submissions");
    assert_eq!(submissions.len(), 2);
    let new_submission = submissions
        .iter()
        .find(|s| s.id == pending.submission_id)
        .expect("new submission present in history");
    assert!(matches!(
        new_submission.status,
        models::ScoreSubmissionStatus::Pending
    ));

    // The opponent can now confirm the resubmitted score, which finally
    // promotes it to the match's confirmed score.
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &pending.submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm resubmitted score");

    let final_match = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    let confirmed = final_match
        .confirmed_score
        .expect("resubmitted score is confirmed after opponent approves");
    assert_eq!(confirmed.winner_side_id.as_deref(), Some(side_a.as_str()));
}

/// Sets up a match with a confirmed score (owner on side "a", opponent on side
/// "b", 6-3 to the owner), for tests that edit an already-confirmed score.
/// Returns (owner_config, owner, opponent_config, opponent, match, side_a, side_b).
async fn match_with_a_confirmed_score() -> (
    Configuration,
    models::User,
    Configuration,
    models::User,
    models::Match,
    String,
    String,
) {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    let mut input = create_match_input(&opponent.profile.id);
    input.invites = vec![models::CreateMatchInviteInput {
        side_client_id: Some("b".to_string()),
        invited_user_ids: vec![opponent.profile.id.clone()],
        invited_externals: vec![],
    }];
    input.starts_at = iso_offset_hours(-2);
    input.creator_side_client_id = Some("a".to_string());
    input.score = Some(Box::new(simple_score("a", "b", 6, 3)));
    input.winner_side_id = Some("a".to_string());

    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    // `created.sides` is keyed (and returned) by side id, not creation order,
    // so `sides[0]`/`sides[1]` aren't reliably "a" then "b" — resolve each
    // real side id off the roster instead, by who's on it.
    let side_a = side_id_for_user(&created, &owner.profile.id);
    let side_b = side_id_for_user(&created, &opponent.profile.id);
    let first_submission_id = created
        .pending_score
        .as_ref()
        .expect("pending at create time")
        .submission_id
        .clone();

    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &first_submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm original score");

    let confirmed = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    assert!(
        confirmed.confirmed_score.is_some(),
        "setup: score confirmed"
    );

    (
        owner_config,
        owner,
        opponent_config,
        opponent,
        confirmed,
        side_a,
        side_b,
    )
}

#[tokio::test]
async fn editing_a_confirmed_score_stays_pending_until_the_other_side_confirms() {
    let (owner_config, _owner, opponent_config, _opponent, created, side_a, side_b) =
        match_with_a_confirmed_score().await;

    // The owner edits the match with a new, different score.
    let edited = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 4))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch edited score");

    // The old confirmed score is untouched; the new one shows up as pending.
    let still_confirmed = edited
        .confirmed_score
        .expect("confirmed score is unaffected by an unapproved edit");
    assert_eq!(
        simple_score_points(&still_confirmed.score),
        sorted_points(vec![(side_a.clone(), 6), (side_b.clone(), 3)]),
        "the previously-confirmed score must not change until the edit is approved"
    );
    let pending = edited
        .pending_score
        .expect("the edit is pending, not applied immediately");
    assert_eq!(
        simple_score_points(&pending.score),
        sorted_points(vec![(side_a.clone(), 6), (side_b.clone(), 4)])
    );

    // History now has the confirmed original plus the new pending edit.
    let submissions = matches_match_id_score_submissions_get(&owner_config, &created.id)
        .await
        .expect("list submissions");
    assert_eq!(submissions.len(), 2);

    // The opponent confirms the edit, promoting it to the confirmed score.
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &pending.submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm edited score");

    let final_match = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    let final_confirmed = final_match
        .confirmed_score
        .expect("edited score is confirmed");
    assert_eq!(
        simple_score_points(&final_confirmed.score),
        sorted_points(vec![(side_a.clone(), 6), (side_b.clone(), 4)])
    );
    assert!(final_match.pending_score.is_none());
}

#[tokio::test]
async fn disputing_an_edit_to_a_confirmed_score_leaves_the_original_confirmed_score_intact() {
    let (owner_config, _owner, opponent_config, _opponent, created, side_a, side_b) =
        match_with_a_confirmed_score().await;

    let edited = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 4))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch edited score");
    let pending = edited.pending_score.expect("the edit is pending");

    // The opponent disputes the edit.
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &pending.submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Dispute,
        },
    )
    .await
    .expect("dispute edited score");

    let after_dispute = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    assert!(
        after_dispute.pending_score.is_none(),
        "a disputed edit clears the pending score"
    );
    let confirmed = after_dispute
        .confirmed_score
        .expect("the original confirmed score still stands");
    assert_eq!(
        simple_score_points(&confirmed.score),
        sorted_points(vec![(side_a.clone(), 6), (side_b.clone(), 3)]),
        "a disputed edit must not change the previously-confirmed score"
    );

    // History has the original confirmed submission and the disputed edit.
    let submissions = matches_match_id_score_submissions_get(&owner_config, &created.id)
        .await
        .expect("list submissions");
    assert_eq!(submissions.len(), 2);
    let disputed_submission = submissions
        .iter()
        .find(|s| s.id == pending.submission_id)
        .expect("edited submission present in history");
    assert!(matches!(
        disputed_submission.status,
        models::ScoreSubmissionStatus::Disputed
    ));
}

#[tokio::test]
async fn submitting_a_score_notifies_the_other_side_to_confirm() {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    // Owner plays on side "a", opponent invited onto side "b" — no score at
    // creation. The opponent accepts the invite first, so the score PATCHed in
    // below is unambiguously "submitted after accept", which must always
    // notify (unlike a create-time score arriving alongside a still-pending
    // invite — see `inviting_with_a_score_does_not_duplicate_the_invite_notification`
    // for that deliberately-deduped case).
    let mut input = create_match_input(&opponent.profile.id);
    input.invites = vec![models::CreateMatchInviteInput {
        side_client_id: Some("b".to_string()),
        invited_user_ids: vec![opponent.profile.id.clone()],
        invited_externals: vec![],
    }];
    input.creator_side_client_id = Some("a".to_string());

    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");
    assert!(created.confirmed_score.is_none(), "no score yet");

    // The opponent accepts their invite before any score exists.
    let inbox = users_me_invitations_get(&opponent_config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| {
            matches!(&*i.context,
            models::InvitationContext::Match(ctx) if ctx.match_id == created.id)
        })
        .expect("match invitation in inbox");
    invitations_invitation_id_respond_post(
        &opponent_config,
        &detail.invitation.id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept invitation");

    // Now the owner submits a score (post-creation, via PATCH) — pending on
    // the opponent's (already-joined) side.
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();
    matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 3))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("patch score");

    // The opponent gets a ScoreSubmitted notification asking them to confirm, with
    // the owner as the submitter.
    let notif = eventually("score-submitted notification to be generated", || {
        let config = &opponent_config;
        let match_id = &created.id;
        let owner_id = &owner.profile.id;
        async move {
            let page = notifications_get(config, None, None).await.ok()?;
            page.items.into_iter().find(|n| match &*n.kind {
                models::NotificationKind::ScoreSubmitted(s) => {
                    &s.match_id == match_id && &s.submitted_by.id == owner_id
                }
                _ => false,
            })
        }
    })
    .await;
    let models::NotificationKind::ScoreSubmitted(submitted) = &*notif.kind else {
        unreachable!("filtered to ScoreSubmitted above");
    };
    assert!(
        submitted.needs_confirmation,
        "the opposing side must be asked to confirm"
    );

    // The opponent confirms the submission, which completes the score.
    matches_match_id_score_submissions_submission_id_respond_post(
        &opponent_config,
        &created.id,
        &submitted.submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm score");

    // The owner (the submitter) is notified that their score was confirmed, with
    // the opponent as the confirming actor.
    let confirmed = eventually("score-confirmed notification to be generated", || {
        let config = &owner_config;
        let match_id = &created.id;
        let opponent_id = &opponent.profile.id;
        async move {
            let page = notifications_get(config, None, None).await.ok()?;
            page.items.into_iter().find(|n| match &*n.kind {
                models::NotificationKind::ScoreConfirmed(c) => {
                    &c.match_id == match_id && &c.confirmed_by.id == opponent_id
                }
                _ => false,
            })
        }
    })
    .await;
    assert!(!confirmed.is_read, "a fresh notification is unread");
}

/// A match created with a score already attached (e.g. logging a completed
/// match and inviting the opponent in the same action) must not duplicate the
/// invite notification with a separate "score submitted, confirm it?" one —
/// the invitee's accept flow already surfaces the pending score in the same
/// step. Once they accept, a later submission notifies as normal (covered by
/// `submitting_a_score_notifies_the_other_side_to_confirm`).
#[tokio::test]
async fn inviting_with_a_score_does_not_duplicate_the_invite_notification() {
    let (owner_config, _owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    // Owner plays on side "a" and submits a create-time score; the opponent is
    // invited (left pending — never accepted in this test) onto side "b".
    let mut input = create_match_input(&opponent.profile.id);
    input.invites = vec![models::CreateMatchInviteInput {
        side_client_id: Some("b".to_string()),
        invited_user_ids: vec![opponent.profile.id.clone()],
        invited_externals: vec![],
    }];
    input.starts_at = iso_offset_hours(-2);
    input.creator_side_client_id = Some("a".to_string());
    input.score = Some(Box::new(simple_score("a", "b", 6, 3)));
    input.winner_side_id = Some("a".to_string());

    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");

    // Wait for the invite notification — proves the worker has processed this
    // match's events, so a still-absent score-submitted notification below
    // reflects the dedup rather than the async pipeline simply not having run.
    eventually("match invitation notification to be generated", || {
        let config = &opponent_config;
        let match_id = &created.id;
        async move {
            let page = notifications_get(config, None, None).await.ok()?;
            page.items.into_iter().find(|n| match &*n.kind {
                models::NotificationKind::MatchInvitation(m) => m.match_id == *match_id,
                _ => false,
            })
        }
    })
    .await;

    // A short extra settle (same idiom as `assert_match_absent_from_feed`),
    // then confirm no score-submitted notification arrived for the invitee —
    // their invite is still pending, so it was deliberately deduped.
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    let page = notifications_get(&opponent_config, None, None)
        .await
        .expect("list notifications");
    assert!(
        !page.items.iter().any(|n| matches!(&*n.kind,
            models::NotificationKind::ScoreSubmitted(s) if s.match_id == created.id)),
        "a still-pending invitee should not get a separate score-submitted notification"
    );
}

// ---------------------------------------------------------------------------
// Match discovery (GET /matches) — filter smoke test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_matches_accepts_filters() {
    let (config, _user) = new_user().await;
    // Discovery is served by the search index (async-populated), so we don't
    // assert content — just that the endpoint accepts the full filter set and
    // returns a well-formed page.
    let page = matches_get(
        &config,
        Some("test"),
        None,
        None,
        None,
        Some(models::MatchType::Tennis),
        Some("2026-01-01T00:00:00Z".to_string()),
        Some("2026-12-31T00:00:00Z".to_string()),
        None,
        Some(10),
    )
    .await
    .expect("list matches");
    let _ = page.items.len();
}

#[tokio::test]
async fn list_matches_rejects_inverted_date_range() {
    let (config, _user) = new_user().await;
    // `from` after `to` is a 400 with a specific message.
    let response = matches_get(
        &config,
        None,
        None,
        None,
        None,
        None,
        Some("2026-12-31T00:00:00Z".to_string()),
        Some("2026-01-01T00:00:00Z".to_string()),
        None,
        None,
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "`from` must be before `to`",
    );
}

/// The `team_id` filter on `GET /matches` finds a match via its team-linked
/// sides, in all three combinations: a single id, several ids with the
/// default `any` (union) mode, and several ids with `all` — head-to-head,
/// since a match's `team_ids` array must contain every listed id to match.
/// Exercises the indexed `team_ids` facet end to end (worker indexing +
/// Meilisearch filter), not just that the endpoint accepts the params.
#[tokio::test]
async fn list_matches_filters_by_team() {
    let (owner_config, owner) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    let team_a = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Discovery FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    let team_b = teams_post(
        &opponent_config,
        models::CreateTeamInput {
            name: "Discovery United".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    let unrelated_team = teams_post(
        &opponent_config,
        models::CreateTeamInput {
            name: "Unrelated FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    // Both teams play each other: side "a" is team_a, side "b" is team_b.
    let mut input = match_between(
        "Team Discovery Match",
        &[&owner.profile.id],
        &[&opponent.profile.id],
    );
    // Two distinct teams, so neither side needs (or is allowed) a custom name
    // — drop the placeholders `match_between` sets.
    input.sides[0].team_id = Some(team_a.id.clone());
    input.sides[0].name = None;
    input.sides[1].team_id = Some(team_b.id.clone());
    input.sides[1].name = None;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");

    // Indexing is async (stream -> SQS -> worker -> Meilisearch), so poll a
    // single-team lookup until the doc lands before asserting on the
    // multi-team combinators below (which would otherwise just look flaky).
    eventually("match to be searchable by team", || {
        let config = &owner_config;
        let team_id = team_a.id.clone();
        let match_id = created.id.clone();
        async move {
            let page = matches_get(
                config,
                None,
                None,
                Some(vec![team_id]),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .ok()?;
            page.items.into_iter().find(|m| m.id == match_id)
        }
    })
    .await;

    // Head-to-head: both teams with `all` finds it — the match's team_ids
    // array contains both.
    let h2h = matches_get(
        &owner_config,
        None,
        None,
        Some(vec![team_a.id.clone(), team_b.id.clone()]),
        Some(models::TeamMatchMode::All),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("list matches (head-to-head)");
    assert!(
        h2h.items.iter().any(|m| m.id == created.id),
        "head-to-head (all) should find the match"
    );

    // team_a plus an uninvolved third team with `all` finds nothing — no
    // match's team_ids array contains all three.
    let all_with_unrelated = matches_get(
        &owner_config,
        None,
        None,
        Some(vec![team_a.id.clone(), unrelated_team.id.clone()]),
        Some(models::TeamMatchMode::All),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("list matches (all, with an unrelated team)");
    assert!(
        !all_with_unrelated.items.iter().any(|m| m.id == created.id),
        "`all` with an uninvolved team should not match"
    );

    // Same two ids, default `any` mode: team_a alone is enough.
    let any_with_unrelated = matches_get(
        &owner_config,
        None,
        None,
        Some(vec![team_a.id.clone(), unrelated_team.id.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("list matches (any, with an unrelated team)");
    assert!(
        any_with_unrelated.items.iter().any(|m| m.id == created.id),
        "`any` should find it via team_a alone"
    );

    // The unrelated team alone finds nothing.
    let unrelated_only = matches_get(
        &owner_config,
        None,
        None,
        Some(vec![unrelated_team.id.clone()]),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("list matches (unrelated team only)");
    assert!(
        !unrelated_only.items.iter().any(|m| m.id == created.id),
        "unrelated team should not match"
    );
}

// ---------------------------------------------------------------------------
// Validation errors (assert the specific rejection message, not just status)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creating_a_match_with_one_side_is_rejected() {
    let (config, _user) = new_user().await;
    let mut input = create_match_input("irrelevant");
    input.sides.truncate(1); // only one side
    input.invites.clear();
    let response = matches_post(&config, input).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "at least two sides",
    );
}

/// A side with no players is fine (recording a result against an opposition
/// whose roster you don't know) as long as it's identifiable some other way —
/// here, an explicit name. `create_match_input`'s side "b" already carries no
/// invites, so this only has to strip its name.
#[tokio::test]
async fn creating_a_match_with_an_unnamed_empty_side_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let mut input = create_match_input(&invitee.profile.id);
    input.sides[1].name = None; // side "b": no players, no team, now no name either
    let response = matches_post(&config, input).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "needs a name or a team",
    );
}

/// The counterpart to the rejection above: an empty side with an explicit
/// name is accepted, and that name is what the side resolves to (no players
/// to fall back on, no team, so the custom name is the only source left).
#[tokio::test]
async fn creating_a_match_with_a_named_empty_side_succeeds() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let input = create_match_input(&invitee.profile.id); // side "b" is empty, named "Side B"
    let created = matches_post(&config, input).await.expect("create match");
    // Response side order isn't creation order (sides sort by their generated
    // id), so find the empty one by roster rather than assuming an index.
    let side_b = created
        .sides
        .iter()
        .find(|s| {
            !created
                .players
                .iter()
                .any(|p| p.side_id.as_deref() == Some(s.id.as_str()))
        })
        .expect("one side has no players");
    assert_eq!(side_b.name.as_deref(), Some("Side B"));
}

/// Same rule, projected forward at edit time: a request that would both clear
/// a side's name *and* move its only player off it (leaving it with no
/// players and no name) is rejected, mirroring `create_match`'s check.
#[tokio::test]
async fn emptying_an_unnamed_side_via_side_assignments_is_rejected() {
    let (owner_config, owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let created = matches_post(
        &owner_config,
        match_between("Test Match", &[&owner.profile.id], &[&invitee.profile.id]),
    )
    .await
    .expect("create match");

    let side_a = side_id_for_user(&created, &owner.profile.id);
    let side_b = side_id_for_user(&created, &invitee.profile.id);
    let player_b_id = created
        .players
        .iter()
        .find_map(|p| match &*p.member {
            models::Member::User(u) if u.user_id == invitee.profile.id => Some(u.id.clone()),
            _ => None,
        })
        .expect("invitee player");

    let response = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            side_names: Some(vec![models::UpdateMatchSideNameInput {
                side_id: side_b.clone(),
                name: None,
            }]),
            side_assignments: Some(vec![models::SetPlayerSideInput {
                player_id: player_b_id,
                side_id: Some(side_a),
            }]),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "would have no players",
    );
}

#[tokio::test]
async fn scoring_an_unknown_side_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let real_side = match_.sides[0].id.clone();

    // One real side, one bogus side id.
    let response = matches_match_id_patch(
        &config,
        &match_.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&real_side, "not-a-real-side", 6, 3))),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "not part of this match",
    );
}

#[tokio::test]
async fn creating_a_scheduled_match_in_the_past_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    // No score => Scheduled, but the time is in the past.
    let mut input = create_match_input(&invitee.profile.id);
    input.starts_at = iso_offset_hours(-24);

    let response = matches_post(&config, input).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "scheduled match's time must be in the future",
    );
}

#[tokio::test]
async fn creating_a_completed_match_in_the_future_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    // A score => Completed, so a future time is contradictory. The creator plays
    // on side "a" (required to submit a create-time score).
    let mut input = create_match_input(&invitee.profile.id);
    input.starts_at = iso_offset_hours(24);
    input.creator_side_client_id = Some("a".to_string());
    input.score = Some(Box::new(simple_score("a", "b", 6, 3)));
    input.winner_side_id = Some("a".to_string());

    let response = matches_post(&config, input).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "completed match's time must be in the past",
    );
}

#[tokio::test]
async fn creating_a_completed_match_in_the_past_succeeds() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    // A score with a past time is a valid already-played match.
    let mut input = create_match_input(&invitee.profile.id);
    input.starts_at = iso_offset_hours(-2);
    input.creator_side_client_id = Some("a".to_string());
    input.score = Some(Box::new(simple_score("a", "b", 6, 3)));
    input.winner_side_id = Some("a".to_string());

    let created = matches_post(&config, input).await.expect("create match");
    assert!(matches!(created.status, models::MatchStatus::Completed));
}

#[tokio::test]
async fn empty_comment_text_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let response = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "   ".to_string(), // whitespace only
            parent_id: None,
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "must not be empty",
    );
}

#[tokio::test]
async fn replying_to_a_reply_is_rejected() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let parent = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "parent".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("parent");
    let reply = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "reply".to_string(),
            parent_id: Some(parent.id.clone()),
        },
    )
    .await
    .expect("reply");

    // Replying to the reply (a second-level reply) is rejected.
    let response = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "nested".to_string(),
            parent_id: Some(reply.id.clone()),
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "cannot reply to a reply",
    );
}

// ---------------------------------------------------------------------------
// Comment replies & like listing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replies_are_listed_under_their_parent() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let parent = matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "parent".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("parent comment");

    matches_match_id_comments_post(
        &config,
        &match_.id,
        models::CreateCommentInput {
            text: "reply".to_string(),
            parent_id: Some(parent.id.clone()),
        },
    )
    .await
    .expect("reply");

    let replies = matches_match_id_comments_comment_id_replies_get(
        &config, &match_.id, &parent.id, None, None,
    )
    .await
    .expect("list replies");
    assert!(
        replies
            .items
            .iter()
            .any(|c| c.text.as_deref() == Some("reply"))
    );
}

#[tokio::test]
async fn a_matchs_likers_are_listed() {
    let (config, liker) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    matches_match_id_likes_post(&config, &match_.id)
        .await
        .expect("like");

    let likers = matches_match_id_likes_get(&config, &match_.id, None, None)
        .await
        .expect("list likers");
    assert!(likers.items.iter().any(|u| u.id == liker.profile.id));
}

// ---------------------------------------------------------------------------
// Follow listing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn following_list_includes_followed_user() {
    let (follower_config, follower) = new_user().await;
    let (_followee_config, followee) = new_user().await;

    users_user_id_follow_post(&follower_config, &followee.profile.id)
        .await
        .expect("follow");

    let following = users_user_id_following_get(&follower_config, &follower.profile.id, None, None)
        .await
        .expect("following list");
    assert!(following.items.iter().any(|u| u.id == followee.profile.id));
}

#[tokio::test]
async fn team_followers_list_includes_the_follower() {
    let (owner_config, _owner) = new_user().await;
    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Followed Team".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let (follower_config, follower) = new_user().await;
    teams_team_id_follow_post(&follower_config, &team.id)
        .await
        .expect("follow team");

    let followers = teams_team_id_followers_get(&owner_config, &team.id, None, None)
        .await
        .expect("team followers");
    assert!(followers.items.iter().any(|u| u.id == follower.profile.id));
}

// ---------------------------------------------------------------------------
// Team update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn patch_team_updates_name() {
    let (config, _user) = new_user().await;
    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Before".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let updated = teams_team_id_patch(
        &config,
        &team.id,
        models::UpdateTeamInput {
            name: Some("After".to_string()),
            logo_asset_id: None,
        },
    )
    .await
    .expect("patch team");
    assert_eq!(updated.name, "After");
}

// ---------------------------------------------------------------------------
// Team roles & permissions
// ---------------------------------------------------------------------------

/// The full permission matrix in one pass: a plain member (and a total
/// stranger) is rejected from every management action; an admin — promoted
/// via `PATCH /teams/:id/members/:id`'s role endpoint — can do everything a
/// member can't (rename, add members, invite, remove a member, promote
/// someone else), except delete the team, which stays owner-only.
#[tokio::test]
async fn team_admin_can_manage_but_member_cannot() {
    let (owner_config, _owner) = new_user().await;
    let (admin_config, admin) = new_user().await;
    let (member_config, member) = new_user().await;
    let (stranger_config, _stranger) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Roles FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    teams_team_id_members_post(
        &owner_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![admin.profile.id.clone(), member.profile.id.clone()],
        },
    )
    .await
    .expect("add members");
    let members = all_team_members(&owner_config, &team.id).await;
    let admin_membership_id =
        membership_id_for(&members, &admin.profile.id).expect("admin membership");
    let member_membership_id =
        membership_id_for(&members, &member.profile.id).expect("member membership");

    // The owner promotes the future admin — still just a member so far, so
    // this also covers "owner can change roles".
    teams_team_id_members_member_id_patch(
        &owner_config,
        &team.id,
        &admin_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("promote to admin");

    let rename_input = || models::UpdateTeamInput {
        name: Some("Hijacked".to_string()),
        logo_asset_id: None,
    };
    let no_invitees = || models::AddInvitationsInput {
        invited_user_ids: vec![],
        invited_external_names: vec![],
        side_id: None,
        role: None,
    };

    // A plain member can't manage the team...
    assert_forbidden(teams_team_id_patch(&member_config, &team.id, rename_input()).await);
    assert_forbidden(
        teams_team_id_members_post(
            &member_config,
            &team.id,
            models::AddTeamMembersInput { user_ids: vec![] },
        )
        .await,
    );
    assert_forbidden(teams_team_id_invitations_post(&member_config, &team.id, no_invitees()).await);
    assert_forbidden(
        teams_team_id_members_member_id_delete(&member_config, &team.id, &member_membership_id)
            .await,
    );
    assert_forbidden(
        teams_team_id_members_member_id_patch(
            &member_config,
            &team.id,
            &member_membership_id,
            models::UpdateTeamMemberRoleInput {
                role: models::AssignableTeamRole::Admin,
            },
        )
        .await,
    );
    // ...and neither can a total stranger, not even on the team at all.
    assert_forbidden(teams_team_id_patch(&stranger_config, &team.id, rename_input()).await);

    // The admin, though, can do everything the owner can except delete.
    let renamed = teams_team_id_patch(
        &admin_config,
        &team.id,
        models::UpdateTeamInput {
            name: Some("Renamed by admin".to_string()),
            logo_asset_id: None,
        },
    )
    .await
    .expect("admin can rename");
    assert_eq!(renamed.name, "Renamed by admin");

    let (_extra_config, extra) = new_user().await;
    teams_team_id_members_post(
        &admin_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![extra.profile.id.clone()],
        },
    )
    .await
    .expect("admin can add members");

    let invited = teams_team_id_invitations_post(
        &admin_config,
        &team.id,
        models::AddInvitationsInput {
            invited_user_ids: vec![],
            invited_external_names: vec!["Guest".to_string()],
            side_id: None,
            role: None,
        },
    )
    .await
    .expect("admin can invite");
    assert_eq!(invited.len(), 1);

    let members_now = all_team_members(&admin_config, &team.id).await;
    let extra_membership_id =
        membership_id_for(&members_now, &extra.profile.id).expect("extra's membership");
    teams_team_id_members_member_id_patch(
        &admin_config,
        &team.id,
        &extra_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("admin can promote someone else");

    teams_team_id_members_member_id_delete(&admin_config, &team.id, &member_membership_id)
        .await
        .expect("admin can remove a member");

    // But only the owner may delete the team itself.
    assert_forbidden(teams_team_id_delete(&admin_config, &team.id).await);
}

/// The team's owner can't be removed, nor have their role changed, by
/// anyone — not an admin, and not even themselves. Deleting the whole team
/// is the only way the owner relationship ever ends (see
/// `only_owner_can_delete_team`); there's no ownership-transfer flow.
#[tokio::test]
async fn team_owner_cannot_be_removed_or_have_their_role_changed() {
    let (owner_config, owner) = new_user().await;
    let (admin_config, admin) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Protected FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    teams_team_id_members_post(
        &owner_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![admin.profile.id.clone()],
        },
    )
    .await
    .expect("add admin candidate");
    let members = all_team_members(&owner_config, &team.id).await;
    let admin_membership_id = membership_id_for(&members, &admin.profile.id).expect("admin id");
    let owner_membership_id = membership_id_for(&members, &owner.profile.id).expect("owner id");
    teams_team_id_members_member_id_patch(
        &owner_config,
        &team.id,
        &admin_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("promote");

    // Neither the owner themselves nor an admin can remove the owner.
    assert_forbidden(
        teams_team_id_members_member_id_delete(&owner_config, &team.id, &owner_membership_id).await,
    );
    assert_forbidden(
        teams_team_id_members_member_id_delete(&admin_config, &team.id, &owner_membership_id).await,
    );

    // Nor change the owner's role, from either side.
    let demote = || models::UpdateTeamMemberRoleInput {
        role: models::AssignableTeamRole::Member,
    };
    assert_forbidden(
        teams_team_id_members_member_id_patch(
            &owner_config,
            &team.id,
            &owner_membership_id,
            demote(),
        )
        .await,
    );
    assert_forbidden(
        teams_team_id_members_member_id_patch(
            &admin_config,
            &team.id,
            &owner_membership_id,
            demote(),
        )
        .await,
    );

    // The owner is still there, still the owner.
    let members_after = all_team_members(&owner_config, &team.id).await;
    let owner_role = members_after
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.id == owner_membership_id => Some(m.role),
            _ => None,
        })
        .expect("owner still on the roster");
    assert_eq!(owner_role, models::TeamRole::Owner);
}

/// Delete-team is owner-only, and actually removes the team — a 404 on a
/// subsequent `GET` (both the team itself and its member list), not just a
/// 204 that leaves stale data behind.
#[tokio::test]
async fn only_owner_can_delete_team() {
    let (owner_config, _owner) = new_user().await;
    let (admin_config, admin) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Deletable FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    teams_team_id_members_post(
        &owner_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![admin.profile.id.clone()],
        },
    )
    .await
    .expect("add admin candidate");
    let members = all_team_members(&owner_config, &team.id).await;
    let admin_membership_id = membership_id_for(&members, &admin.profile.id).expect("admin id");
    teams_team_id_members_member_id_patch(
        &owner_config,
        &team.id,
        &admin_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("promote");

    // An admin — who can do everything else — still can't delete the team.
    assert_forbidden(teams_team_id_delete(&admin_config, &team.id).await);

    // The owner can, and it's actually gone afterward.
    teams_team_id_delete(&owner_config, &team.id)
        .await
        .expect("owner deletes team");
    assert_not_found(teams_team_id_get(&owner_config, &team.id).await);
    assert_not_found(teams_team_id_members_get(&owner_config, &team.id, None, None).await);
}

/// A plain member and an admin can both leave a team on their own; the
/// owner can't — until they transfer ownership away (to an accepted member),
/// at which point they're just an admin and leaving works the same way.
#[tokio::test]
async fn member_and_admin_can_leave_but_owner_must_transfer_first() {
    let (owner_config, owner) = new_user().await;
    let (member_config, member) = new_user().await;
    let (admin_config, admin) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Leavable FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");
    teams_team_id_members_post(
        &owner_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member.profile.id.clone(), admin.profile.id.clone()],
        },
    )
    .await
    .expect("add members");
    let members = all_team_members(&owner_config, &team.id).await;
    let admin_membership_id = membership_id_for(&members, &admin.profile.id).expect("admin id");
    let owner_membership_id = membership_id_for(&members, &owner.profile.id).expect("owner id");
    teams_team_id_members_member_id_patch(
        &owner_config,
        &team.id,
        &admin_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("promote");

    // The plain member leaves on their own — no owner/admin action needed.
    teams_team_id_leave_post(&member_config, &team.id)
        .await
        .expect("member leaves");
    let after_member_leaves = all_team_members(&owner_config, &team.id).await;
    assert!(!member_ids(&after_member_leaves).contains(&member.profile.id));

    // The owner can't — the server rejects it with a specific reason, and
    // they're still on the roster afterward.
    assert_status_with_content(
        teams_team_id_leave_post(&owner_config, &team.id).await,
        reqwest::StatusCode::BAD_REQUEST,
        "transfer ownership",
    );
    let still_owner = all_team_members(&owner_config, &team.id).await;
    assert!(member_ids(&still_owner).contains(&owner.profile.id));

    // Transfer ownership to the admin — roles swap...
    teams_team_id_transfer_ownership_post(
        &owner_config,
        &team.id,
        models::TransferTeamOwnershipInput {
            member_id: admin_membership_id.clone(),
        },
    )
    .await
    .expect("transfer ownership");
    let after_transfer = all_team_members(&owner_config, &team.id).await;
    let new_owner_role = after_transfer
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.id == admin_membership_id => Some(m.role),
            _ => None,
        })
        .expect("former admin still on roster");
    assert_eq!(new_owner_role, models::TeamRole::Owner);
    let former_owner_role = after_transfer
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.id == owner_membership_id => Some(m.role),
            _ => None,
        })
        .expect("former owner still on roster");
    assert_eq!(former_owner_role, models::TeamRole::Admin);

    // ...and now the former owner (just an admin) can leave like anyone else.
    teams_team_id_leave_post(&owner_config, &team.id)
        .await
        .expect("former owner leaves");
    let final_members = all_team_members(&admin_config, &team.id).await;
    assert!(!member_ids(&final_members).contains(&owner.profile.id));
    assert!(member_ids(&final_members).contains(&admin.profile.id));
}

/// Only the owner may transfer ownership, and only to someone who's actually
/// an accepted member — not themselves (a no-op that would just be
/// confusing to allow), and not a still-pending invitee.
#[tokio::test]
async fn transfer_ownership_rejects_non_owner_self_and_pending_targets() {
    let (owner_config, owner) = new_user().await;
    let (admin_config, admin) = new_user().await;
    let (invitee_config, invitee) = new_user().await;
    let _ = invitee_config; // only need their id, never signs in for this test

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Transferable FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team with a pending invite");
    teams_team_id_members_post(
        &owner_config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![admin.profile.id.clone()],
        },
    )
    .await
    .expect("add admin candidate");
    let members = all_team_members(&owner_config, &team.id).await;
    let admin_membership_id = membership_id_for(&members, &admin.profile.id).expect("admin id");
    let owner_membership_id = membership_id_for(&members, &owner.profile.id).expect("owner id");
    let invitee_membership_id =
        membership_id_for(&members, &invitee.profile.id).expect("invitee still pending");
    teams_team_id_members_member_id_patch(
        &owner_config,
        &team.id,
        &admin_membership_id,
        models::UpdateTeamMemberRoleInput {
            role: models::AssignableTeamRole::Admin,
        },
    )
    .await
    .expect("promote");

    // A non-owner (even an admin, who can do almost everything else) can't
    // transfer ownership.
    assert_forbidden(
        teams_team_id_transfer_ownership_post(
            &admin_config,
            &team.id,
            models::TransferTeamOwnershipInput {
                member_id: admin_membership_id.clone(),
            },
        )
        .await,
    );

    // The owner can't "transfer" to themselves.
    assert_bad_request(
        teams_team_id_transfer_ownership_post(
            &owner_config,
            &team.id,
            models::TransferTeamOwnershipInput {
                member_id: owner_membership_id.clone(),
            },
        )
        .await,
    );

    // Nor to the still-pending invitee — they haven't actually joined yet.
    assert_bad_request(
        teams_team_id_transfer_ownership_post(
            &owner_config,
            &team.id,
            models::TransferTeamOwnershipInput {
                member_id: invitee_membership_id,
            },
        )
        .await,
    );

    // Ownership never moved through any of that.
    let members_after = all_team_members(&owner_config, &team.id).await;
    let owner_role = members_after
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.id == owner_membership_id => Some(m.role),
            _ => None,
        })
        .expect("owner still on roster");
    assert_eq!(owner_role, models::TeamRole::Owner);
}

/// Leaving a team you're not a member of (or that doesn't exist) is a 404,
/// not a silent success.
#[tokio::test]
async fn leaving_a_team_youre_not_a_member_of_returns_not_found() {
    let (owner_config, _owner) = new_user().await;
    let (stranger_config, _stranger) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Not Yours FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    assert_not_found(teams_team_id_leave_post(&stranger_config, &team.id).await);
    assert_not_found(teams_team_id_leave_post(&stranger_config, "no-such-team").await);
}

// ---------------------------------------------------------------------------
// Invitations: fetch, decline, revoke
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invitation_can_be_fetched_and_declined() {
    let (owner_config, _owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let inbox = users_me_invitations_get(&invitee_config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| matches!(&*i.context, models::InvitationContext::Match(ctx) if ctx.match_id == match_.id))
        .expect("invitation in inbox");
    let invitation_id = detail.invitation.id.clone();

    // Fetchable by id.
    let fetched = invitations_invitation_id_get(&invitee_config, &invitation_id)
        .await
        .expect("get invitation");
    assert_eq!(fetched.invitation.id, invitation_id);

    // Decline it.
    let responded = invitations_invitation_id_respond_post(
        &invitee_config,
        &invitation_id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Declined,
            side_id: None,
        },
    )
    .await
    .expect("decline");
    assert!(matches!(
        responded.status,
        models::InvitationStatus::Declined
    ));
}

#[tokio::test]
async fn inviter_can_revoke_an_invitation() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    // Add a fresh invitation via the match-invitations endpoint so we own it.
    let created = matches_match_id_invitations_post(
        &owner_config,
        &match_.id,
        models::AddInvitationsInput {
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec![],
            side_id: None,
            role: None,
        },
    )
    .await
    .expect("add invitation");
    let inv_id = created.first().expect("one invitation").id.clone();

    invitations_invitation_id_delete(&owner_config, &inv_id)
        .await
        .expect("revoke");

    // Now gone.
    let response = invitations_invitation_id_get(&owner_config, &inv_id).await;
    assert_not_found(response);
}

/// Revoking an already-revoked invitation is not idempotent, unlike
/// follow/like: an invitation has no idempotent-create counterpart to stay
/// symmetric with, so a second revoke is a genuine 404, not a silent no-op.
#[tokio::test]
async fn revoking_an_already_revoked_invitation_returns_not_found() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&owner_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let created = matches_match_id_invitations_post(
        &owner_config,
        &match_.id,
        models::AddInvitationsInput {
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec![],
            side_id: None,
            role: None,
        },
    )
    .await
    .expect("add invitation");
    let inv_id = created.first().expect("one invitation").id.clone();

    invitations_invitation_id_delete(&owner_config, &inv_id)
        .await
        .expect("first revoke");

    let response = invitations_invitation_id_delete(&owner_config, &inv_id).await;
    assert_not_found(response);
}

// ---------------------------------------------------------------------------
// Notifications: single mark-read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn marking_a_single_notification_read_is_accepted() {
    let (config, _user) = new_user().await;
    // No notification necessarily exists (generation is async), but the endpoint
    // is idempotent and must accept an arbitrary id without erroring.
    notifications_notification_id_read_post(&config, "any-id")
        .await
        .expect("mark single read");
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creating_a_user_with_a_duplicate_email_is_rejected() {
    let (_config, user) = new_user().await;

    // A second, DISTINCT subject whose token carries the SAME email claim. Email
    // now comes from the verified token (not the body), so a duplicate means two
    // identities presenting the same authenticated email.
    let other_subject = Uuid::new_v4().to_string();
    let other_config = config_with_email(&other_subject, &user.email);
    let response = users_post(
        &other_config,
        models::CreateUserInput {
            name: "Dupe".to_string(),
        },
    )
    .await;
    // Email uniqueness is guarded. The API surfaces DAO conflicts as a 400
    // ValidationError (its consistent convention — there are no 409s), rather
    // than 409 Conflict.
    assert_bad_request(response);
}

#[tokio::test]
async fn non_author_cannot_edit_a_comment() {
    let (author_config, _author) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;
    let match_ = matches_post(&author_config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");

    let comment = matches_match_id_comments_post(
        &author_config,
        &match_.id,
        models::CreateCommentInput {
            text: "mine".to_string(),
            parent_id: None,
        },
    )
    .await
    .expect("create comment");

    // A different user tries to edit it -> 403 Forbidden.
    let (other_config, _other) = new_user().await;
    let response = matches_match_id_comments_comment_id_patch(
        &other_config,
        &match_.id,
        &comment.id,
        models::UpdateCommentInput {
            text: "hijacked".to_string(),
        },
    )
    .await;
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert!(matches!(
        err,
        openapi::apis::Error::ResponseError(openapi::apis::ResponseContent {
            status: reqwest::StatusCode::FORBIDDEN,
            ..
        })
    ));
}

// ---------------------------------------------------------------------------
// Not-found path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_missing_match_returns_not_found() {
    let (config, _user) = new_user().await;

    let response = matches_match_id_get(&config, "does-not-exist").await;
    assert_not_found(response);
}

// ---------------------------------------------------------------------------
// Response-status assertion helpers
// ---------------------------------------------------------------------------

/// Assert a client call failed with a specific HTTP status.
fn assert_status<T, E: std::fmt::Debug>(
    response: Result<T, openapi::apis::Error<E>>,
    expected: reqwest::StatusCode,
) {
    match response {
        Ok(_) => panic!("expected {expected}, got success"),
        Err(openapi::apis::Error::ResponseError(rc)) => {
            assert_eq!(rc.status, expected, "unexpected status");
        }
        Err(e) => panic!("expected {expected} response error, got: {e:?}"),
    }
}

/// Assert a client call failed with a status AND that the response body contains
/// `expected_content` (the human-readable validation message). Verifies we get
/// the *specific* rejection we expect, not just any error of that status.
fn assert_status_with_content<T, E: std::fmt::Debug>(
    response: Result<T, openapi::apis::Error<E>>,
    expected: reqwest::StatusCode,
    expected_content: &str,
) {
    match response {
        Ok(_) => panic!("expected {expected} ({expected_content}), got success"),
        Err(openapi::apis::Error::ResponseError(rc)) => {
            assert_eq!(rc.status, expected, "unexpected status");
            assert!(
                rc.content.contains(expected_content),
                "expected body to contain {expected_content:?}, got: {:?}",
                rc.content
            );
        }
        Err(e) => panic!("expected {expected} response error, got: {e:?}"),
    }
}

fn assert_not_found<T, E: std::fmt::Debug>(response: Result<T, openapi::apis::Error<E>>) {
    assert_status(response, reqwest::StatusCode::NOT_FOUND);
}

fn assert_forbidden<T, E: std::fmt::Debug>(response: Result<T, openapi::apis::Error<E>>) {
    assert_status(response, reqwest::StatusCode::FORBIDDEN);
}

fn assert_bad_request<T, E: std::fmt::Debug>(response: Result<T, openapi::apis::Error<E>>) {
    assert_status(response, reqwest::StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Async eventual-consistency helper
// ---------------------------------------------------------------------------

/// Poll `f` until it returns `Some(v)` or the timeout elapses, then return `v`.
/// For async pipeline effects (search indexing, notification generation) that
/// are eventually consistent: the write commits synchronously, but the stream →
/// SQS → worker → Meilisearch/notification path lands afterwards.
async fn eventually<T, F, Fut>(what: &str, f: F) -> T
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    // ~20s total: async fan-out via SQS long-poll + worker processing can take a
    // few seconds; generous so CI isn't flaky, bounded so a broken pipeline fails
    // rather than hangs.
    const ATTEMPTS: u32 = 20;
    for attempt in 1..=ATTEMPTS {
        if let Some(v) = f().await {
            return v;
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    }
    panic!("timed out after {ATTEMPTS}s waiting for: {what}");
}

/// Whether `match_id` is currently anywhere in the viewer's paged feed.
async fn feed_contains(config: &Configuration, match_id: &str) -> bool {
    let mut cursor: Option<String> = None;
    loop {
        let page = feed_get(config, cursor.as_deref(), Some(50), None, None)
            .await
            .expect("list feed");
        if page.items.iter().any(|item| item.id == match_id) {
            return true;
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return false,
        }
    }
}

/// Assert `match_id` eventually fans out into the viewer's feed (polls through
/// the async stream -> SQS -> worker -> feed pipeline).
async fn assert_match_reaches_feed(config: &Configuration, match_id: &str, whose: &str) {
    eventually(&format!("match to reach {whose}"), || async {
        feed_contains(config, match_id).await.then_some(())
    })
    .await;
}

/// Assert `match_id` is NOT in the viewer's feed. Meant to be called only AFTER
/// the fan-out has demonstrably completed for someone who *should* receive it
/// (assert that first) — so a still-absent match reflects a real audience
/// exclusion rather than the async pipeline simply not having run yet. A short
/// extra settle avoids a same-moment race where the negative is checked before
/// a (hypothetical) erroneous write lands.
async fn assert_match_absent_from_feed(config: &Configuration, match_id: &str, whose: &str) {
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    assert!(
        !feed_contains(config, match_id).await,
        "match {match_id} should NOT be in {whose}"
    );
}

/// The caller's own `matches_played` for a sport (0 if they have no stat row for
/// it yet). Reads `/users/me`, whose profile carries one field per sport.
async fn my_matches_played(config: &Configuration, sport: models::MatchType) -> i32 {
    let me = users_me_get(config).await.expect("get me");
    let stats = &me.profile.stats;
    let matches_played = match sport {
        models::MatchType::Cricket => stats.cricket.as_ref().map(|s| s.matches_played),
        models::MatchType::Football => stats.football.as_ref().map(|s| s.matches_played),
        models::MatchType::Tennis => stats.tennis.as_ref().map(|s| s.matches_played),
        models::MatchType::Badminton => stats.badminton.as_ref().map(|s| s.matches_played),
        models::MatchType::Squash => stats.squash.as_ref().map(|s| s.matches_played),
        models::MatchType::TableTennis => stats.table_tennis.as_ref().map(|s| s.matches_played),
        models::MatchType::Netball => stats.netball.as_ref().map(|s| s.matches_played),
        models::MatchType::Other => stats.other.as_ref().map(|s| s.matches_played),
    };
    matches_played.unwrap_or(0)
}

/// Poll the caller's own stats until they've played `expected` matches of a
/// sport. Stats are reconciled asynchronously by the accept saga (a roster link
/// doesn't touch match `#META`, so the stream-driven stats handler doesn't fire —
/// the saga reconciles the newly-linked player explicitly), so this is eventual.
async fn assert_matches_played_reaches(
    config: &Configuration,
    sport: models::MatchType,
    expected: i32,
    whose: &str,
) {
    eventually(
        &format!("{whose} to have played {expected} match(es)"),
        || async { (my_matches_played(config, sport).await == expected).then_some(()) },
    )
    .await;
}

/// The bearer token minted for an external (unaccounted) invitee, pulled off a
/// created match's players. Panics if no external player carries a token invite —
/// the by-token accept flow depends on this credential existing.
fn external_invite_token(match_: &models::Match) -> String {
    match_
        .players
        .iter()
        .find_map(|p| match &*p.member {
            models::Member::External(ext) => {
                ext.invitation.as_ref().and_then(|inv| match &*inv.kind {
                    models::InvitationKind::Token(t) => Some(t.invite_token.clone()),
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("an external player with a token invitation")
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// The real side id a linked Agon user is assigned to on a match. Sides are
/// returned keyed (and sorted) by side id, not creation order, so
/// `match_.sides[0]`/`[1]` don't reliably correspond to a particular
/// `side_client_id` used at create time — resolve by roster membership
/// instead.
fn side_id_for_user(match_: &models::Match, user_id: &str) -> String {
    match_
        .players
        .iter()
        .find(|p| matches!(&*p.member, models::Member::User(u) if u.user_id == user_id))
        .and_then(|p| p.side_id.clone())
        .expect("player with a side assigned")
}

/// The stable, match-scoped player id a linked Agon user is assigned —
/// what score events (batting/bowling entries, goal scorers/assists) key by,
/// as opposed to their (stable, cross-match) Agon user id.
fn player_id_for_user(match_: &models::Match, user_id: &str) -> String {
    match_
        .players
        .iter()
        .find_map(|p| match &*p.member {
            models::Member::User(u) if u.user_id == user_id => Some(u.id.clone()),
            _ => None,
        })
        .expect("player with that user id")
}

/// Accept `config`'s own pending invitation to `match_id`, found in their
/// inbox. A genuinely invited (non-creator) participant's contribution only
/// counts toward their stats once accepted — the creator/self-added case has
/// no invitation to accept in the first place.
async fn accept_match_invitation(config: &Configuration, match_id: &str) {
    let inbox = users_me_invitations_get(config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| {
            matches!(&*i.context, models::InvitationContext::Match(ctx) if ctx.match_id == match_id)
        })
        .expect("match invitation in inbox");
    invitations_invitation_id_respond_post(
        config,
        &detail.invitation.id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept invitation");
}

/// Create, score, and confirm a completed cricket match where `bowler_id`
/// (on side "a", against a fresh opponent on side "b") takes `wickets` for
/// `runs_conceded` off `overs` — the minimal shape needed to drive
/// `best_bowling_figures_only_ratchet_up_across_matches` across several
/// matches without repeating the full scorecard setup each time.
async fn play_cricket_bowling_match(
    bowler_config: &Configuration,
    bowler_id: &str,
    wickets: i32,
    runs_conceded: i32,
    overs: models::Overs,
) {
    let (batter_config, batter) = new_user().await;

    let mut input = match_between("Cricket Match", &[bowler_id], &[&batter.profile.id]);
    input.match_type = models::MatchType::Cricket;
    let created = matches_post(bowler_config, input)
        .await
        .expect("create match");
    accept_match_invitation(&batter_config, &created.id).await;

    let side_a = side_id_for_user(&created, bowler_id);
    let side_b = side_id_for_user(&created, &batter.profile.id);
    let bowler_pid = player_id_for_user(&created, bowler_id);

    let mut innings = models::CricketScoreInnings::new(
        side_b,
        side_a.clone(),
        runs_conceded + 20,
        wickets,
        models::Overs::new(20, 0),
        false,
    );
    innings.bowling = Some(vec![models::CricketBowlingEntry::new(
        bowler_pid,
        overs,
        0,
        runs_conceded,
        wickets,
        0,
        0,
    )]);

    let score = models::Score::Cricket(Box::new(models::ScoreCricketScore::new(
        vec![innings],
        std::collections::HashMap::new(),
        Default::default(),
    )));

    let updated = matches_match_id_patch(
        bowler_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(score)),
            winner_side_id: Some(side_a),
            ..Default::default()
        },
    )
    .await
    .expect("patch cricket score");
    let submission_id = updated
        .pending_score
        .expect("pending score after patch")
        .submission_id;
    matches_match_id_score_submissions_submission_id_respond_post(
        &batter_config,
        &created.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm cricket score");
}

/// Every member of a team, across all pages of `GET /teams/:id/members` — a
/// squad in these tests never comes close to one page, but walking pages
/// rather than assuming one keeps this honest as the paginated endpoint it is.
async fn all_team_members(config: &Configuration, team_id: &str) -> Vec<models::TeamMember> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = teams_team_id_members_get(config, team_id, cursor.as_deref(), None)
            .await
            .expect("list team members");
        items.extend(page.items);
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return items,
        }
    }
}

/// The (linked user id, stable membership id) of each team member that is a
/// linked Agon user. External (unlinked) members have no user id, so they're
/// omitted.
fn member_user_and_membership_ids(members: &[models::TeamMember]) -> Vec<(String, String)> {
    members
        .iter()
        .filter_map(|m| match &*m.member {
            models::Member::User(u) => Some((u.user_id.clone(), u.id.clone())),
            models::Member::External(_) => None,
        })
        .collect()
}

/// The linked user ids of a team's members.
fn member_ids(members: &[models::TeamMember]) -> Vec<String> {
    member_user_and_membership_ids(members)
        .into_iter()
        .map(|(user_id, _)| user_id)
        .collect()
}

/// The stable membership id for a member with the given linked user id.
fn membership_id_for(members: &[models::TeamMember], user_id: &str) -> Option<String> {
    member_user_and_membership_ids(members)
        .into_iter()
        .find(|(uid, _)| uid == user_id)
        .map(|(_, membership_id)| membership_id)
}

// ---------------------------------------------------------------------------
// Assets (image upload)
// ---------------------------------------------------------------------------
//
// These exercise the full asset lifecycle against the real service + S3 + the
// async storage-event worker:
//   POST /assets            -> a pending asset with a presigned PUT target
//   PUT bytes to that URL   -> object lands in the private bucket
//   S3 -> EventBridge -> SQS -> worker flips the asset to `uploaded`
//   attach the asset id     -> profile_image_asset_id / header_photo_asset_ids
//
// The attach-validation tests don't need a real upload; they assert the server
// rejects assets that aren't uploaded / not owned by the caller / of the wrong
// purpose, which is the security-critical surface.

/// A tiny valid 1x1 PNG (67 bytes) used as upload payload.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Create a pending asset for the given purpose sized to `TINY_PNG`.
async fn create_png_asset(config: &Configuration, purpose: models::UploadPurpose) -> models::Asset {
    assets_post(
        config,
        models::CreateAssetInput {
            purpose,
            content_type: "image/png".to_string(),
            content_length: TINY_PNG.len() as i64,
        },
    )
    .await
    .expect("create asset")
}

/// Replay a presigned upload target against storage with the PNG bytes, then
/// wait for the storage-event worker to flip the asset to `uploaded`. Returns the
/// uploaded asset. This drives the real S3 PUT + async pipeline.
async fn upload_and_confirm(config: &Configuration, asset: &models::Asset) -> models::Asset {
    let target = asset
        .upload
        .as_ref()
        .expect("pending asset has an upload target");
    let client = reqwest::Client::new();
    let mut req = client
        .request(
            target.method.parse().expect("valid upload method"),
            &target.upload_url,
        )
        .body(TINY_PNG.to_vec());
    // Replay exactly the headers the server signed (content-type, length, ...).
    for h in &target.headers {
        req = req.header(&h.name, &h.value);
    }
    let res = req.send().await.expect("PUT bytes to storage");
    assert!(
        res.status().is_success(),
        "upload PUT failed: {} {}",
        res.status(),
        res.text().await.unwrap_or_default()
    );

    // The worker marks it uploaded off the S3 event — eventually consistent.
    eventually("asset to be marked uploaded", || async {
        let a = assets_asset_id_get(config, &asset.id).await.ok()?;
        matches!(a.status, models::AssetStatus::Uploaded).then_some(a)
    })
    .await
}

#[tokio::test]
async fn create_asset_returns_pending_with_upload_target() {
    let (config, _user) = new_user().await;

    let asset = create_png_asset(&config, models::UploadPurpose::ProfileImage).await;

    assert!(matches!(asset.status, models::AssetStatus::Pending));
    assert_eq!(asset.content_type, "image/png");
    assert!(
        asset.url.is_none(),
        "a pending asset has no serving url yet"
    );
    let target = asset
        .upload
        .expect("pending asset carries an upload target");
    assert_eq!(target.method, "PUT");
    assert!(
        target.upload_url.starts_with("http"),
        "upload_url should be a real URL, got {:?}",
        target.upload_url
    );
}

#[tokio::test]
async fn create_asset_rejects_non_image_content_type() {
    let (config, _user) = new_user().await;

    let response = assets_post(
        &config,
        models::CreateAssetInput {
            purpose: models::UploadPurpose::ProfileImage,
            content_type: "application/pdf".to_string(),
            content_length: 1024,
        },
    )
    .await;
    assert_bad_request(response);
}

#[tokio::test]
async fn create_asset_rejects_oversized_content_length() {
    let (config, _user) = new_user().await;

    // 11 MB — over the 10 MB server cap.
    let response = assets_post(
        &config,
        models::CreateAssetInput {
            purpose: models::UploadPurpose::ProfileImage,
            content_type: "image/png".to_string(),
            content_length: 11 * 1024 * 1024,
        },
    )
    .await;
    assert_bad_request(response);
}

#[tokio::test]
async fn create_asset_rejects_zero_content_length() {
    let (config, _user) = new_user().await;

    let response = assets_post(
        &config,
        models::CreateAssetInput {
            purpose: models::UploadPurpose::ProfileImage,
            content_type: "image/png".to_string(),
            content_length: 0,
        },
    )
    .await;
    assert_bad_request(response);
}

#[tokio::test]
async fn get_missing_asset_returns_not_found() {
    let (config, _user) = new_user().await;

    let response = assets_asset_id_get(&config, "does-not-exist").await;
    assert_not_found(response);
}

#[tokio::test]
async fn upload_profile_image_end_to_end() {
    let (config, _user) = new_user().await;

    // Create -> PUT -> worker marks uploaded.
    let asset = create_png_asset(&config, models::UploadPurpose::ProfileImage).await;
    let uploaded = upload_and_confirm(&config, &asset).await;
    assert!(
        uploaded.url.is_some(),
        "uploaded asset carries a serving url"
    );
    assert!(
        uploaded.upload.is_none(),
        "uploaded asset has no upload target"
    );

    // Attach it to the profile; it should surface on /users/me.
    let updated = users_me_patch(
        &config,
        models::UpdateUserInput {
            name: None,
            profile_image_asset_id: Some(asset.id.clone()),
            rating_visibility: None,
        },
    )
    .await
    .expect("attach profile image");
    let photo = updated
        .profile
        .profile_image
        .expect("profile image is set after attach");
    assert!(!photo.image_url.is_empty());
}

/// `POST /teams` accepts a `team_logo`-purpose asset the same way `PATCH
/// /users/me` accepts a `profile_image` one.
#[tokio::test]
async fn create_team_with_logo() {
    let (config, _user) = new_user().await;

    let asset = create_png_asset(&config, models::UploadPurpose::TeamLogo).await;
    upload_and_confirm(&config, &asset).await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Pictured FC".to_string(),
            logo_asset_id: Some(asset.id.clone()),
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team with logo");
    let logo = team.logo.expect("logo is set from creation");
    assert!(!logo.image_url.is_empty());
}

/// `PATCH /teams/:id` rejects a profile-image asset uploaded for a different
/// purpose — same ownership/purpose check `PATCH /users/me` runs.
#[tokio::test]
async fn create_team_rejects_wrong_purpose_asset() {
    let (config, _user) = new_user().await;

    let asset = create_png_asset(&config, models::UploadPurpose::ProfileImage).await;
    upload_and_confirm(&config, &asset).await;

    let response = teams_post(
        &config,
        models::CreateTeamInput {
            name: "Wrong Purpose FC".to_string(),
            logo_asset_id: Some(asset.id.clone()),
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await;
    assert_bad_request(response);
}

/// Creating a team with `invited_user_ids` bundles in a real invite: the
/// invitee gets a pending roster slot (shows up on the team immediately) *and*
/// a standalone invitation they can find in their inbox and accept — at which
/// point the roster slot flips to accepted. This is the path that used to be
/// a no-op (`add_team_invitations`' deferred TODO): a team invitation created
/// no roster entry, so acceptance had nothing to link and would 404.
#[tokio::test]
async fn team_created_with_initial_invite_can_be_accepted() {
    let (owner_config, owner) = new_user().await;
    let (invitee_config, invitee) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Founders FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team with initial invite");

    // The invitee already has a roster slot, pending — members are listed
    // separately (paginated), not embedded on the team itself.
    let initial_members = all_team_members(&owner_config, &team.id).await;
    assert!(
        member_ids(&initial_members).contains(&invitee.profile.id),
        "invitee should appear on the team roster immediately, pending acceptance"
    );
    // The creator is the only accepted (non-invited) member so far.
    assert!(member_ids(&initial_members).contains(&owner.profile.id));

    // ...and a standalone invitation in their inbox.
    let inbox = users_me_invitations_get(&invitee_config, None, None, None)
        .await
        .expect("inbox");
    let detail = inbox
        .items
        .iter()
        .find(|i| {
            matches!(&*i.context,
            models::InvitationContext::Team(ctx) if ctx.team_id == team.id)
        })
        .expect("team invitation in inbox");

    // Accepting it succeeds (this used to 404 — no roster entry to link).
    let responded = invitations_invitation_id_respond_post(
        &invitee_config,
        &detail.invitation.id,
        models::RespondToInvitationInput {
            response: models::InvitationResponse::Accepted,
            side_id: None,
        },
    )
    .await
    .expect("accept team invitation");
    assert!(matches!(
        responded.status,
        models::InvitationStatus::Accepted
    ));

    // The team now shows the invitee as a full (accepted) member.
    let updated_members = all_team_members(&owner_config, &team.id).await;
    let membership_id =
        membership_id_for(&updated_members, &invitee.profile.id).expect("invitee still on roster");
    let member = updated_members
        .iter()
        .find(|m| match &*m.member {
            models::Member::User(u) => u.id == membership_id,
            models::Member::External(_) => false,
        })
        .expect("invitee's membership row");
    match &*member.member {
        models::Member::User(u) => {
            let invitation = u
                .invitation
                .as_ref()
                .expect("membership carries invitation");
            assert!(matches!(
                invitation.status,
                models::InvitationStatus::Accepted
            ));
        }
        models::Member::External(_) => panic!("expected a linked user member"),
    }
}

/// `CreateTeamInput.invited_role` sets the role every bundled invitee lands
/// with — checked before acceptance, since `build_invited_team_member`
/// stamps it onto the pending roster slot immediately, not just onto the
/// membership once accepted. Covers both a user invitee and an external
/// (token) one, since only one of those is ever linked to a real account.
#[tokio::test]
async fn initial_invitees_land_with_the_requested_role() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Admins FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec!["Guest Coach".to_string()],
            invited_role: Some(models::AssignableTeamRole::Admin),
        },
    )
    .await
    .expect("create team with admin invites");

    let members = all_team_members(&owner_config, &team.id).await;
    let invitee_role = members
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.user_id == invitee.profile.id => Some(m.role),
            _ => None,
        })
        .expect("user invitee on roster");
    assert_eq!(invitee_role, models::TeamRole::Admin);
    let external_role = members
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::External(e) if e.display_name == "Guest Coach" => Some(m.role),
            _ => None,
        })
        .expect("external invitee on roster");
    assert_eq!(external_role, models::TeamRole::Admin);
}

/// Same as above but via the standalone `POST /teams/:id/invitations`
/// endpoint — `AddInvitationsInput.role`, the shared invite input's
/// team-only field (mirroring `side_id`'s match-only one).
#[tokio::test]
async fn invitations_endpoint_honors_requested_role() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Later Admins FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    teams_team_id_invitations_post(
        &owner_config,
        &team.id,
        models::AddInvitationsInput {
            invited_user_ids: vec![invitee.profile.id.clone()],
            invited_external_names: vec![],
            side_id: None,
            role: Some(models::AssignableTeamRole::Admin),
        },
    )
    .await
    .expect("invite as admin");

    let members = all_team_members(&owner_config, &team.id).await;
    let role = members
        .iter()
        .find_map(|m| match &*m.member {
            models::Member::User(u) if u.user_id == invitee.profile.id => Some(m.role),
            _ => None,
        })
        .expect("invitee on roster");
    assert_eq!(role, models::TeamRole::Admin);
}

/// A total stranger — not a member, not invited, nothing — can still view a
/// team and its roster: `GET /teams/:id` and `GET /teams/:id/members` carry
/// no permission check (see `get_team`/`list_team_members`), unlike every
/// management endpoint (`team_admin_can_manage_but_member_cannot` covers
/// those being rejected). Teams are open to view for now, same as a user's
/// own profile.
#[tokio::test]
async fn stranger_can_view_team_and_members_read_only() {
    let (owner_config, owner) = new_user().await;
    let (stranger_config, _stranger) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Open View FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    let seen = teams_team_id_get(&stranger_config, &team.id)
        .await
        .expect("stranger can view the team");
    assert_eq!(seen.name, "Open View FC");

    let members = teams_team_id_members_get(&stranger_config, &team.id, None, None)
        .await
        .expect("stranger can view the roster");
    assert!(member_ids(&members.items).contains(&owner.profile.id));
}

/// When a team is deleted, matches it played keep their `team_id` snapshot
/// (not scrubbed — same "reference outlives the record" shape as
/// `deleted_user_profile` for a deleted account) but the side's resolved
/// `name` falls back to "Deleted team" instead of the team's (now gone) real
/// name, with no logo. Uses a side with no players so the sole-player-name
/// fallback doesn't win over the team's name and mask what we're testing.
#[tokio::test]
async fn deleted_teams_matches_show_deleted_team() {
    let (owner_config, _owner) = new_user().await;
    let (_opponent_config, opponent) = new_user().await;

    let team = teams_post(
        &owner_config,
        models::CreateTeamInput {
            name: "Doomed FC".to_string(),
            logo_asset_id: None,
            invited_user_ids: vec![],
            invited_external_names: vec![],
            invited_role: None,
        },
    )
    .await
    .expect("create team");

    // Side "a" carries the team id and no players, so the team's name (not a
    // sole player's) is what resolves. Side "b" is a normal opponent side.
    let mut input = match_between("Doomed Match", &[], &[&opponent.profile.id]);
    input.sides[0].team_id = Some(team.id.clone());
    input.sides[0].name = None;
    let created = matches_post(&owner_config, input)
        .await
        .expect("create match");

    let before = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match before team is deleted");
    let side_before = before
        .sides
        .iter()
        .find(|s| s.team_id.as_deref() == Some(team.id.as_str()))
        .expect("team side");
    assert_eq!(
        side_before.name.as_deref(),
        Some(team.name.as_str()),
        "resolves to the team's real name while it still exists"
    );

    teams_team_id_delete(&owner_config, &team.id)
        .await
        .expect("delete team");

    let after = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match after team is deleted");
    let side_after = after
        .sides
        .iter()
        .find(|s| s.id == side_before.id)
        .expect("side is still there");
    assert_eq!(side_after.name.as_deref(), Some("Deleted team"));
    assert_eq!(
        side_after.team_id.as_deref(),
        Some(team.id.as_str()),
        "the team_id snapshot is preserved, not scrubbed"
    );
    assert!(side_after.team_logo.is_none());
}

#[tokio::test]
async fn upload_match_header_end_to_end() {
    let (config, _user) = new_user().await;

    let asset = create_png_asset(&config, models::UploadPurpose::MatchHeader).await;
    upload_and_confirm(&config, &asset).await;

    // Create a match with the uploaded asset as its header.
    let mut input = match_between("Header Match", &[], &[]);
    input.header_photo_asset_ids = Some(vec![asset.id.clone()]);
    let created = matches_post(&config, input).await.expect("create match");

    assert_eq!(
        created.header_photos.len(),
        1,
        "the uploaded header should be attached"
    );
    assert!(!created.header_photos[0].image_url.is_empty());
    assert_eq!(
        created.header_photos[0].asset_id.as_deref(),
        Some(asset.id.as_str()),
        "the photo's asset id round-trips so it can be reordered/kept on a later edit"
    );

    // Adding a second photo via PATCH, re-sending the first photo's asset id
    // alongside the new one, keeps both rather than replacing the first.
    let second_asset = create_png_asset(&config, models::UploadPurpose::MatchHeader).await;
    upload_and_confirm(&config, &second_asset).await;
    let first_asset_id = created.header_photos[0]
        .asset_id
        .clone()
        .expect("first photo has an asset id");
    let updated = matches_match_id_patch(
        &config,
        &created.id,
        models::UpdateMatchInput {
            header_photo_asset_ids: Some(vec![second_asset.id.clone(), first_asset_id.clone()]),
            ..Default::default()
        },
    )
    .await
    .expect("patch photos");
    assert_eq!(updated.header_photos.len(), 2, "both photos are kept");
    assert_eq!(
        updated.header_photos[0].asset_id.as_deref(),
        Some(second_asset.id.as_str()),
        "the new photo is first, matching the order it was sent in"
    );
    assert_eq!(
        updated.header_photos[1].asset_id.as_deref(),
        Some(first_asset_id.as_str()),
    );
}

#[tokio::test]
async fn attach_rejects_pending_asset() {
    let (config, _user) = new_user().await;

    // Created but never uploaded → still pending.
    let asset = create_png_asset(&config, models::UploadPurpose::ProfileImage).await;

    let response = users_me_patch(
        &config,
        models::UpdateUserInput {
            name: None,
            profile_image_asset_id: Some(asset.id.clone()),
            rating_visibility: None,
        },
    )
    .await;
    assert_bad_request(response);
}

#[tokio::test]
async fn attach_rejects_asset_owned_by_another_user() {
    let (owner_config, _owner) = new_user().await;
    let (other_config, _other) = new_user().await;

    // Owner uploads a profile image.
    let asset = create_png_asset(&owner_config, models::UploadPurpose::ProfileImage).await;
    upload_and_confirm(&owner_config, &asset).await;

    // A different user tries to attach it → rejected (ownership check).
    let response = users_me_patch(
        &other_config,
        models::UpdateUserInput {
            name: None,
            profile_image_asset_id: Some(asset.id.clone()),
            rating_visibility: None,
        },
    )
    .await;
    assert_bad_request(response);
}

#[tokio::test]
async fn attach_rejects_wrong_purpose_asset() {
    let (config, _user) = new_user().await;

    // Upload a match_header asset, then try to use it as a profile image.
    let asset = create_png_asset(&config, models::UploadPurpose::MatchHeader).await;
    upload_and_confirm(&config, &asset).await;

    let response = users_me_patch(
        &config,
        models::UpdateUserInput {
            name: None,
            profile_image_asset_id: Some(asset.id.clone()),
            rating_visibility: None,
        },
    )
    .await;
    assert_bad_request(response);
}

// ---------------------------------------------------------------------------
// Netball live scoring — both of netball's two live-scoring methods
// (event-by-event, quarter-only) fold through the same `NetballLiveEvent`
// vocabulary, so both are exercised here against the real service.
//
// The append call is made with raw JSON over `reqwest` rather than the
// generated client: the generated Rust model for a *sport* union nested
// around a *kind* union (`LiveEventInput` -> `NetballLiveEvent`/
// `FootballLiveEvent`/...) flattens incorrectly (a pre-existing
// openapi-generator limitation that predates netball and affects football's
// Goal/Card/Substitution/Period kinds too — see `docs/openapi-client.md`),
// so its `kind` enum only ever contains one variant instead of all of them.
// Reading the result back is unaffected — `GET /matches/:id/score` returns
// `Score`, which embeds goals/fouls as plain (non-nested-union) structs — so
// that leg of each test still goes through the typed client.
// ---------------------------------------------------------------------------

/// A netball match between two invited users, scheduled in the future (no
/// create-time score) so live events can be appended afterward. The owner is
/// assigned to side "a" so they're a participant and can record events.
fn netball_match_input(invited_user_id: &str) -> models::CreateMatchInput {
    let mut input = create_match_input(invited_user_id);
    input.match_type = models::MatchType::Netball;
    input.creator_side_client_id = Some("a".to_string());
    input
}

fn netball_goal_event_json(side_id: &str, two_points: bool) -> serde_json::Value {
    serde_json::json!({
        "sport": "Netball",
        "kind": "Goal",
        "side_id": side_id,
        "two_points": two_points,
    })
}

fn netball_foul_event_json(side_id: &str) -> serde_json::Value {
    serde_json::json!({
        "sport": "Netball",
        "kind": "Foul",
        "side_id": side_id,
        "foul_kind": "contact",
    })
}

fn netball_period_event_json(period: &str, scores: &[(&str, i32)]) -> serde_json::Value {
    serde_json::json!({
        "sport": "Netball",
        "kind": "Period",
        "period": period,
        "score": scores.iter().copied().collect::<std::collections::HashMap<_, _>>(),
    })
}

/// POST `/matches/:id/live/events` with raw JSON events (see this section's
/// doc comment for why), returning the parsed `LiveScoreSnapshot`.
async fn append_live_events_raw(
    config: &Configuration,
    match_id: &str,
    expected_last_seq: i32,
    events: Vec<serde_json::Value>,
) -> models::LiveScoreSnapshot {
    let body = serde_json::json!({
        "expected_last_seq": expected_last_seq,
        "events": events
            .into_iter()
            .map(|event| serde_json::json!({ "occurred_at": iso_offset_hours(0), "event": event }))
            .collect::<Vec<_>>(),
    });
    let res = reqwest::Client::new()
        .post(format!(
            "{}/matches/{match_id}/live/events",
            config.base_path
        ))
        .bearer_auth(
            config
                .bearer_access_token
                .as_ref()
                .expect("config has a bearer token"),
        )
        .json(&body)
        .send()
        .await
        .expect("send append request");
    assert!(
        res.status().is_success(),
        "append live events failed: {} {}",
        res.status(),
        res.text().await.unwrap_or_default()
    );
    res.json().await.expect("parse LiveScoreSnapshot")
}

/// Appending `Goal`/`Foul` events (plus a `Period` marker for time-tracking)
/// derives the running score from the goals themselves — the event-by-event
/// method.
#[tokio::test]
async fn netball_event_by_event_scoring_derives_score_from_goals() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    let snapshot = append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, true),
            netball_goal_event_json(&side_b, false),
            netball_foul_event_json(&side_b),
            netball_period_event_json("quarter_one_end", &[(&side_a, 3), (&side_b, 1)]),
        ],
    )
    .await;
    assert_eq!(snapshot.last_seq, 5);

    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Netball(s) => {
            assert_eq!(s.score.get(&side_a), Some(&3));
            assert_eq!(s.score.get(&side_b), Some(&1));
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(3));
            assert_eq!(s.fouls.as_ref().map(Vec::len), Some(1));
            assert!(matches!(
                s.period,
                Some(models::NetballPeriod::QuarterOneEnd)
            ));
            let quarter_one_score = s
                .period_scores
                .as_ref()
                .expect("period_scores present")
                .get("quarter_one_end")
                .expect("quarter one entry present");
            assert_eq!(quarter_one_score.get(&side_a), Some(&3));
            assert_eq!(quarter_one_score.get(&side_b), Some(&1));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }
}

/// Appending only `Period` markers (no `Goal`/`Foul` events at all) derives
/// the score purely from each marker's own `score` field — the quarter-only
/// method.
#[tokio::test]
async fn netball_quarter_only_scoring_uses_period_marker_score_directly() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_period_event_json("quarter_one_end", &[(&side_a, 12), (&side_b, 9)]),
            netball_period_event_json("quarter_two_end", &[(&side_a, 22), (&side_b, 18)]),
        ],
    )
    .await;

    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Netball(s) => {
            assert_eq!(s.score.get(&side_a), Some(&22));
            assert_eq!(s.score.get(&side_b), Some(&18));
            // No Goal events at all — no goal-by-goal detail to hand over.
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(0));
            assert!(matches!(
                s.period,
                Some(models::NetballPeriod::QuarterTwoEnd)
            ));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Undo (`DELETE /matches/:id/live/events/:seq`) — restricted to the log's
// current tip. Exercised against netball's event vocabulary since the
// underlying append/delete/seq machinery under test is shared by every sport
// (see `Dao::append_live_events`/`delete_live_event`); nothing here is
// netball-specific.
// ---------------------------------------------------------------------------

/// Regression test for the bug where a UI client seeded its append token
/// (`expected_last_seq`) from the physical event log's own max seq — fine
/// before any undo, but permanently wrong after one, since undoing bumps the
/// real `live_seq` counter past the deleted event without leaving a matching
/// physical item behind (see `Dao::delete_live_event`'s doc comment). In the
/// app this showed up as: undo an event, refresh the page (throwing away the
/// in-session cache that had been seeded correctly from the undo's own
/// response), and every append from then on 409s forever. `GET
/// /matches/:id/live/seq` exists precisely so a fresh client has a correct
/// value to seed from instead of falling back to the event log.
#[tokio::test]
async fn live_seq_endpoint_reflects_the_real_counter_not_the_physical_log_max() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();

    // No events yet — the counter is 0.
    let seq = matches_match_id_live_seq_get(&owner_config, &created.id)
        .await
        .expect("get live seq");
    assert_eq!(seq.last_seq, 0);

    append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, false),
        ],
    )
    .await;

    let seq = matches_match_id_live_seq_get(&owner_config, &created.id)
        .await
        .expect("get live seq");
    assert_eq!(seq.last_seq, 2);

    // Undo the tip — the physical log's own max seq drops to 1, but the real
    // counter is 3 (bumped past the deleted event). This endpoint must
    // report the counter, not the physical max.
    matches_match_id_live_events_seq_delete(&owner_config, &created.id, 2)
        .await
        .expect("undo the second goal");

    let page = matches_match_id_live_events_get(&owner_config, &created.id, None, None)
        .await
        .expect("list live events");
    let physical_max = page.items.iter().map(|e| e.seq).max().unwrap_or(0);
    assert_eq!(physical_max, 1, "physical log's own max seq after the undo");

    let seq = matches_match_id_live_seq_get(&owner_config, &created.id)
        .await
        .expect("get live seq");
    assert_eq!(
        seq.last_seq, 3,
        "the real counter, not the physical log's max seq (1)"
    );

    // And it's exactly the value the very next append needs as
    // expected_last_seq — the point of the whole endpoint.
    let after_append = append_live_events_raw(
        &owner_config,
        &created.id,
        seq.last_seq,
        vec![netball_goal_event_json(&side_a, false)],
    )
    .await;
    assert_eq!(after_append.last_seq, 4);
}

/// Regression test for the bug where undoing the tip event left the client
/// unable to record anything else afterward.
///
/// `delete_live_event` bumps the match's `live_seq` counter by one on every
/// undo (see that DAO method's doc comment) — so the snapshot handed back
/// from delete must report `seq + 1` as `last_seq`, the value the caller
/// then sends back as `expected_last_seq` on its very next append. Before
/// this, the endpoint instead reported one *less* than that (the deleted
/// tip's seq no longer physically present among the remaining events), so
/// every append after an undo sent a stale `expected_last_seq` that could
/// never satisfy the server's optimistic-concurrency check, and 409'd
/// forever — exactly what was reported: undo appears to succeed, then every
/// subsequent event fails to record.
#[tokio::test]
async fn undoing_the_tip_lets_scoring_continue_without_reusing_its_seq() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // Three events: two goals for A (seq 1, 2), then a foul on B (seq 3) that
    // we'll undo.
    let snapshot = append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, true),
            netball_foul_event_json(&side_b),
        ],
    )
    .await;
    assert_eq!(snapshot.last_seq, 3);

    // Undo the tip (the foul, seq 3).
    let after_undo = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 3)
        .await
        .expect("undo tip event");

    // The counter advances *past* the deleted seq (3 -> 4) rather than
    // staying put — this is the value the client caches as its next
    // `expected_last_seq`.
    assert_eq!(
        after_undo.last_seq, 4,
        "last_seq after undoing seq 3 must be the bumped counter (4)"
    );
    match *after_undo.score {
        models::Score::Netball(s) => {
            // The foul is gone from the derived score...
            assert_eq!(s.fouls.as_ref().map(Vec::len), Some(0));
            // ...but the two goals that preceded it are untouched.
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(2));
            assert_eq!(s.score.get(&side_a), Some(&3));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // The very next append — this is the call that used to 409 forever after
    // an undo — using the `last_seq` the delete just handed back.
    let after_append = append_live_events_raw(
        &owner_config,
        &created.id,
        after_undo.last_seq,
        vec![netball_goal_event_json(&side_b, false)],
    )
    .await;
    // The new event lands at seq 5, not the freed-up seq 3 or the burned
    // seq 4 — seq numbers are never reused once assigned, even across an
    // undo.
    assert_eq!(after_append.last_seq, 5);

    // The physical log confirms the (two-number) gap: 1, 2, 5 — seq 3
    // (deleted) and seq 4 (burned by the undo's counter bump) both stay
    // permanently absent, never reused by the new event.
    let page = matches_match_id_live_events_get(&owner_config, &created.id, None, None)
        .await
        .expect("list live events");
    let seqs: Vec<i32> = page.items.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 5]);

    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Netball(s) => {
            assert_eq!(s.score.get(&side_a), Some(&3));
            assert_eq!(s.score.get(&side_b), Some(&1));
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(3));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }
}

/// Only the current tip can be undone — a mid-log seq is rejected with a 400
/// rather than silently reordering the log. This check is now enforced
/// atomically inside `Dao::delete_live_event`'s transaction (against the
/// live counter at commit time) rather than as a separate read-then-check in
/// the handler, but the outward behavior is unchanged: the tip itself stays
/// intact after the rejection.
#[tokio::test]
async fn undoing_a_non_tip_event_is_rejected() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();

    let snapshot = append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, false),
        ],
    )
    .await;
    assert_eq!(snapshot.last_seq, 2);

    // seq 1 is no longer the tip (seq 2 is) — rejected.
    let response = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 1).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "only the most recently recorded event can be undone",
    );

    // Confirmed untouched: both goals are still there, and the tip is still 2.
    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Netball(s) => assert_eq!(s.goals.as_ref().map(Vec::len), Some(2)),
        other => panic!("expected a netball score, got {other:?}"),
    }
}

/// Retrying an undo with the *same, now-stale* seq 400s just like any other
/// non-tip delete: the counter has already moved past it (see the previous
/// test's sibling scenario), so a second call with that same number no
/// longer matches the current tip.
#[tokio::test]
async fn retrying_an_undo_with_the_same_seq_is_rejected_as_non_tip() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();

    append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![netball_goal_event_json(&side_a, false)],
    )
    .await;

    matches_match_id_live_events_seq_delete(&owner_config, &created.id, 1)
        .await
        .expect("undo the only event");

    // seq 1 was the tip a moment ago, but the counter has since bumped to 2
    // (see `undoing_the_tip_lets_scoring_continue_without_reusing_its_seq`)
    // — a repeat call with the same stale seq is exactly the "not the tip
    // anymore" case, same as a genuinely non-tip seq.
    let response = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 1).await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::BAD_REQUEST,
        "only the most recently recorded event can be undone",
    );
}

/// Undoing *the counter's current value* when nothing physically lives
/// there — the realistic "hit undo twice in a row" case, since a client
/// caches the server's returned (bumped) `last_seq` as its next tip and
/// would retry against that, not the original seq. The tip check passes
/// (it genuinely is the current counter value), but there's nothing left to
/// delete: `live_seq` having advanced past the log's last real event
/// doesn't mean a real event lives at that new number.
#[tokio::test]
async fn undoing_the_bumped_tip_with_nothing_left_to_delete_is_not_found() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();

    append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![netball_goal_event_json(&side_a, false)],
    )
    .await;

    let after_undo = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 1)
        .await
        .expect("undo the only event");
    assert_eq!(after_undo.last_seq, 2);

    // A client would cache `2` (the response above) as its next tip and
    // retry undo against that, exactly like this.
    let response =
        matches_match_id_live_events_seq_delete(&owner_config, &created.id, after_undo.last_seq)
            .await;
    assert_not_found(response);
}

/// Two, then three, successful undos in a row — each one targeting the log's
/// *real* physical tip at that point (2, then 1), not the previous undo's
/// bumped `last_seq`. This is the scenario the UI's `useUndoTargetSeq` split
/// exists for (see that hook's doc comment): every undo advances `live_seq`
/// by one past the deleted event, so consecutive undos land on a strictly
/// increasing sequence of `last_seq` values (4, then 5, then 6) even while
/// the *targets* count down (3, then 2, then 1). Confirms the derived score
/// and physical log both end up completely empty once every event has been
/// undone, not just "missing the most recent one" — and, the actual point
/// of the original bug report this whole chain of fixes started from,
/// confirms scoring can still continue afterward: one final append,
/// `expected_last_seq` all the way down at the fully-undone counter value.
#[tokio::test]
async fn undoing_multiple_times_in_a_row_removes_each_real_tip_in_turn() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // Three events: two goals for A (seq 1, 2), then a foul on B (seq 3).
    let snapshot = append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, false),
            netball_foul_event_json(&side_b),
        ],
    )
    .await;
    assert_eq!(snapshot.last_seq, 3);

    // Undo #1: the foul (seq 3, the real tip). Counter bumps 3 -> 4.
    let after_first = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 3)
        .await
        .expect("undo the foul");
    assert_eq!(after_first.last_seq, 4);
    match *after_first.score {
        models::Score::Netball(s) => {
            assert_eq!(s.fouls.as_ref().map(Vec::len), Some(0));
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(2));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // Undo #2: the real tip is now seq 2 (the second goal) — NOT
    // `after_first.last_seq` (4), which is the bumped, phantom counter. A
    // client sending 4 here would 404 (see the previous test); the real
    // target is 2.
    let after_second = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 2)
        .await
        .expect("undo the second goal");
    // The counter keeps climbing (3 -> 4 was undo #1's bump; this one bumps
    // 4 -> 5)... but note the *target* seq (2) is nowhere near the counter
    // (5): `live_seq` tracks "how many mutations have ever happened", not
    // "how many events currently exist".
    assert_eq!(after_second.last_seq, 5);
    match *after_second.score {
        models::Score::Netball(s) => {
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(1));
            assert_eq!(s.score.get(&side_a), Some(&1));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // Undo #3: the real tip is now seq 1 (the first, and last remaining,
    // goal) — the log goes fully empty. The counter keeps climbing the same
    // way it did for undo #2 (5 -> 6), even though there's nothing left to
    // undo afterward — `live_seq` never resets just because the log is
    // momentarily empty.
    let after_third = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 1)
        .await
        .expect("undo the first goal");
    assert_eq!(after_third.last_seq, 6);
    match *after_third.score {
        models::Score::Netball(s) => {
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(0));
            assert_eq!(
                s.score.get(&side_a),
                None,
                "absence means zero, same as never scored"
            );
            assert_eq!(s.score.get(&side_b), None);
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // Nothing physically left in the log at all.
    let page = matches_match_id_live_events_get(&owner_config, &created.id, None, None)
        .await
        .expect("list live events");
    assert!(page.items.is_empty());

    // ...and the fully-recomputed score agrees.
    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Netball(s) => {
            assert!(s.score.is_empty());
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(0));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // The actual point of all this: scoring can still continue afterward.
    // `expected_last_seq` must be `after_third.last_seq` (6) — the climbed
    // counter, not 0 — even though every event ever recorded has now been
    // undone.
    let after_append = append_live_events_raw(
        &owner_config,
        &created.id,
        after_third.last_seq,
        vec![netball_goal_event_json(&side_a, false)],
    )
    .await;
    // Lands at seq 7, continuing on from the counter — not seq 1, which
    // would silently collide with the (deleted) first goal's old identity.
    assert_eq!(after_append.last_seq, 7);
    match *after_append.score {
        models::Score::Netball(s) => {
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(1));
            assert_eq!(s.score.get(&side_a), Some(&1));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }
}

/// Undo, then append (log not empty — covered by
/// `undoing_the_tip_lets_scoring_continue_without_reusing_its_seq`), then
/// undo *again* — this time undoing the event that append just added. Checks
/// that a freshly-appended event becomes a correctly-deletable tip in its
/// own right, i.e. that appending after an undo doesn't leave the counter
/// and the physical log in some inconsistent state that only tolerates
/// further appends, not a subsequent real undo too.
#[tokio::test]
async fn undoing_then_appending_then_undoing_the_new_event_stays_consistent() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, netball_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // Two goals for A (seq 1, 2).
    let snapshot = append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            netball_goal_event_json(&side_a, false),
            netball_goal_event_json(&side_a, false),
        ],
    )
    .await;
    assert_eq!(snapshot.last_seq, 2);

    // Undo the tip (seq 2) — one goal remains (seq 1), log not empty.
    let after_undo = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 2)
        .await
        .expect("undo the second goal");
    assert_eq!(after_undo.last_seq, 3);

    // Append a goal for B — lands at seq 4 (the bumped counter + 1), not
    // seq 2 (the deleted goal's old, never-reused identity).
    let after_append = append_live_events_raw(
        &owner_config,
        &created.id,
        after_undo.last_seq,
        vec![netball_goal_event_json(&side_b, false)],
    )
    .await;
    assert_eq!(after_append.last_seq, 4);
    match *after_append.score {
        models::Score::Netball(s) => {
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(2));
            assert_eq!(s.score.get(&side_a), Some(&1));
            assert_eq!(s.score.get(&side_b), Some(&1));
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // Now undo the *newly appended* event (seq 4) — the real question this
    // test asks: does the append above leave seq 4 undoable, same as any
    // other tip?
    let after_second_undo = matches_match_id_live_events_seq_delete(&owner_config, &created.id, 4)
        .await
        .expect("undo the newly appended goal");
    assert_eq!(after_second_undo.last_seq, 5);
    match *after_second_undo.score {
        models::Score::Netball(s) => {
            // Back to just the one surviving original goal.
            assert_eq!(s.goals.as_ref().map(Vec::len), Some(1));
            assert_eq!(s.score.get(&side_a), Some(&1));
            assert_eq!(s.score.get(&side_b), None);
        }
        other => panic!("expected a netball score, got {other:?}"),
    }

    // The physical log agrees: only seq 1 (the first goal) survives all of
    // this — 2 and 4 were both undone, 3 was never a real event (just a
    // burned counter value).
    let page = matches_match_id_live_events_get(&owner_config, &created.id, None, None)
        .await
        .expect("list live events");
    let seqs: Vec<i32> = page.items.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1]);
}

// ---------------------------------------------------------------------------
// Result editor / live scoring integration — a manually submitted `score`
// (the general "add/edit result" editor, `MatchResultEditor`) is held to the
// same live-derived cross-check as finishing the live-scored game from its
// own screen (`LiveScoringPage`'s `finishMatch`), whether or not the
// submission is the one that completes the match. Regression coverage for a
// bug where a score submitted while live scoring was still under way
// (`in_progress`, no `status` in the request) slipped past the check
// entirely — because it only fired when the request explicitly completed the
// match — and silently displaced the live-scored detail instead of finishing
// it (see `update_match`'s doc comment on the check, and `override_live_score`
// on `UpdateMatchInput`).
// ---------------------------------------------------------------------------

/// A football match between two invited users, scheduled in the future (no
/// create-time score) so live events can be appended afterward. The owner is
/// assigned to side "a" so they're a participant and can record events.
fn football_match_input(invited_user_id: &str) -> models::CreateMatchInput {
    let mut input = create_match_input(invited_user_id);
    input.match_type = models::MatchType::Football;
    input.creator_side_client_id = Some("a".to_string());
    input
}

fn football_kick_off_event_json() -> serde_json::Value {
    serde_json::json!({
        "sport": "Football",
        "kind": "Period",
        "period": "kick_off",
    })
}

fn football_goal_event_json(side_id: &str) -> serde_json::Value {
    serde_json::json!({
        "sport": "Football",
        "kind": "Goal",
        "side_id": side_id,
        "own_goal": false,
        "penalty": false,
    })
}

#[tokio::test]
async fn manual_score_conflicting_with_live_football_detail_is_rejected_even_mid_live_scoring() {
    let (owner_config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&owner_config, football_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // Live-score a single goal for side A — recording the first event
    // auto-flips a still-`scheduled` match to `in_progress`.
    append_live_events_raw(
        &owner_config,
        &created.id,
        0,
        vec![
            football_kick_off_event_json(),
            football_goal_event_json(&side_a),
        ],
    )
    .await;

    let mid_scoring = matches_match_id_get(&owner_config, &created.id)
        .await
        .expect("get match");
    assert!(matches!(
        mid_scoring.status,
        models::MatchStatus::InProgress
    ));

    // A manually-entered score that disagrees with the live 1-0 — sent with
    // no `status`, the way the general result editor submits it. This used
    // to be accepted outright (the cross-check only fired when the request
    // explicitly completed the match), silently displacing the live-scored
    // detail while the match was still `in_progress`.
    let response = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 4, 4))),
            ..Default::default()
        },
    )
    .await;
    assert_status_with_content(
        response,
        reqwest::StatusCode::CONFLICT,
        "doesn't match the match's live result",
    );

    // The rejected attempt left the live-scored detail untouched.
    let score = matches_match_id_score_get(&owner_config, &created.id)
        .await
        .expect("get score");
    match score {
        models::Score::Football(s) => assert_eq!(s.score.get(&side_a), Some(&1)),
        other => panic!("expected a football score, got {other:?}"),
    }

    // `override_live_score` submits it anyway — the explicit escape hatch —
    // and, like any genuinely new score, completes the match.
    let overridden = matches_match_id_patch(
        &owner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 4, 4))),
            override_live_score: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("override the live score");
    assert!(matches!(overridden.status, models::MatchStatus::Completed));
}

// ---------------------------------------------------------------------------
// Ratings — the incremental path (phase 2b-i)
// ---------------------------------------------------------------------------
//
// Ratings have no API surface yet (that is phase 3), so these assertions read
// the table the service under test writes to, through `agon_core`'s own DAO —
// same typed keys, same record shapes, no hand-written key strings. That means
// they need DynamoDB credentials as well as a service URL, which `make test`
// supplies from `.env` and the staging CI job does not; see [`rating_dao`] for
// what happens when it can't connect.
//
// Everything here goes through the real pipeline: the API confirms a score,
// which rewrites the match's `#META`, which the stream bridge turns into an
// SQS message, which `agon_worker`'s rating handler picks up. So these are
// also the only coverage `agon_core::dao::rating` has against a real table —
// in particular the two things that cannot be checked in a unit test: that a
// `ratings.<ladder> = :expected` condition on a *map*-typed attribute
// evaluates the way the optimistic lock assumes, and that `mu`/`sigma`
// survive a round trip through DynamoDB's `N` type exactly (they must, or an
// unchanged redelivery would recompute a different movement and look like a
// re-score every time somebody likes a finished match).

/// A DAO on the table under test, or `None` when the environment isn't wired
/// for direct table access.
///
/// The gate is `AGON_TABLE_NAME`, which `make test` exports from `.env`
/// alongside the AWS credentials and endpoint. The staging CI job
/// (`.github/workflows/test.yml`) deliberately sets only `AGON_SERVICE_URL` —
/// it tests a deployed API, and handing CI database credentials to assert on
/// storage internals would be a worse trade than skipping. Every rating test
/// therefore starts by asking for one and returns early without it, printing
/// why so a skipped run is never silent.
async fn rating_dao() -> Option<agon_core::dao::Dao> {
    let Ok(table) = std::env::var("AGON_TABLE_NAME") else {
        eprintln!(
            "SKIPPING rating assertions: AGON_TABLE_NAME is unset, so the table \
             the service writes to can't be read. Run via `make test`."
        );
        return None;
    };
    Some(agon_core::dao::Dao::from_env(table).await)
}

/// Poll until `user_id` has a rating on `ladder` folded from exactly
/// `matches_rated` matches.
///
/// Polls rather than reads once because rating is the far end of an async
/// pipeline (API → DynamoDB stream → SQS → worker), and asserts on
/// `matches_rated` rather than on the rating value because it is the one field
/// that is exactly predictable — a count, not a float.
async fn rating_after_matches(
    dao: &agon_core::dao::Dao,
    user_id: &str,
    ladder: &str,
    matches_rated: u64,
) -> agon_core::dao::records::RatingRecord {
    let owner = agon_core::dao::rating::RatingOwner::user(user_id);
    eventually(
        &format!("{user_id} to be rated on {ladder} from {matches_rated} match(es)"),
        || async {
            dao.get_rating(&owner, ladder)
                .await
                .expect("read rating")
                .filter(|r| r.matches_rated == matches_rated)
        },
    )
    .await
}

/// One user's stored rating on a ladder, or `None` if they have never been
/// rated on it. A point read, for the cases that assert nothing happened.
async fn stored_rating(
    dao: &agon_core::dao::Dao,
    user_id: &str,
    ladder: &str,
) -> Option<agon_core::dao::records::RatingRecord> {
    dao.get_rating(&agon_core::dao::rating::RatingOwner::user(user_id), ladder)
        .await
        .expect("read rating")
}

/// A match's rating contributions, keyed by the account they belong to.
async fn contributions_by_owner(
    dao: &agon_core::dao::Dao,
    match_id: &str,
) -> std::collections::HashMap<String, agon_core::dao::records::RatingContributionRecord> {
    dao.list_rating_contributions(match_id)
        .await
        .expect("list rating contributions")
        .into_iter()
        .map(|c| (c.owner_id.clone(), c))
        .collect()
}

/// One user's whole rating history on a ladder, oldest first.
async fn rating_history(
    dao: &agon_core::dao::Dao,
    user_id: &str,
    ladder: &str,
) -> Vec<agon_core::dao::records::RatingHistoryRecord> {
    dao.list_rating_history(
        &agon_core::dao::rating::RatingOwner::user(user_id),
        ladder,
        None,
        None,
        50,
    )
    .await
    .expect("list rating history")
    .items
}

/// A completed (already-played) tennis match starting at exactly `starts_at`,
/// creator on side "a" with a 6-3 win recorded, `invites` placing everyone
/// else. [`completed_match`] with a caller-chosen start time, because ratings
/// are applied in **played** order and half of what is worth testing here is
/// what happens when confirmation order disagrees with it. The caller passes
/// the literal string so it can compare stored `played_at` values against it
/// (the API re-serializes timestamps, so the round-tripped form on the
/// response is not necessarily character-identical to what was stored).
fn completed_match_at(
    starts_at: &str,
    invites: Vec<models::CreateMatchInviteInput>,
) -> models::CreateMatchInput {
    models::CreateMatchInput {
        starts_at: starts_at.to_string(),
        ..completed_match(invites)
    }
}

/// Play a full ranked tennis match to a confirmed score: `winner` creates it
/// (on side "a", 6-3 in their favour), `loser` accepts their invitation and
/// then confirms the score, which is the write that makes the match eligible
/// to be rated. Returns the created match.
///
/// The accept matters as much as the confirm: a pending invitee has not been
/// established to have played, so a match whose losing side is nothing but
/// unanswered invitations is not rated at all.
async fn play_ranked_tennis_match(
    winner_config: &Configuration,
    loser_config: &Configuration,
    loser_id: &str,
    starts_at: &str,
) -> models::Match {
    let created = matches_post(
        winner_config,
        completed_match_at(starts_at, vec![invite_users("b", &[loser_id])]),
    )
    .await
    .expect("create ranked match");
    accept_match_invitation(loser_config, &created.id).await;
    confirm_pending_score(loser_config, &created).await;
    created
}

/// Confirm a match's outstanding score submission as `config`'s user — the
/// last side to agree, which is what promotes the submission to the match's
/// `confirmed_score`.
async fn confirm_pending_score(config: &Configuration, match_: &models::Match) {
    let submission_id = match_
        .pending_score
        .as_ref()
        .expect("a pending score to confirm")
        .submission_id
        .clone();
    matches_match_id_score_submissions_submission_id_respond_post(
        config,
        &match_.id,
        &submission_id,
        models::RespondToScoreInput {
            response: models::ScoreResponseKind::Confirm,
        },
    )
    .await
    .expect("confirm score");
}

/// Set a match's `ranked` flag directly on its `#META` item.
///
/// There is no API for this until phase 3 — `create_match` hard-codes
/// `ranked: true` — so the friendly-match test has to write the field the way
/// the record defines it. Keyed through `agon_core`'s `Pk`/`Sk` rather than a
/// literal `"MATCH#..."`, so a key change breaks this loudly instead of
/// silently addressing nothing.
async fn set_match_ranked(match_id: &str, ranked: bool) {
    use agon_core::dao::keys::{Pk, Sk};
    let config = aws_config::load_from_env().await;
    aws_sdk_dynamodb::Client::new(&config)
        .update_item()
        .table_name(std::env::var("AGON_TABLE_NAME").expect("AGON_TABLE_NAME"))
        .key(
            "PK",
            aws_sdk_dynamodb::types::AttributeValue::S(Pk::Match(match_id.into()).to_string()),
        )
        .key(
            "SK",
            aws_sdk_dynamodb::types::AttributeValue::S(Sk::Meta.to_string()),
        )
        .update_expression("SET #ranked = :ranked")
        .expression_attribute_names("#ranked", "ranked")
        .expression_attribute_values(
            ":ranked",
            aws_sdk_dynamodb::types::AttributeValue::Bool(ranked),
        )
        .send()
        .await
        .expect("set ranked");
}

/// The end-to-end happy path: a ranked match both sides confirm moves both
/// ratings, writes each participant a history entry, and records what the
/// match contributed as a `RATINGCONTRIB#` item.
///
/// The three writes are one transaction in the DAO precisely so they cannot
/// disagree — a movement with no contribution recording it would be applied
/// again on the next redelivery — so this asserts all three rather than just
/// the headline number.
#[tokio::test]
async fn a_confirmed_ranked_match_moves_both_ratings_and_writes_history() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let starts_at = iso_offset_hours(-3);
    let created =
        play_ranked_tennis_match(&winner_config, &loser_config, &loser.profile.id, &starts_at)
            .await;

    let winner_rating = rating_after_matches(&dao, &winner.profile.id, "tennis", 1).await;
    let loser_rating = rating_after_matches(&dao, &loser.profile.id, "tennis", 1).await;

    // Everyone starts at the engine's native mu 25 with sigma 25/3. Winning
    // moves mu up, losing moves it down, and a result of any kind shrinks the
    // uncertainty for both.
    assert!(
        winner_rating.mu > 25.0,
        "the winner's rating should rise, got {}",
        winner_rating.mu
    );
    assert!(
        loser_rating.mu < 25.0,
        "the loser's rating should fall, got {}",
        loser_rating.mu
    );
    for (whose, rating) in [("winner", &winner_rating), ("loser", &loser_rating)] {
        assert!(
            rating.sigma < 25.0 / 3.0,
            "{whose}'s uncertainty should shrink, got {}",
            rating.sigma
        );
        assert_eq!(
            rating.last_rated_at, starts_at,
            "{whose}'s newest rated match is the one just played"
        );
    }

    // A zero-sum-ish sanity check on the pair: the loser lost roughly what the
    // winner gained. Exactly equal for two identically-rated players, so this
    // would catch a sign or scale error in either direction.
    assert!(
        ((winner_rating.mu - 25.0) - (25.0 - loser_rating.mu)).abs() < 1e-9,
        "evenly-matched players should move symmetrically: {} vs {}",
        winner_rating.mu,
        loser_rating.mu
    );

    // The per-match record of what was applied, which is what makes a
    // redelivery a no-op and what the UI will read to show "+18".
    let contributions = contributions_by_owner(&dao, &created.id).await;
    assert_eq!(contributions.len(), 2, "one contribution per participant");
    let winner_side = side_id_for_user(&created, &winner.profile.id);
    let loser_side = side_id_for_user(&created, &loser.profile.id);
    let winner_contribution = &contributions[&winner.profile.id];
    let loser_contribution = &contributions[&loser.profile.id];
    assert_eq!(winner_contribution.ladder, "tennis");
    assert_eq!(winner_contribution.side_id, winner_side);
    assert_eq!(loser_contribution.side_id, loser_side);
    assert_eq!(winner_contribution.played_at, starts_at);
    assert_eq!(
        winner_contribution.owner_kind,
        agon_core::dao::records::RatingOwnerKindRecord::User
    );
    // The contribution's `after` is the stored rating: the two are written in
    // the same transaction and must agree, or a repair replaying history would
    // land somewhere the profile doesn't.
    assert_eq!(winner_contribution.movement.mu_after, winner_rating.mu);
    assert_eq!(
        winner_contribution.movement.sigma_after,
        winner_rating.sigma
    );
    assert_eq!(winner_contribution.movement.mu_before, 25.0);
    assert!(
        winner_contribution.movement.display_delta > 0,
        "the winner's shown delta should be positive, got {}",
        winner_contribution.movement.display_delta
    );
    assert!(
        loser_contribution.movement.display_delta < 0,
        "the loser's shown delta should be negative, got {}",
        loser_contribution.movement.display_delta
    );

    // The history entry — the replay source for repair, and the
    // rating-over-time chart, from the same write.
    let history = rating_history(&dao, &winner.profile.id, "tennis").await;
    assert_eq!(history.len(), 1, "one history entry per rated match");
    assert_eq!(history[0].match_id, created.id);
    assert_eq!(history[0].played_at, starts_at);
    assert_eq!(history[0].movement, winner_contribution.movement);
}

/// A friendly match moves nothing. `ranked` has no API surface until phase 3
/// (`create_match` hard-codes it true), so the flag is flipped directly on the
/// stored record before the score is confirmed — which is also the only shape
/// this can take until then.
///
/// The barrier for asserting a negative is the *stats* handler: it runs
/// immediately before the rating handler on the same event in
/// `handlers::route`, so once the match has been counted as played, the rating
/// handler has demonstrably seen it and decided not to rate it. Sleeping
/// instead would make this a test of how fast the worker happens to be.
#[tokio::test]
async fn a_friendly_match_is_not_rated() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let created = matches_post(
        &winner_config,
        completed_match_at(
            &iso_offset_hours(-3),
            vec![invite_users("b", &[&loser.profile.id])],
        ),
    )
    .await
    .expect("create match");
    set_match_ranked(&created.id, false).await;
    accept_match_invitation(&loser_config, &created.id).await;
    confirm_pending_score(&loser_config, &created).await;

    assert_matches_played_reaches(&loser_config, models::MatchType::Tennis, 1, "loser").await;

    assert_eq!(
        stored_rating(&dao, &winner.profile.id, "tennis").await,
        None,
        "a friendly must not rate the winner"
    );
    assert_eq!(
        stored_rating(&dao, &loser.profile.id, "tennis").await,
        None,
        "a friendly must not rate the loser"
    );
    assert!(
        contributions_by_owner(&dao, &created.id).await.is_empty(),
        "a friendly must not record a rating contribution"
    );
}

/// One unlinked guest on a side leaves the whole match unrated — not just the
/// guest's own row.
///
/// The rule looks harsh until you look at the maths: Weng-Lin treats a side's
/// strength as the sum of its players' beliefs, so silently rating "everyone
/// we happen to have an account for" would model a 2-player side as a
/// 1-player one and credit the guest's contribution to their partner. There is
/// nothing to rate a guest *with* and nothing to give them, so the match
/// waits until the guest claims their invite (which is a roster change, and
/// therefore a repair).
#[tokio::test]
async fn a_match_with_an_unlinked_guest_is_not_rated() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let created = matches_post(
        &winner_config,
        completed_match_at(
            &iso_offset_hours(-3),
            vec![
                invite_users("b", &[&loser.profile.id]),
                invite_externals("b", &["Ringer Rita"]),
            ],
        ),
    )
    .await
    .expect("create match");
    accept_match_invitation(&loser_config, &created.id).await;
    confirm_pending_score(&loser_config, &created).await;

    // Same barrier as the friendly test: stats prove the event was processed.
    assert_matches_played_reaches(&loser_config, models::MatchType::Tennis, 1, "loser").await;

    assert_eq!(
        stored_rating(&dao, &winner.profile.id, "tennis").await,
        None,
        "a match with an unlinked guest must rate nobody"
    );
    assert_eq!(
        stored_rating(&dao, &loser.profile.id, "tennis").await,
        None,
        "a match with an unlinked guest must rate nobody"
    );
    assert!(
        contributions_by_owner(&dao, &created.id).await.is_empty(),
        "nothing should have been applied to back out later"
    );
}

/// Redelivering an already-rated match applies nothing a second time.
///
/// This is the property the whole design rests on and the easiest one to get
/// wrong: a match's `#META` item is rewritten by every like and every comment,
/// so the rating handler re-runs on finished matches constantly, and SQS is
/// at-least-once on top of that. The naive implementation — re-rate against
/// the account's *current* rating and write if it differs — double-counts
/// every one of those.
///
/// The like here is a real redelivery trigger (it rewrites `#META`), and the
/// second match is both the observable barrier proving the queue drained past
/// it and the check that the chain is intact: match two's `mu_before` must be
/// exactly match one's `mu_after`, which cannot hold if match one was applied
/// twice in between.
#[tokio::test]
async fn redelivering_a_rated_match_does_not_apply_it_twice() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let first_starts_at = iso_offset_hours(-5);
    let first = play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &first_starts_at,
    )
    .await;
    let after_first = rating_after_matches(&dao, &winner.profile.id, "tennis", 1).await;
    let first_contributions = contributions_by_owner(&dao, &first.id).await;

    // A like rewrites the match's `#META`, which is exactly the handler's
    // trigger — the redelivery, arranged through the API rather than simulated.
    matches_match_id_likes_post(&loser_config, &first.id)
        .await
        .expect("like the match");

    // A second, later match on the same ladder: the barrier, and the thing
    // that exercises the optimistic lock's map-typed condition
    // (`ratings.tennis = :expected`), which only runs on a *second* rating.
    let second_starts_at = iso_offset_hours(-2);
    let second = play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &second_starts_at,
    )
    .await;
    let after_second = rating_after_matches(&dao, &winner.profile.id, "tennis", 2).await;

    // Two matches played, two matches rated — not three.
    assert_eq!(after_second.matches_rated, 2);
    assert_eq!(after_second.last_rated_at, second_starts_at);

    // The first match's contribution is untouched, `applied_at` included — a
    // rewrite would have refreshed that wall clock even if the numbers
    // happened to land in the same place.
    assert_eq!(
        contributions_by_owner(&dao, &first.id).await,
        first_contributions,
        "a redelivered match must not be rewritten"
    );

    // The chain: the second match started from exactly where the first
    // finished. Bit-exact, because the stored value is fed straight back into
    // the engine — anything less than exact here means `mu`/`sigma` are not
    // surviving the round trip through DynamoDB's `N` type, which would make
    // every redelivery look like a re-score.
    let second_contributions = contributions_by_owner(&dao, &second.id).await;
    let second_winner = &second_contributions[&winner.profile.id];
    assert_eq!(second_winner.movement.mu_before, after_first.mu);
    assert_eq!(second_winner.movement.sigma_before, after_first.sigma);
    assert_eq!(second_winner.movement.mu_after, after_second.mu);

    // Both matches are in the history exactly once each, in played order.
    let history = rating_history(&dao, &winner.profile.id, "tennis").await;
    let match_ids: Vec<&str> = history.iter().map(|h| h.match_id.as_str()).collect();
    assert_eq!(match_ids, vec![first.id.as_str(), second.id.as_str()]);
}

/// A match played earlier but confirmed later is still rated, and does not
/// drag `last_rated_at` backwards.
///
/// Confirmation order is the order the stream delivers results in; played
/// order is the order they are supposed to be rated in, and the two disagree
/// routinely (a Monday game confirmed on Thursday). Phase 2b-i detects the
/// disagreement and applies the result anyway — the alternative, refusing to
/// rate it, would leave the match with no history entry, which is precisely
/// what a replay reads, so it would be invisible to the repair meant to fix
/// it. What must hold in the meantime is that the *history* is in played
/// order, since that is what makes the eventual replay produce the right
/// answer, and that `last_rated_at` still names the newest match played.
#[tokio::test]
async fn a_match_confirmed_out_of_order_is_still_rated_and_lands_in_played_order() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    // Played on Wednesday, confirmed first.
    let recent_starts_at = iso_offset_hours(-2);
    let recent = play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &recent_starts_at,
    )
    .await;
    rating_after_matches(&dao, &winner.profile.id, "tennis", 1).await;

    // Played on Monday, confirmed second.
    let older_starts_at = iso_offset_hours(-30);
    let older = play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &older_starts_at,
    )
    .await;
    let rating = rating_after_matches(&dao, &winner.profile.id, "tennis", 2).await;

    assert_eq!(
        rating.last_rated_at, recent_starts_at,
        "the newest match *played* stays the newest, whatever order they were confirmed in"
    );

    // The history collection is keyed on played time, so it is already in the
    // order a replay needs even though the second match arrived last.
    let history = rating_history(&dao, &winner.profile.id, "tennis").await;
    let match_ids: Vec<&str> = history.iter().map(|h| h.match_id.as_str()).collect();
    assert_eq!(
        match_ids,
        vec![older.id.as_str(), recent.id.as_str()],
        "history is ordered by when matches were played, not when they were rated"
    );

    // Both matches were applied, neither was dropped.
    assert_eq!(contributions_by_owner(&dao, &older.id).await.len(), 2);
    assert_eq!(contributions_by_owner(&dao, &recent.id).await.len(), 2);
}

// ---------------------------------------------------------------------------
// Ratings — repair (phase 2b-ii)
// ---------------------------------------------------------------------------
//
// Where the tests above assert that a change is *detected*, these assert what
// `RepairRatings` does about it. The bar is the same one
// `rating::engine::tests::replaying_from_scratch_matches_incremental_rating`
// sets for the pure engine, carried through the whole pipeline: **the repaired
// state must equal the state a from-scratch replay produces** — bit for bit,
// not approximately, because any drift would compound on every correction.
//
// The expectation is computed here through the engine itself rather than
// hard-coded, which is the only way to state that property. That does mean a
// bug shared between the engine and the worker would pass; the engine's own
// known-vector tests are what guard against that, and duplicating Weng-Lin's
// arithmetic in a test would guard nothing while rotting immediately.
//
// **What these deliberately do not assert: global consistency.** Repair is
// first-order (see `temporal::workflows::RepairRatings`) — it corrects the
// owner it replays and holds everybody else at the belief their own
// contribution recorded. Each scenario below is therefore built so that the
// held-fixed beliefs happen to be the true ones for the owner under test
// (their opponents are playing their first rated match), which is what makes
// "equals a from-scratch replay" a legitimate assertion rather than a lucky
// one. Where a third party is left knowingly stale, the test says so.

/// Rate a sequence of matches from scratch in played order, through the same
/// engine the worker uses. Each entry is `(participants, winner_side_id)`,
/// with participants as `(user_id, side_id)`.
///
/// Real side ids, not `"a"`/`"b"`: `rate_sides` orders sides by id before doing
/// any floating-point arithmetic, so a different id ordering moves the result
/// in the last bits — and these comparisons are exact.
fn replayed_from_scratch(
    matches: &[(Vec<(String, String)>, Option<String>)],
) -> agon_core::rating::RatingTable {
    use agon_core::rating::{MatchParticipant, RatingTable, apply, rate_match};

    let mut ratings = RatingTable::new();
    for (participants, winner_side_id) in matches {
        let participants: Vec<MatchParticipant> = participants
            .iter()
            .map(|(competitor_id, side_id)| MatchParticipant {
                competitor_id: competitor_id.clone(),
                side_id: side_id.clone(),
            })
            .collect();
        let updates = rate_match(&participants, winner_side_id.as_deref(), &ratings)
            .expect("the replayed match is rateable");
        apply(&updates, &mut ratings);
    }
    ratings
}

/// One match in the shape [`replayed_from_scratch`] wants, given the two
/// users' side ids and who won.
fn replayable(
    winner: (&str, &str),
    loser: (&str, &str),
    winner_won: bool,
) -> (Vec<(String, String)>, Option<String>) {
    let participants = vec![
        (winner.0.to_string(), winner.1.to_string()),
        (loser.0.to_string(), loser.1.to_string()),
    ];
    let winning_side = if winner_won { winner.1 } else { loser.1 };
    (participants, Some(winning_side.to_string()))
}

/// Poll until `user_id`'s stored rating is exactly the one a from-scratch
/// replay produces, and return it.
///
/// Not [`eventually`], for two reasons. The comparison is bit-exact on `f64`
/// by design — the whole repair story rests on a replay reproducing the
/// incremental numbers precisely — so a near miss is a real failure and the
/// message has to show both numbers to be diagnosable at all. And the
/// pre-repair value is itself a perfectly plausible rating, so there is no
/// coarser "has it happened yet" signal to wait on first.
async fn rating_converges_on(
    dao: &agon_core::dao::Dao,
    user_id: &str,
    ladder: &str,
    expected: agon_core::rating::PlayerRating,
    matches_rated: u64,
) -> agon_core::dao::records::RatingRecord {
    const ATTEMPTS: u32 = 25;
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        let current = stored_rating(dao, user_id, ladder).await;
        if let Some(rating) = &current
            && rating.matches_rated == matches_rated
            && rating.mu == expected.mu
            && rating.sigma == expected.sigma
        {
            return rating.clone();
        }
        last = current;
        if attempt < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    }
    panic!(
        "timed out after {ATTEMPTS}s waiting for {user_id}'s {ladder} rating to be replayed onto \
         mu={} sigma={} over {matches_rated} match(es); last saw {last:?}",
        expected.mu, expected.sigma
    );
}

/// A match confirmed after a later one is applied out of order, then repaired
/// back onto the ratings played order would have produced.
///
/// This is the trigger phase 2b-i could only log. The incremental result is
/// self-consistent but wrong: the Monday match was rated from the belief the
/// player carried *out* of Wednesday's, because Wednesday's was confirmed
/// first. `RepairRatings` replays the ladder from the start and lands on the
/// numbers a from-scratch run produces.
///
/// The two opponents are different accounts, each playing their first rated
/// match, and that is not incidental. Only the account with an existing
/// rating is flagged out of order, and holding an opponent at the belief
/// their contribution recorded is exactly right when that belief is the
/// starting default — so first-order repair coincides with full consistency
/// here, and the assertion can be the strong one. Both opponents are left
/// carrying the second-order error the design documents: they each played a
/// player whose rating has since been corrected underneath them, and nothing
/// goes back for them.
#[tokio::test]
async fn a_match_confirmed_out_of_order_is_replayed_into_played_order() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (hero_config, hero) = new_user().await;
    let (recent_foe_config, recent_foe) = new_user().await;
    let (older_foe_config, older_foe) = new_user().await;

    // Wednesday, confirmed first — so it is rated first, from nothing.
    let recent_starts_at = iso_offset_hours(-2);
    let recent = play_ranked_tennis_match(
        &hero_config,
        &recent_foe_config,
        &recent_foe.profile.id,
        &recent_starts_at,
    )
    .await;
    rating_after_matches(&dao, &hero.profile.id, "tennis", 1).await;

    // Monday, confirmed second — out of played order, which is what fires the
    // repair.
    let older_starts_at = iso_offset_hours(-30);
    let older = play_ranked_tennis_match(
        &hero_config,
        &older_foe_config,
        &older_foe.profile.id,
        &older_starts_at,
    )
    .await;

    let replayed = replayed_from_scratch(&[
        replayable(
            (
                &hero.profile.id,
                &side_id_for_user(&older, &hero.profile.id),
            ),
            (
                &older_foe.profile.id,
                &side_id_for_user(&older, &older_foe.profile.id),
            ),
            true,
        ),
        replayable(
            (
                &hero.profile.id,
                &side_id_for_user(&recent, &hero.profile.id),
            ),
            (
                &recent_foe.profile.id,
                &side_id_for_user(&recent, &recent_foe.profile.id),
            ),
            true,
        ),
    ]);

    let repaired = rating_converges_on(
        &dao,
        &hero.profile.id,
        "tennis",
        replayed[&hero.profile.id],
        2,
    )
    .await;
    assert_eq!(
        repaired.last_rated_at, recent_starts_at,
        "the newest match played is still the newest after a replay"
    );

    // The replay rewrote the Monday match's own record of what it did: it was
    // originally applied on top of Wednesday's result, and now starts from an
    // unrated player, as played order says it should. Without this the
    // contribution would still describe the movement that was actually
    // applied, and the next redelivery would read it as a re-score.
    let older_contributions = contributions_by_owner(&dao, &older.id).await;
    assert_eq!(
        older_contributions[&hero.profile.id].movement.mu_before,
        agon_core::rating::INITIAL_MU,
        "the earliest match must be replayed from an unrated player"
    );

    // History stays keyed on played time throughout — it is the replay's own
    // input, so a repair that reordered it would be unable to run twice.
    let history = rating_history(&dao, &hero.profile.id, "tennis").await;
    let match_ids: Vec<&str> = history.iter().map(|h| h.match_id.as_str()).collect();
    assert_eq!(match_ids, vec![older.id.as_str(), recent.id.as_str()]);
    assert_eq!(
        history[1].movement.mu_before, history[0].movement.mu_after,
        "each replayed match must start from the one before it"
    );
}

/// Re-scoring a rated match replays it, and both sides land on the ratings the
/// corrected result implies.
///
/// A Weng-Lin update has no inverse, so this cannot be applied as a delta —
/// the old movement is unwound only by computing the whole thing again. Phase
/// 2b-i detected the change and deliberately left the stored contributions
/// alone because they are the replay's input; this is the other half of that
/// bargain.
///
/// Both accounts can be asserted exactly here, and symmetrically, because it
/// is each of their first rated matches: the belief each holds the other at is
/// the starting default either way round, so it does not matter which of the
/// two repairs runs first.
#[tokio::test]
async fn re_scoring_a_rated_match_replays_both_sides_onto_the_corrected_result() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let starts_at = iso_offset_hours(-6);
    let created =
        play_ranked_tennis_match(&winner_config, &loser_config, &loser.profile.id, &starts_at)
            .await;
    let originally = rating_after_matches(&dao, &winner.profile.id, "tennis", 1).await;
    assert!(
        originally.mu > agon_core::rating::INITIAL_MU,
        "the recorded winner should have gone up first"
    );

    // Correct the result the other way round: the loser actually won.
    let side_a = side_id_for_user(&created, &winner.profile.id);
    let side_b = side_id_for_user(&created, &loser.profile.id);
    let rescored = matches_match_id_patch(
        &winner_config,
        &created.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 3, 6))),
            winner_side_id: Some(side_b.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("re-score the match");
    confirm_pending_score(&loser_config, &rescored).await;

    let replayed = replayed_from_scratch(&[replayable(
        (&winner.profile.id, &side_a),
        (&loser.profile.id, &side_b),
        false,
    )]);

    let corrected = rating_converges_on(
        &dao,
        &winner.profile.id,
        "tennis",
        replayed[&winner.profile.id],
        1,
    )
    .await;
    rating_converges_on(
        &dao,
        &loser.profile.id,
        "tennis",
        replayed[&loser.profile.id],
        1,
    )
    .await;
    assert!(
        corrected.mu < agon_core::rating::INITIAL_MU,
        "the account that actually lost must end up below the starting rating, got {}",
        corrected.mu
    );

    // The contribution and the history entry describe the corrected match, not
    // the original one — including the "+18" the player was shown, which flips
    // sign with the result.
    let contributions = contributions_by_owner(&dao, &created.id).await;
    assert_eq!(
        contributions[&winner.profile.id].movement.mu_after, corrected.mu,
        "the contribution and the stored rating are written in one transaction"
    );
    let history = rating_history(&dao, &winner.profile.id, "tennis").await;
    assert_eq!(history.len(), 1, "a replay rewrites history, never appends");
    assert!(
        history[0].movement.display_delta < 0,
        "the shown movement follows the corrected result"
    );
}

/// A match that stops counting is replayed out of its participants' ratings —
/// including all the way back to unrated when it was the only one.
///
/// Phase 2b-i removes the contribution and the history entry immediately (they
/// are the replay's input, so leaving them would have a replay faithfully
/// re-apply a match that no longer counts) and leaves the stored rating stale,
/// because subtracting a Weng-Lin update is not a thing that can be computed.
/// This is what finishes the job.
///
/// The two halves are worth asserting separately: the account with a match
/// left keeps a rating replayed from what remains, and the account with none
/// left goes back to the unrated default — which is the only path in the whole
/// system that *lowers* `matches_rated`, and the reason the replay has a
/// separate settling step at all (an empty history writes nothing per-match
/// and would otherwise leave the stale value untouched).
#[tokio::test]
async fn withdrawing_a_match_replays_the_owners_remaining_history() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (hero_config, hero) = new_user().await;
    let (withdrawn_foe_config, withdrawn_foe) = new_user().await;
    let (kept_foe_config, kept_foe) = new_user().await;

    // Both in played order, so nothing is out of order and the only change is
    // the withdrawal.
    let withdrawn = play_ranked_tennis_match(
        &hero_config,
        &withdrawn_foe_config,
        &withdrawn_foe.profile.id,
        &iso_offset_hours(-30),
    )
    .await;
    rating_after_matches(&dao, &hero.profile.id, "tennis", 1).await;

    let kept_starts_at = iso_offset_hours(-2);
    let kept = play_ranked_tennis_match(
        &hero_config,
        &kept_foe_config,
        &kept_foe.profile.id,
        &kept_starts_at,
    )
    .await;
    rating_after_matches(&dao, &hero.profile.id, "tennis", 2).await;

    // Demoting it to a friendly rewrites `#META`, which is the handler's own
    // trigger — no separate nudge needed. It stands in for every way a match
    // can stop counting (cancellation, a roster edit that unlinks a player).
    set_match_ranked(&withdrawn.id, false).await;

    let replayed = replayed_from_scratch(&[replayable(
        (&hero.profile.id, &side_id_for_user(&kept, &hero.profile.id)),
        (
            &kept_foe.profile.id,
            &side_id_for_user(&kept, &kept_foe.profile.id),
        ),
        true,
    )]);

    let repaired = rating_converges_on(
        &dao,
        &hero.profile.id,
        "tennis",
        replayed[&hero.profile.id],
        1,
    )
    .await;
    assert_eq!(
        repaired.last_rated_at, kept_starts_at,
        "the withdrawn match no longer counts as the newest played"
    );

    assert!(
        contributions_by_owner(&dao, &withdrawn.id).await.is_empty(),
        "the withdrawn match contributes to nobody"
    );
    let history = rating_history(&dao, &hero.profile.id, "tennis").await;
    assert_eq!(
        history
            .iter()
            .map(|h| h.match_id.as_str())
            .collect::<Vec<_>>(),
        vec![kept.id.as_str()],
        "only the surviving match is left for the next replay to read"
    );

    // The opponent's only match is gone, so their replay covers no matches at
    // all and has to put them back where they started — not leave them holding
    // a rating earned in a game that no longer counts.
    let reset = rating_converges_on(
        &dao,
        &withdrawn_foe.profile.id,
        "tennis",
        agon_core::rating::PlayerRating::default(),
        0,
    )
    .await;
    assert_eq!(
        reset.last_rated_at, "",
        "an emptied ladder must not keep claiming a newest match played"
    );
    assert!(
        rating_history(&dao, &withdrawn_foe.profile.id, "tennis")
            .await
            .is_empty()
    );
}

/// A player added to an already-rated match ends up rated for it, end to end.
///
/// The roster edit rewrites `#META` (side roster previews are cached on the
/// match record), which re-runs the rating handler with one delivery carrying
/// two different jobs: the two players already there have *moved*, because the
/// side they face just gained a player, so they need a replay — while the
/// added player has no contribution at all and needs a first apply. Handling
/// only the first strands the second, and `RepairRatings` cannot cover for it,
/// because it replays *stored history* and so can only correct a contribution
/// that already exists.
///
/// **This is not the regression test for that**, and the distinction is worth
/// stating rather than assuming. It passes against the buggy handler too: a
/// single `PATCH { added_players }` produces *two* `#META` writes (the
/// metadata update, then `refresh_side_roster_previews`), and if the repairs
/// the first one starts happen to land before the second delivery arrives, the
/// second finds the existing contributions already corrected, classifies
/// nothing as re-rated, and applies the added player after all. That masking
/// is a race, not a guarantee. The deterministic reproduction is the unit test
/// `handlers::rating::tests::a_participant_added_to_a_rated_match_is_applied_even_when_the_others_need_a_replay`,
/// which drives one delivery. This is the end-to-end assertion that the
/// pipeline really does reach the intended state.
#[tokio::test]
async fn a_player_added_to_a_rated_match_is_rated_for_it() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;
    let (_latecomer_config, latecomer) = new_user().await;

    let starts_at = iso_offset_hours(-4);
    let created =
        play_ranked_tennis_match(&winner_config, &loser_config, &loser.profile.id, &starts_at)
            .await;
    rating_after_matches(&dao, &winner.profile.id, "tennis", 1).await;

    // Somebody who actually played turns up on the roster afterwards, on the
    // losing side. Added ad hoc rather than invited, so they count as having
    // played under the same rule `reconcile_match_stats` uses (no embedded
    // invitation) without a second round trip to accept one.
    let side_b = side_id_for_user(&created, &loser.profile.id);
    matches_match_id_patch(
        &winner_config,
        &created.id,
        models::UpdateMatchInput {
            added_players: Some(vec![models::AddMatchPlayerInput {
                user_id: Some(latecomer.profile.id.clone()),
                display_name: None,
                side_id: Some(side_b.clone()),
            }]),
            ..Default::default()
        },
    )
    .await
    .expect("add a player to the completed match");

    let rating = rating_after_matches(&dao, &latecomer.profile.id, "tennis", 1).await;
    assert_eq!(
        rating.last_rated_at, starts_at,
        "the added player is rated as of when the match was played"
    );

    // ...and the contribution recording it exists, which is what makes the
    // apply idempotent and what a later repair of theirs would replay.
    let contributions = contributions_by_owner(&dao, &created.id).await;
    assert_eq!(
        contributions.len(),
        3,
        "every participant on the corrected roster contributes: {:?}",
        contributions.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        contributions[&latecomer.profile.id].side_id, side_b,
        "the contribution records the side they were added to"
    );
    assert_eq!(
        rating_history(&dao, &latecomer.profile.id, "tennis")
            .await
            .iter()
            .map(|h| h.match_id.as_str())
            .collect::<Vec<_>>(),
        vec![created.id.as_str()],
        "and the history entry a repair of theirs would read"
    );
}

/// Rescheduling a rated match replays it at its new played time, and counts it
/// exactly once.
///
/// Moving `starts_at` moves the sort key of the match's history item, so the
/// replay writes the item at the new key and deletes the old one in the same
/// transaction. This asserts the two properties that fall out of getting that
/// right end to end: the ladder still holds one entry per match, at the new
/// time, and `matches_rated` has not grown.
///
/// It does **not** reproduce the double-fold that
/// `handlers::rating::tests::a_match_rescheduled_mid_replay_is_folded_once_not_twice`
/// covers. That one needs the moved key to land beyond a *page* boundary,
/// which takes more than `REPLAY_PAGE` (50) rated matches on one ladder — far
/// past what is reasonable to play through an HTTP API here. This is the
/// end-to-end smoke test for the same code path; the unit test is the
/// regression.
#[tokio::test]
async fn rescheduling_a_rated_match_replays_it_at_its_new_played_time() {
    let Some(dao) = rating_dao().await else {
        return;
    };
    let (hero_config, hero) = new_user().await;
    let (older_foe_config, older_foe) = new_user().await;
    let (recent_foe_config, recent_foe) = new_user().await;

    // In played order, so nothing is out of order and the reschedule is the
    // only change the repair has to account for.
    let older = play_ranked_tennis_match(
        &hero_config,
        &older_foe_config,
        &older_foe.profile.id,
        &iso_offset_hours(-30),
    )
    .await;
    rating_after_matches(&dao, &hero.profile.id, "tennis", 1).await;
    let recent = play_ranked_tennis_match(
        &hero_config,
        &recent_foe_config,
        &recent_foe.profile.id,
        &iso_offset_hours(-6),
    )
    .await;
    rating_after_matches(&dao, &hero.profile.id, "tennis", 2).await;

    // The organiser corrects the older match's date to *after* the newer one.
    // `starts_at` is freely editable on a completed match, and the `#META`
    // write it produces is what the handler sees as a re-rating.
    let corrected_starts_at = iso_offset_hours(-1);
    matches_match_id_patch(
        &hero_config,
        &older.id,
        models::UpdateMatchInput {
            starts_at: Some(corrected_starts_at.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("correct the match's date");

    // The history item moving to its new key is the observable signal that the
    // replay has run — the rating count, which is what is actually at stake,
    // is unchanged either way and so cannot be waited on.
    let history = eventually(
        "the rescheduled match's history entry to move to its new played time",
        || async {
            let history = rating_history(&dao, &hero.profile.id, "tennis").await;
            history
                .iter()
                .any(|h| h.match_id == older.id && h.played_at == corrected_starts_at)
                .then_some(history)
        },
    )
    .await;

    assert_eq!(
        history
            .iter()
            .map(|h| h.match_id.as_str())
            .collect::<Vec<_>>(),
        vec![recent.id.as_str(), older.id.as_str()],
        "one entry per match, now in the corrected played order"
    );
    let rated = stored_rating(&dao, &hero.profile.id, "tennis")
        .await
        .expect("a rating");
    assert_eq!(
        rated.matches_rated, 2,
        "a rescheduled match must be folded once, not twice"
    );
    assert_eq!(
        rated.last_rated_at, corrected_starts_at,
        "the rescheduled match is now the newest played"
    );
}

// ---------------------------------------------------------------------------
// Ratings — the API surface (phase 3a)
// ---------------------------------------------------------------------------
//
// Unlike the 2b tests above, everything here goes through the API alone: no
// `rating_dao()`, no table reads, so these also run in the staging CI job that
// only has a service URL. That is the point of the phase — a rating that only
// the table can see is not a surface.
//
// They still depend on the same async pipeline (API → stream → SQS → worker),
// so the first read of a freshly-earned rating polls; everything after it is a
// synchronous read of state that has already landed.

/// Poll `GET /users/:id` as `viewer` until the profile carries a rating on
/// `ladder` folded from exactly `matches_rated` matches.
///
/// Keyed on the count rather than on the number for the same reason
/// [`rating_after_matches`] is: it is the one field that is exactly
/// predictable, and it is what distinguishes "the second match landed" from
/// "the first one is still all there is".
async fn profile_rating_after_matches(
    viewer: &Configuration,
    user_id: &str,
    ladder: &str,
    matches_rated: i32,
) -> models::LadderRating {
    eventually(
        &format!("{user_id}'s {ladder} rating on their profile, from {matches_rated} match(es)"),
        || async {
            users_user_id_get(viewer, user_id)
                .await
                .expect("get profile")
                .ratings
                .into_iter()
                .find(|r| r.ladder == ladder && r.matches_rated == matches_rated)
        },
    )
    .await
}

/// Opt an account's ratings in or out of being publicly visible.
async fn set_rating_visibility(config: &Configuration, visibility: models::RatingVisibility) {
    users_me_patch(
        config,
        models::UpdateUserInput {
            name: None,
            profile_image_asset_id: None,
            rating_visibility: Some(visibility),
        },
    )
    .await
    .expect("set rating visibility");
}

/// The band a placement carries, or `None` while the account is unplaced.
fn band_of(rating: &models::LadderRating) -> Option<models::RatingBand> {
    match &*rating.placement {
        models::RatingPlacement::Placed(placed) => Some(placed.band),
        models::RatingPlacement::Unrated(_) => None,
    }
}

/// How many more rated matches this account needs before it is placed, or
/// `None` once it is.
fn matches_remaining(rating: &models::LadderRating) -> Option<i32> {
    match &*rating.placement {
        models::RatingPlacement::Unrated(unrated) => Some(unrated.matches_remaining),
        models::RatingPlacement::Placed(_) => None,
    }
}

/// Play `count` ranked tennis matches between the same two accounts, each an
/// hour after the last, waiting for each to be folded into `winner`'s rating
/// before starting the next.
///
/// Sequential on purpose, twice over: rating a ladder is one optimistic-locked
/// write, so overlapping confirmations would race it, and confirming out of
/// played order is the out-of-order case that starts a repair — a real
/// behaviour with its own tests (`2b-ii`), and not what anything here is
/// about.
async fn play_ranked_series(
    winner_config: &Configuration,
    winner_id: &str,
    loser_config: &Configuration,
    loser_id: &str,
    count: i32,
) -> Vec<models::Match> {
    let mut played = Vec::new();
    for index in 0..count {
        let starts_at = iso_offset_hours(-24 + i64::from(index));
        played.push(
            play_ranked_tennis_match(winner_config, loser_config, loser_id, &starts_at).await,
        );
        profile_rating_after_matches(winner_config, winner_id, "tennis", index + 1).await;
    }
    played
}

/// The headline of the visibility design: opting in unlocks *information*,
/// never access. A private account is rated all along — the owner can see the
/// number the whole time — and opting in publishes the rating that was already
/// there rather than starting one.
///
/// Also pins that hiding covers the number **and** the band together. A band
/// is a 150-point window on the number it would be concealing, so leaking one
/// while hiding the other would make the setting decorative.
#[tokio::test]
async fn a_rating_is_shown_to_others_only_once_its_owner_opts_in() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;
    let (viewer_config, _viewer) = new_user().await;

    play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &iso_offset_hours(-3),
    )
    .await;

    // The owner sees their own while it is still Private, which is the
    // default nobody opted into.
    let own = profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 1).await;
    assert!(
        own.rating > 1500,
        "the winner's rating should have risen, got {}",
        own.rating
    );
    assert_eq!(own.confidence, own.rating - own.floor, "the three must agree");
    assert_eq!(
        matches_remaining(&own),
        Some(4),
        "one game earns a number, not a band"
    );

    // A third party sees neither the number nor the band — but is told the
    // account is private, so it can render that rather than "never played".
    let hidden = users_user_id_get(&viewer_config, &winner.profile.id)
        .await
        .expect("get profile as a stranger");
    assert!(
        hidden.ratings.is_empty(),
        "a private rating must not reach another account"
    );
    assert_eq!(hidden.rating_visibility, models::RatingVisibility::Private);

    // Opting in publishes exactly the rating that was already there.
    set_rating_visibility(&winner_config, models::RatingVisibility::Public).await;
    let shown = users_user_id_get(&viewer_config, &winner.profile.id)
        .await
        .expect("get profile as a stranger");
    assert_eq!(shown.rating_visibility, models::RatingVisibility::Public);
    let shown = shown
        .ratings
        .into_iter()
        .find(|r| r.ladder == "tennis")
        .expect("the opted-in rating is visible");
    assert_eq!(shown.rating, own.rating, "opting in must not restate it");
    assert_eq!(shown.confidence, own.confidence);
    assert_eq!(shown.matches_rated, 1);

    // And it can be taken back.
    set_rating_visibility(&winner_config, models::RatingVisibility::Private).await;
    assert!(
        users_user_id_get(&viewer_config, &winner.profile.id)
            .await
            .expect("get profile as a stranger")
            .ratings
            .is_empty(),
        "opting back out must hide it again"
    );
}

/// `/users/me` is the owner's own view, so it carries their ratings whatever
/// their visibility setting says — that is what makes the opt-in toggle
/// renderable from the profile the client already has.
#[tokio::test]
async fn the_owner_sees_their_own_private_rating_on_users_me() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &iso_offset_hours(-3),
    )
    .await;
    profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 1).await;

    let me = users_me_get(&winner_config).await.expect("get me");
    assert_eq!(
        me.profile.rating_visibility,
        models::RatingVisibility::Private,
        "still opted out"
    );
    assert_eq!(
        me.profile
            .ratings
            .iter()
            .filter(|r| r.ladder == "tennis")
            .count(),
        1,
        "the owner sees their own rating regardless"
    );
}

/// The friendly opt-out, end to end: a match created with `ranked: false` is
/// confirmed like any other and contributes nothing to anybody's ladder.
///
/// Proved against a *subsequent* ranked match rather than by waiting for
/// nothing to happen: `matches_rated == 1` after playing two games is a
/// positive assertion that exactly one of them counted, where a bare "no
/// rating yet" would also pass if the pipeline were simply slow.
#[tokio::test]
async fn a_match_created_as_friendly_never_reaches_the_ladder() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let friendly = matches_post(
        &winner_config,
        models::CreateMatchInput {
            ranked: Some(false),
            ..completed_match_at(
                &iso_offset_hours(-5),
                vec![invite_users("b", &[&loser.profile.id])],
            )
        },
    )
    .await
    .expect("create friendly match");
    assert!(!friendly.ranked, "the create input must be honoured");
    assert!(
        !matches_match_id_get(&winner_config, &friendly.id)
            .await
            .expect("re-read the friendly match")
            .ranked,
        "and be readable back"
    );
    accept_match_invitation(&loser_config, &friendly.id).await;
    confirm_pending_score(&loser_config, &friendly).await;

    // A ranked match afterwards, which is what gives the assertion a moment
    // it can be checked at.
    play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &iso_offset_hours(-4),
    )
    .await;

    let rating = profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 1).await;
    assert_eq!(
        rating.matches_rated, 1,
        "only the ranked match may count towards the ladder"
    );
}

/// The lock that makes `ranked` trustworthy: you cannot log a game, see that
/// you won, and *then* enrol it in the ladder.
///
/// Rejected rather than silently ignored — a caller who asked for something
/// that didn't happen has to be told — and the stored flag is checked
/// afterwards, because "returns 400" and "changed nothing" are two claims and
/// only one of them is about correctness.
#[tokio::test]
async fn ranked_cannot_be_changed_once_a_score_has_been_submitted() {
    let (config, _user) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    // A scheduled match, so the flag starts out genuinely open. The creator
    // plays in it too, because only an assigned participant may submit the
    // score that closes the flag later on.
    let mut input = create_match_input(&opponent.profile.id);
    input.creator_side_client_id = Some("a".to_string());
    let scheduled = matches_post(&config, input)
        .await
        .expect("create scheduled match");
    assert!(scheduled.ranked, "matches are ranked unless opted out");

    let friendly = matches_match_id_patch(
        &config,
        &scheduled.id,
        models::UpdateMatchInput {
            ranked: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("flip to friendly before it starts");
    assert!(!friendly.ranked);

    // Scoring it closes the decision. (A PATCHed score references the match's
    // real side ids, not the `client_id`s only `create_match` resolves.)
    accept_match_invitation(&opponent_config, &scheduled.id).await;
    let side_a = scheduled.sides[0].id.clone();
    let side_b = scheduled.sides[1].id.clone();
    matches_match_id_patch(
        &config,
        &scheduled.id,
        models::UpdateMatchInput {
            score: Some(Box::new(simple_score(&side_a, &side_b, 6, 3))),
            winner_side_id: Some(side_a.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("submit a score");

    assert_status_with_content(
        matches_match_id_patch(
            &config,
            &scheduled.id,
            models::UpdateMatchInput {
                ranked: Some(true),
                ..Default::default()
            },
        )
        .await,
        reqwest::StatusCode::BAD_REQUEST,
        "a score has already been submitted",
    );
    assert!(
        !matches_match_id_get(&config, &scheduled.id)
            .await
            .expect("re-read the match")
            .ranked,
        "a rejected change must leave the flag alone"
    );

    // Re-sending the value it already has is not a change, and is accepted —
    // otherwise a client that PATCHes its whole edit form would start failing
    // the moment the match was scored.
    let unchanged = matches_match_id_patch(
        &config,
        &scheduled.id,
        models::UpdateMatchInput {
            name: Some("Renamed after the fact".to_string()),
            ranked: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("re-sending the unchanged flag is a no-op, not a rejection");
    assert_eq!(unchanged.name, "Renamed after the fact");
    assert!(!unchanged.ranked);
}

/// The other half of the lock: a match whose start time has passed is closed
/// even if nobody has scored it yet, because by then the result is knowable
/// whether or not it has been typed in.
#[tokio::test]
async fn ranked_cannot_be_changed_once_the_match_has_started() {
    let (config, _user) = new_user().await;
    let (_opponent_config, opponent) = new_user().await;

    let played = matches_post(
        &config,
        completed_match_at(
            &iso_offset_hours(-2),
            vec![invite_users("b", &[&opponent.profile.id])],
        ),
    )
    .await
    .expect("create an already-played match");

    assert_status_with_content(
        matches_match_id_patch(
            &config,
            &played.id,
            models::UpdateMatchInput {
                ranked: Some(false),
                ..Default::default()
            },
        )
        .await,
        reqwest::StatusCode::BAD_REQUEST,
        "ranked and friendly",
    );
    assert!(
        matches_match_id_get(&config, &played.id)
            .await
            .expect("re-read the match")
            .ranked
    );
}

/// **Regression.** The lock used to be evaluated against the match *as
/// stored*, which let a single PATCH carry both the result and the flag.
///
/// The match below is still `scheduled` and still starts tomorrow at the
/// moment the lock runs, so on the prior state alone it passes — and the very
/// same request then records the score and completes the match. That is
/// exactly the "log the game, see that you lost, then de-rank it" case the
/// lock exists to prevent, and reaching it needs no trickery at all: playing
/// earlier than the scheduled slot is ordinary.
#[tokio::test]
async fn ranked_cannot_be_flipped_by_the_same_request_that_submits_the_score() {
    let (config, _user) = new_user().await;
    let (opponent_config, opponent) = new_user().await;

    // Scheduled for tomorrow, so the flag is genuinely open right now — which
    // is the trap: the lock has nothing to object to in the stored record.
    let mut input = create_match_input(&opponent.profile.id);
    input.creator_side_client_id = Some("a".to_string());
    let scheduled = matches_post(&config, input)
        .await
        .expect("create scheduled match");
    assert!(scheduled.ranked, "matches are ranked unless opted out");
    accept_match_invitation(&opponent_config, &scheduled.id).await;

    let side_a = scheduled.sides[0].id.clone();
    let side_b = scheduled.sides[1].id.clone();
    assert_status_with_content(
        matches_match_id_patch(
            &config,
            &scheduled.id,
            models::UpdateMatchInput {
                score: Some(Box::new(simple_score(&side_a, &side_b, 3, 6))),
                winner_side_id: Some(side_b.clone()),
                ranked: Some(false),
                ..Default::default()
            },
        )
        .await,
        reqwest::StatusCode::BAD_REQUEST,
        "this request submits a score",
    );

    // The whole request is refused, not half of it: the endpoint validates
    // before it writes anything, so the score is not recorded either. That
    // matters — a caller who got the score in and the flag rejected would
    // simply have needed one more request.
    let after = matches_match_id_get(&config, &scheduled.id)
        .await
        .expect("re-read the match");
    assert!(after.ranked, "the flag must be untouched");
    assert!(
        after.pending_score.is_none(),
        "and the score must not have landed either"
    );
}

/// The rating-over-time series: one entry per rated match, oldest first, and
/// paginated because it is the same collection a three-year-old account's
/// repair replays.
///
/// The last entry has to equal the number on the profile. They come from two
/// different items written in one transaction, so a mismatch would mean the
/// chart and the headline disagreed about the same account.
#[tokio::test]
async fn rating_history_is_returned_oldest_first_and_paginates() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let played = play_ranked_series(
        &winner_config,
        &winner.profile.id,
        &loser_config,
        &loser.profile.id,
        3,
    )
    .await;

    let first = users_user_id_rating_history_get(
        &winner_config,
        &winner.profile.id,
        "tennis",
        None,
        Some(2),
    )
    .await
    .expect("first page of rating history");
    assert_eq!(first.items.len(), 2, "the page limit is honoured");
    let cursor = first
        .next_cursor
        .clone()
        .expect("a third entry is still to come");

    let second = users_user_id_rating_history_get(
        &winner_config,
        &winner.profile.id,
        "tennis",
        Some(&cursor),
        Some(2),
    )
    .await
    .expect("second page of rating history");
    assert_eq!(second.items.len(), 1, "the tail of the series");

    let entries: Vec<&models::RatingHistoryEntry> =
        first.items.iter().chain(second.items.iter()).collect();
    assert_eq!(
        entries.iter().map(|e| e.match_id.as_str()).collect::<Vec<_>>(),
        played.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        "the series is in played order, oldest first"
    );
    assert!(
        entries.windows(2).all(|w| w[0].played_at < w[1].played_at),
        "played_at must ascend: {:?}",
        entries.iter().map(|e| &e.played_at).collect::<Vec<_>>()
    );
    assert!(
        entries.iter().all(|e| e.delta > 0),
        "three wins should each be a rise: {:?}",
        entries.iter().map(|e| e.delta).collect::<Vec<_>>()
    );
    assert!(
        entries.windows(2).all(|w| w[0].confidence >= w[1].confidence),
        "the ± should narrow as the ladder fills in"
    );

    // The chart's last point is the profile's headline number.
    let profile = profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 3)
        .await;
    let last = entries.last().expect("at least one entry");
    assert_eq!(last.rating, profile.rating);
    assert_eq!(last.confidence, profile.confidence);
    assert_eq!(last.floor, profile.floor);
}

/// History obeys the same visibility rule as the profile — otherwise the
/// setting would hide a number that a second endpoint handed straight over.
///
/// `403` rather than an empty page, because an empty page already means "no
/// rated matches on this ladder" and a client cannot render one response that
/// means either.
#[tokio::test]
async fn rating_history_is_private_until_its_owner_opts_in() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;
    let (viewer_config, _viewer) = new_user().await;

    play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &iso_offset_hours(-3),
    )
    .await;
    profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 1).await;

    assert_forbidden(
        users_user_id_rating_history_get(
            &viewer_config,
            &winner.profile.id,
            "tennis",
            None,
            None,
        )
        .await,
    );
    // The owner is never locked out of their own.
    assert_eq!(
        users_user_id_rating_history_get(
            &winner_config,
            &winner.profile.id,
            "tennis",
            None,
            None
        )
        .await
        .expect("own history")
        .items
        .len(),
        1
    );

    set_rating_visibility(&winner_config, models::RatingVisibility::Public).await;
    assert_eq!(
        users_user_id_rating_history_get(
            &viewer_config,
            &winner.profile.id,
            "tennis",
            None,
            None
        )
        .await
        .expect("history is public now")
        .items
        .len(),
        1
    );

    assert_not_found(
        users_user_id_rating_history_get(
            &viewer_config,
            "does-not-exist",
            "tennis",
            None,
            None,
        )
        .await,
    );
}

/// The placement gate, through the API: five rated matches on a ladder before
/// a band is named, with the countdown visible the whole way.
///
/// The number is shown throughout — it is only the *tier* that would
/// overclaim from a ±500 belief.
#[tokio::test]
async fn a_band_is_only_named_after_five_rated_matches() {
    let (winner_config, winner) = new_user().await;
    let (loser_config, loser) = new_user().await;

    let mut seen_countdowns = Vec::new();
    for played in 1..5 {
        play_ranked_tennis_match(
            &winner_config,
            &loser_config,
            &loser.profile.id,
            &iso_offset_hours(-24 + i64::from(played)),
        )
        .await;
        let rating =
            profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", played).await;
        assert!(rating.rating > 0, "the number is shown while unplaced");
        assert_eq!(band_of(&rating), None, "{played} matches must not band");
        seen_countdowns.push(matches_remaining(&rating));
    }
    assert_eq!(
        seen_countdowns,
        vec![Some(4), Some(3), Some(2), Some(1)],
        "the countdown must tick down towards placement"
    );

    play_ranked_tennis_match(
        &winner_config,
        &loser_config,
        &loser.profile.id,
        &iso_offset_hours(-19),
    )
    .await;
    let placed = profile_rating_after_matches(&winner_config, &winner.profile.id, "tennis", 5).await;
    assert!(
        band_of(&placed).is_some(),
        "the fifth rated match places the account"
    );
    assert_eq!(matches_remaining(&placed), None);
}
