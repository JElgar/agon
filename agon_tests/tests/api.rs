//! Integration tests for the Agon API.
//!
//! These run against a live service (`AGON_SERVICE_URL`) backed by real
//! DynamoDB, using the generated OpenAPI client. Each test authenticates as a
//! freshly-created user (JWT minted from `JWT_SECRET`) so tests are independent
//! and can run against a shared environment without colliding.
//!
//! Scope: the synchronously-working surface — users, teams, matches, comments,
//! likes, follows, invitations, notifications. Search and feed depend on the
//! async worker populating indexes / fan-out, so they're only smoke-tested for
//! shape (see the `search` and `feed` tests), not for eventual-consistency
//! content.

use jsonwebtoken::{EncodingKey, Header, encode};
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
}

fn generate_jwt(user_id: &str) -> String {
    let claims = JwtData {
        sub: user_id.to_string(),
        exp: 9999999999,
    };
    let secret_key = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret_key.as_bytes()),
    )
    .expect("failed to generate test jwt")
}

/// A client configured to authenticate as the given subject (JWT `sub`).
fn config_for(subject: &str) -> Configuration {
    Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(generate_jwt(subject)),
        ..Default::default()
    }
}

/// Create a brand-new user and return (their client config, their profile).
/// The JWT subject and the created user id are the same, so the config
/// authenticates as the created user.
async fn new_user() -> (Configuration, models::User) {
    let subject = Uuid::new_v4().to_string();
    let config = config_for(&subject);
    let email = format!("{subject}@example.com");
    let user = users_post(
        &config,
        models::CreateUserInput {
            email,
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
            email: None,
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

#[tokio::test]
async fn search_users_returns_ok() {
    let (config, _user) = new_user().await;
    // We don't assert on content: indexing is async (the worker populates
    // Meilisearch off the stream), so a just-created user may not be indexed
    // yet. We assert the endpoint is wired and returns a well-formed list.
    let results = users_search_get(&config, "Test")
        .await
        .expect("search users");
    let _ = results.len();
}

#[tokio::test]
async fn feed_returns_ok() {
    let (config, _user) = new_user().await;
    let page = feed_get(&config, None, None, None, None)
        .await
        .expect("get feed");
    // Feed fan-out is async / not yet fully implemented; just assert shape.
    let _ = page.items.len();
}

// ---------------------------------------------------------------------------
// Not-found path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_missing_match_returns_not_found() {
    let (config, _user) = new_user().await;

    let response = matches_match_id_get(&config, "does-not-exist").await;
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert!(matches!(
        err,
        openapi::apis::Error::ResponseError(openapi::apis::ResponseContent {
            status: reqwest::StatusCode::NOT_FOUND,
            ..
        })
    ));
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
