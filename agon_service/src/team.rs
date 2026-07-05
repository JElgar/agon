use poem_openapi::{Enum, Object};

use crate::membership::Member;

/// A persistent team/squad. The pool of people a match side can be drawn from;
/// a match never derives its roster live from this (see MatchSide), it snapshots
/// the selected players at match-creation time.
#[derive(Object)]
pub struct Team {
    pub id: String,
    pub name: String,
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
    pub follower_count: u32,
    /// Whether the requesting user follows this team.
    pub is_followed_by_me: bool,
}

#[derive(Object)]
pub struct CreateTeamInput {
    pub name: String,
}

/// Editable fields on a team. All optional — only supplied fields change.
#[derive(Object)]
pub struct UpdateTeamInput {
    pub name: Option<String>,
}

#[derive(Object)]
pub struct AddTeamMembersInput {
    pub user_ids: Vec<String>,
}
