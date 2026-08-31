use poem_openapi::{Enum, Object, Union};

use crate::team::AssignableTeamRole;

/// A person in some context (a team, a match side). Modelled as a union so the
/// type enforces what data applies: a linked Agon user has a `user_id` (name and
/// profile resolved from the account), while an external person has only a
/// `display_name`. Both carry an optional `invitation` — even known Agon users
/// accept before they are active. The `id` is stable: when an external member
/// accepts and links/creates an account, the membership flips to `User` while
/// keeping the same `id`, so anything referencing it (e.g. match score events)
/// stays valid.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum Member {
    User(UserMember),
    External(ExternalMember),
}

#[derive(Object)]
pub struct UserMember {
    /// Stable membership id.
    pub id: String,
    /// The linked Agon account.
    pub user_id: String,
    /// The invitation to this context. Pending until the user accepts (which
    /// they can do in-app). None only if added without an invite.
    pub invitation: Option<Invitation>,
    /// The linked account's display name, hydrated at read time from the
    /// user profile. Empty if the account could no longer be found.
    pub name: String,
    /// The linked account's profile image, if they've set one.
    pub avatar_url: Option<String>,
}

#[derive(Object)]
pub struct ExternalMember {
    /// Stable membership id (survives acceptance / linking to an account).
    pub id: String,
    /// Display name — the only identity we have for someone without an account.
    pub display_name: String,
    /// None = added ad-hoc by name (e.g. a ringer); Some = formally invited.
    pub invitation: Option<Invitation>,
}

/// An invitation to join a context (team or match). A separately tracked,
/// independently addressable entity (e.g. accepted by token, fetched by id).
/// The common fields are flat; `kind` carries the only thing that varies — how
/// acceptance is authorised. Note `kind` is independent of the eventual member
/// variant: an external invitee accepted via token becomes a `User` member, but
/// the invitation stays `Token` — it records how the outreach was issued.
#[derive(Object)]
pub struct Invitation {
    pub id: String,
    pub status: InvitationStatus,
    /// The Agon user who created (sent) this invitation.
    pub invited_by_user_id: String,
    pub invited_at: chrono::DateTime<chrono::Utc>,
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub kind: InvitationKind,
}

/// How an invitation is authorised on acceptance.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum InvitationKind {
    /// Targets a known Agon user. Accepted by that user (the accepting request's
    /// user id must match `invited_user_id`); no secret needed.
    User(UserInvitation),
    /// Targets someone with no account. Whoever holds `invite_token` may accept
    /// and claim the external identity — the token is the credential.
    Token(TokenInvitation),
}

#[derive(Object)]
pub struct UserInvitation {
    pub invited_user_id: String,
}

#[derive(Object)]
pub struct TokenInvitation {
    pub invite_token: String,
}

/// A reply to an invitation. Distinct from `InvitationStatus` (which includes
/// `pending`) because you can only respond accepted/declined.
#[derive(Enum, Debug)]
#[oai(rename_all = "snake_case")]
pub enum InvitationResponse {
    Accepted,
    Declined,
}

/// A standalone invitation together with what it is an invite *to*. Used by the
/// inbox and fetch-by-id views, where the context isn't otherwise known. The
/// invitation embedded in a team/match member omits this — the context is the
/// team/match being viewed.
#[derive(Object)]
pub struct InvitationDetail {
    pub invitation: Invitation,
    pub context: InvitationContext,
}

#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum InvitationContext {
    Match(InvitationMatchContext),
    Team(InvitationTeamContext),
}

#[derive(Object)]
pub struct InvitationMatchContext {
    pub match_id: String,
    pub match_name: String,
}

#[derive(Object)]
pub struct InvitationTeamContext {
    pub team_id: String,
    pub team_name: String,
}

/// Invite people to a team or match. Agon users by id; external people by name
/// (the server mints a token invitation for each).
#[derive(Object)]
pub struct AddInvitationsInput {
    pub invited_user_ids: Vec<String>,
    pub invited_external_names: Vec<String>,
    /// (Match invitations only) the side to invite these people to. None invites
    /// them to the match without a side, to be chosen on acceptance. Ignored for
    /// team invitations.
    pub side_id: Option<String>,
    /// (Team invitations only) the role every invitee in this batch gets once
    /// accepted — `Admin` or `Member`, applied uniformly (invite some people as
    /// admin and others as member in one call by sending two calls). None
    /// defaults to `Member`. Ignored for match invitations.
    pub role: Option<AssignableTeamRole>,
}

#[derive(Object)]
pub struct RespondToInvitationInput {
    pub response: InvitationResponse,
    /// (Match invitations only) the side the invitee is joining. Required when
    /// accepting a match invitation that was not already assigned a side;
    /// ignored otherwise.
    pub side_id: Option<String>,
}

#[derive(Object)]
pub struct RespondByTokenInput {
    pub invite_token: String,
    pub response: InvitationResponse,
    /// (Match invitations only) the side the invitee is joining. Required when
    /// accepting a match invitation that was not already assigned a side;
    /// ignored otherwise.
    pub side_id: Option<String>,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
}

/// A player's authority on a match — own type, deliberately not a reuse of
/// `TeamRole` (team and match roles may diverge over time). See
/// `MatchPlayer.role`.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum MatchPlayerRole {
    /// The match's owner — one at a time, transferable (`POST
    /// /matches/:id/transfer-ownership`, which demotes the outgoing owner to
    /// `Admin`). The playing creator gets this by default.
    Owner,
    /// Full authority over the match short of transferring ownership: manage
    /// `join_policy`/side caps, mint or revoke join-links, invite people.
    Admin,
    /// An ordinary roster member. Can still invite named people (any
    /// participant may), just not the more structural admin actions.
    Player,
}

/// A match's self-serve-join settings: whether/how a joiner (via a join link
/// today; team self-join in a later phase) may pick a side.
#[derive(Object, Debug, Clone)]
pub struct JoinPolicy {
    pub side_selection: SideSelection,
}

/// See `JoinPolicy`. Naming is literal about behavior — landing unassigned is
/// not a new roster state (a `MatchPlayer` with no `side_id` already means
/// that); this only controls whether a self-serve joiner gets to choose.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
#[oai(rename_all = "snake_case")]
pub enum SideSelection {
    /// Every self-serve joiner lands unassigned; the organizer assigns sides
    /// later (`PATCH /matches/:id`'s `side_assignments`).
    UnassignedOnly,
    /// A joiner must pick one of the match's sides; landing unassigned isn't
    /// offered.
    SideRequired,
    /// A joiner may pick a side or go unassigned. The default.
    SideOptional,
}

/// A shareable, many-use join link for a match. Unlike a token `Invitation`
/// (single-use — pre-bound to one specific roster row), any number of
/// different people may join via the same link's token, bounded only by the
/// match's own capacity.
#[derive(Object)]
pub struct JoinLink {
    pub id: String,
    pub match_id: String,
    pub token: String,
    pub scope: JoinLinkScope,
    pub created_by_user_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Set once the link has been revoked — a revoked link can no longer be
    /// joined via (`POST /matches/:id/join` / the by-token lookup 404s).
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Which side(s)/unassigned a join link may join. Overrides the match's own
/// `JoinPolicy` for joins made through this specific link.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum JoinLinkScope {
    /// Defer to the match's own `join_policy`.
    Inherit(JoinLinkScopeInherit),
    /// Always joins the unassigned pool, regardless of `join_policy`.
    Unassigned(JoinLinkScopeUnassigned),
    /// May only join one of these specific sides — one entry for a
    /// single-side link, several for e.g. "either side of this intra-squad
    /// match". Always wins over an `unassigned_only` `join_policy`: making a
    /// side-scoped link is an explicit choice to fill that side.
    Sides(JoinLinkScopeSides),
}

#[derive(Object)]
pub struct JoinLinkScopeInherit {}

#[derive(Object)]
pub struct JoinLinkScopeUnassigned {}

#[derive(Object)]
pub struct JoinLinkScopeSides {
    pub side_ids: Vec<String>,
}

#[derive(Object)]
pub struct CreateJoinLinkInput {
    pub scope: JoinLinkScope,
}

/// The public, unauthenticated preview of a join link — enough for a client
/// to render "Join Sunday 5-a-side (Team A) — 6/10 joined" before the viewer
/// signs in. Returned by `GET /join-links/by-token/:token`.
#[derive(Object)]
pub struct JoinLinkPreview {
    pub match_id: String,
    pub match_name: String,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub scope: JoinLinkScope,
    pub total_player_count: u32,
    /// The match's derived overall cap (see `JoinPolicy`'s doc comment on
    /// `Match` for how it's computed). `None` = uncapped.
    pub max_players: Option<u32>,
}

/// Join a match — via a join link's token, or (a later phase) by virtue of
/// team membership. `side_id` picks which side to join; omit for unassigned,
/// where the resolved join scope allows it.
#[derive(Object)]
pub struct JoinMatchInput {
    pub token: Option<String>,
    pub side_id: Option<String>,
}

#[derive(Object)]
pub struct TransferMatchOwnershipInput {
    /// The player (by stable player id, as seen on `MatchPlayer.member.id`)
    /// to hand the `Owner` role to. Must already be an accepted player.
    pub player_id: String,
}
