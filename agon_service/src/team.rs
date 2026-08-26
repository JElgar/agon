use poem_openapi::{Enum, Object};

use crate::Photo;
use crate::membership::Member;

/// A persistent team/squad's metadata. The pool of people a match side can be
/// drawn from; a match never derives its roster live from this (see
/// MatchSide), it snapshots the selected players at match-creation time.
///
/// Members are *not* embedded here — a team's roster is fetched separately
/// via the paginated `GET /teams/{team_id}/members`, the same shape as a
/// user's followers, rather than growing this response unboundedly.
#[derive(Object)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// Team logo. Uploaded via the Asset API (`team_logo` purpose), same flow
    /// as a user's profile picture.
    pub logo: Option<Photo>,
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
    /// The team's creator, permanently — there's no ownership-transfer flow
    /// yet. The only role that can delete the team; every other management
    /// action an admin can also do. The owner's own role can never be
    /// changed, and the owner can never be removed as a member.
    Owner,
    /// Can manage the team — edit its name/logo, add/remove/invite members,
    /// change another (non-owner) member's role — and accept fixtures on its
    /// behalf. Everything but delete the team.
    Admin,
    /// View-only: sees the team like anyone else, no management actions.
    Member,
}

/// A role assignable via `PATCH /teams/{team_id}/members/{member_id}`.
/// Deliberately a separate, smaller type from `TeamRole` rather than reusing
/// it with runtime validation: `Owner` is permanent (no ownership-transfer
/// flow), so excluding it here makes "you can't set someone to owner" a type
/// error for API consumers instead of a 400 they have to hit first.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum AssignableTeamRole {
    Admin,
    Member,
}

/// Lightweight team representation for lists / search results (no members).
#[derive(Object)]
pub struct TeamListItem {
    pub id: String,
    pub name: String,
    pub logo: Option<Photo>,
    pub follower_count: u32,
    /// Whether the requesting user follows this team.
    pub is_followed_by_me: bool,
}

#[derive(Object)]
pub struct CreateTeamInput {
    pub name: String,
    /// References an `Asset` (of `team_logo` purpose) the client uploaded via
    /// the Asset API — same flow as a user's profile picture. None = no logo.
    pub logo_asset_id: Option<String>,
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
    /// Same as `CreateTeamInput::logo_asset_id`. None leaves the current logo
    /// unchanged.
    pub logo_asset_id: Option<String>,
}

#[derive(Object)]
pub struct AddTeamMembersInput {
    pub user_ids: Vec<String>,
}

#[derive(Object)]
pub struct UpdateTeamMemberRoleInput {
    pub role: AssignableTeamRole,
}

/// The membership (by stable membership id, as seen on `TeamMember.member.id`)
/// to hand the `owner` role to. A dedicated endpoint/input rather than folding
/// this into `UpdateTeamMemberRoleInput` — `AssignableTeamRole` deliberately
/// excludes `Owner` (see its doc comment), and ownership transfer has its own
/// rule (caller must already be the owner) distinct from a role change
/// (owner or admin).
#[derive(Object)]
pub struct TransferTeamOwnershipInput {
    pub member_id: String,
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
