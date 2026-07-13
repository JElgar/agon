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

/// A minimal two-sided match: sides "a" and "b", one invited user on side "a".
fn create_match_input(invited_user_id: &str) -> models::CreateMatchInput {
    models::CreateMatchInput {
        name: "Test Match".to_string(),
        description: "A test match".to_string(),
        match_type: models::MatchType::Tennis,
        starts_at: "2026-08-01T10:00:00Z".to_string(),
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
            invited_external_names: vec![],
        }],
        score: None,
        winner_side_id: None,
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
        invited_external_names: vec![],
    };
    models::CreateMatchInput {
        name: name.to_string(),
        description: "A test match".to_string(),
        match_type: models::MatchType::Tennis,
        starts_at: "2026-08-01T10:00:00Z".to_string(),
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
        score: None,
        winner_side_id: None,
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
    // The creator is a member.
    assert!(!fetched.members.is_empty());
}

#[tokio::test]
async fn team_appears_in_my_teams() {
    let (config, _user) = new_user().await;

    let team = teams_post(
        &config,
        models::CreateTeamInput {
            name: "My Team".to_string(),
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
        },
    )
    .await
    .expect("create team");

    let with_member = teams_team_id_members_post(
        &config,
        &team.id,
        models::AddTeamMembersInput {
            user_ids: vec![member.profile.id.clone()],
        },
    )
    .await
    .expect("add member");
    assert!(member_ids(&with_member).contains(&member.profile.id));

    // Find the membership id for the added user to remove them.
    let member_id = membership_id_for(&with_member, &member.profile.id).expect("membership id");
    let after_remove = teams_team_id_members_member_id_delete(&config, &team.id, &member_id)
        .await
        .expect("remove member");
    assert!(!member_ids(&after_remove).contains(&member.profile.id));
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
    assert_eq!(fetched.players.len(), 1, "get_match returns the invited player");
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
    input.sides[0].team_id = Some(team.id.clone());
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

// ---------------------------------------------------------------------------
// Match scoring (PATCH a score -> completes match + records a submission)
// ---------------------------------------------------------------------------

/// A simple score for the two default sides ("a"/"b" client ids map to real
/// side ids on the created match, so we read them off the created match).
fn simple_score(side_a: &str, side_b: &str, a: i32, b: i32) -> models::Score {
    models::Score::Simple(Box::new(models::ScoreSimpleScore {
        entries: vec![
            models::SimpleScoreEntry {
                side_id: side_a.to_string(),
                points: a,
            },
            models::SimpleScoreEntry {
                side_id: side_b.to_string(),
                points: b,
            },
        ],
        r#type: Default::default(),
    }))
}

#[tokio::test]
async fn scoring_a_match_completes_it_and_records_a_submission() {
    let (config, _owner) = new_user().await;
    let (_invitee_config, invitee) = new_user().await;

    let created = matches_post(&config, create_match_input(&invitee.profile.id))
        .await
        .expect("create match");
    assert!(matches!(created.status, models::MatchStatus::Scheduled));
    let side_a = created.sides[0].id.clone();
    let side_b = created.sides[1].id.clone();

    // PATCH a score: this both completes the match and records a submission.
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
    assert!(updated.confirmed_score.is_some());

    // The submission is visible in the match's submission history.
    let submissions = matches_match_id_score_submissions_get(&config, &created.id)
        .await
        .expect("list submissions");
    assert!(!submissions.is_empty(), "a submission was recorded");
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
    assert!(replies.items.iter().any(|c| c.text.as_deref() == Some("reply")));
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

    let following =
        users_user_id_following_get(&follower_config, &follower.profile.id, None, None)
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
        },
    )
    .await
    .expect("create team");

    let updated = teams_team_id_patch(
        &config,
        &team.id,
        models::UpdateTeamInput {
            name: Some("After".to_string()),
        },
    )
    .await
    .expect("patch team");
    assert_eq!(updated.name, "After");
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
    assert!(matches!(responded.status, models::InvitationStatus::Declined));
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

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// The (linked user id, stable membership id) of each team member that is a
/// linked Agon user. External (unlinked) members have no user id, so they're
/// omitted.
fn member_user_and_membership_ids(team: &models::Team) -> Vec<(String, String)> {
    team.members
        .iter()
        .filter_map(|m| match &*m.member {
            models::Member::User(u) => Some((u.user_id.clone(), u.id.clone())),
            models::Member::External(_) => None,
        })
        .collect()
}

/// The linked user ids of a team's members.
fn member_ids(team: &models::Team) -> Vec<String> {
    member_user_and_membership_ids(team)
        .into_iter()
        .map(|(user_id, _)| user_id)
        .collect()
}

/// The stable membership id for a member with the given linked user id.
fn membership_id_for(team: &models::Team, user_id: &str) -> Option<String> {
    member_user_and_membership_ids(team)
        .into_iter()
        .find(|(uid, _)| uid == user_id)
        .map(|(_, membership_id)| membership_id)
}
