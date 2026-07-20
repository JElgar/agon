use poem_openapi::{Enum, Object, Union};

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
