use poem_openapi::{Enum, Object};

use crate::Photo;
use crate::membership::Member;

/// A persistent team/squad. The pool of people a match side can be drawn from;
/// a match never derives its roster live from this (see MatchSide), it snapshots
/// the selected players at match-creation time.
#[derive(Object)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// Team picture. Uploaded via the Asset API (`team_image` purpose), same
    /// flow as a user's profile picture.
    pub profile_image: Option<Photo>,
    pub members: Vec<TeamMember>,
    /// Shareable token for inviting people to join the team. None if no active
    /// invite link.
    pub invite_token: Option<String>,
    pub follower_count: u32,
    /// Whether the requesting user follows this team.
    pub is_followed_by_me: bool,
}

/// A person's membership of a team: the shared `Member` (user or external,
/// with optional invitation) plus the team-specific role.
#[derive(Object)]
pub struct TeamMember {
    pub member: Member,
    /// Determines who can act on the team's behalf (e.g. accept a fixture).
    pub role: TeamRole,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum TeamRole {
    /// Can manage the team and accept fixtures on its behalf.
    Admin,
    Member,
}

/// Lightweight team representation for lists / search results (no members).
#[derive(Object)]
pub struct TeamListItem {
    pub id: String,
    pub name: String,
    pub profile_image: Option<Photo>,
    pub follower_count: u32,
    /// Whether the requesting user follows this team.
    pub is_followed_by_me: bool,
}

#[derive(Object)]
pub struct CreateTeamInput {
    pub name: String,
    /// References an `Asset` (of `team_image` purpose) the client uploaded via
    /// the Asset API — same flow as a user's profile picture. None = no image.
    pub profile_image_asset_id: Option<String>,
    /// Agon users to invite to the new team, alongside the creator (who joins
    /// automatically as admin). Each gets a pending membership + invitation,
    /// exactly like `POST /teams/{team_id}/invitations` — just bundled into
    /// team creation instead of a separate call.
    pub invited_user_ids: Vec<String>,
    /// People with no Agon account to invite by name (token-based invites).
    pub invited_external_names: Vec<String>,
}

/// Editable fields on a team. All optional — only supplied fields change.
#[derive(Object)]
pub struct UpdateTeamInput {
    pub name: Option<String>,
    /// Same as `CreateTeamInput::profile_image_asset_id`. None leaves the
    /// current image unchanged.
    pub profile_image_asset_id: Option<String>,
}

#[derive(Object)]
pub struct AddTeamMembersInput {
    pub user_ids: Vec<String>,
}

/// How multiple `team_id` values combine in `GET /matches`'s team filter.
/// Meaningless with fewer than two ids.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum TeamMatchMode {
    /// A match involving at least one of the given teams (union). The default.
    Any,
    /// A match involving every one of the given teams — e.g. head-to-head
    /// history between two teams.
    All,
}
