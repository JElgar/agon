//! DAO record structs — the shapes stored in DynamoDB.
//!
//! Deliberately separate from the API (`poem-openapi`) models: the DAO owns its
//! persistence shape and the API layer maps to/from these. Records hold data
//! fields only; keys/GSI attributes are stamped by the `item` layer.

use serde::{Deserialize, Serialize};

// ===========================================================================
// Shared nested value types (DAO-owned; never the API's poem-openapi types).
// These are the structural blobs embedded within items — stored as nested
// DynamoDB maps, typed here for safety rather than as `serde_json::Value`.
// ===========================================================================

/// A geographic location.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocationRecord {
    pub latitude: f64,
    pub longitude: f64,
}

/// A match score. Tagged union mirroring the sport's scoring shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScoreRecord {
    Simple {
        entries: Vec<SimpleScoreEntryRecord>,
    },
    Sets {
        entries: Vec<SetsScoreEntryRecord>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimpleScoreEntryRecord {
    pub side_id: String,
    pub points: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SetsScoreEntryRecord {
    pub side_id: String,
    pub sets: Vec<u32>,
}

/// The agreed, settled score of a match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfirmedScoreRecord {
    pub score: ScoreRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_side_id: Option<String>,
}

/// A submitted score awaiting confirmation, with per-side confirmations so far.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingScoreRecord {
    pub submission_id: String,
    pub score: ScoreRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_side_id: Option<String>,
    #[serde(default)]
    pub confirmations: Vec<ScoreConfirmationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreConfirmationRecord {
    pub side_id: String,
    pub confirmed_by_player_id: String,
    pub confirmed_at: String,
}

/// A confirm/dispute response to a score submission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreResponseRecord {
    pub side_id: String,
    pub responded_by_player_id: String,
    /// "confirm" | "dispute".
    pub response: String,
    pub responded_at: String,
}

/// How an invitation is authorised on acceptance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvitationKindRecord {
    User { invited_user_id: String },
    Token { invite_token: String },
}

/// What an invitation is to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvitationContextRecord {
    Match {
        match_id: String,
        match_name: String,
    },
    Team {
        team_id: String,
        team_name: String,
    },
}

/// The invitation state embedded on a membership (team member / match player).
/// Distinct from the standalone `InvitationRecord` item: this is the snapshot
/// stored inline on the member, without the entity's own keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddedInvitationRecord {
    pub id: String,
    /// "pending" | "accepted" | "declined".
    pub status: String,
    pub invited_by_user_id: String,
    pub invited_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<String>,
    pub kind: InvitationKindRecord,
}

/// `EMAIL#<lowercased-email>` / `#GUARD` — the email-uniqueness guard item.
///
/// Exists only to reserve the email (a conditional put on its PK enforces
/// uniqueness); it records the owning `user_id` so the guard can be traced back
/// / released on an email change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmailGuardRecord {
    pub user_id: String,
}

/// `AUTH#<sub>` / `#GUARD` — maps an identity-provider subject (`sub`) to our
/// stable internal user id.
///
/// The internal `user_id` never changes; only this mapping does when a user's
/// `sub` changes (e.g. migrating auth providers). Resolving a request therefore
/// looks up `AUTH#<sub>` to get the `user_id`, and everything downstream keys off
/// that internal id. Migrating providers rewrites only these guard items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthGuardRecord {
    pub user_id: String,
}

/// `USER#<id>` / `#PROFILE` — the user profile item.
///
/// Counts are denormalized and maintained via atomic `ADD` (see follow ops).
/// `email` is duplicated here for reads; uniqueness is enforced by a separate
/// `EMAIL#<email>` guard item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub follower_count: u64,
    #[serde(default)]
    pub following_count: u64,
    #[serde(default)]
    pub unread_count: u64,
    pub created_at: String,
}

/// `USER#<followeeId>` / `FOLLOWER#<followerId>` — a directed user→user follow
/// edge. Projected into GSI1 (`UFOLLOWING#<followerId>`) so a user can list who
/// they follow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserFollowRecord {
    /// The user being followed.
    pub followee_id: String,
    /// The user doing the following.
    pub follower_id: String,
    pub created_at: String,
}

/// `TEAM#<teamId>` / `FOLLOWER#<userId>` — a user→team follow edge. Projected
/// into GSI3 (`UFOLLOWS_TEAM#<userId>`) for "teams I follow".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamFollowRecord {
    pub team_id: String,
    pub follower_id: String,
    pub created_at: String,
}

/// `TEAM#<teamId>` / `#META` — team metadata. `follower_count` is denormalized
/// and maintained by the follow ops.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamRecord {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_token: Option<String>,
    #[serde(default)]
    pub follower_count: u64,
    pub created_at: String,
}

/// `TEAM#<teamId>` / `MEMBER#<membershipId>` — a team membership. Embeds the
/// shared membership shape (user or external, with optional invitation) as
/// opaque JSON the API layer interprets, plus the team-specific role. Projected
/// into GSI1 (`UTEAMS#<userId>`) for "my teams" — only for members with a
/// resolved `user_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamMemberRecord {
    /// The team this membership belongs to. Lets "my teams" (the GSI1 query over
    /// membership rows) know each row's team without a join.
    pub team_id: String,
    /// Stable membership id (survives external→user acceptance).
    pub membership_id: String,
    /// Linked Agon user, once known. None for an unaccepted external member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Display name for an external member (None once linked to a user).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// `admin` | `member`.
    pub role: String,
    /// The invitation state, if the member was invited (vs added ad-hoc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation: Option<EmbeddedInvitationRecord>,
    pub created_at: String,
}

/// `MATCH#<matchId>` / `#META` — match metadata + resolved scores + social
/// counts. `sides`, `players`, detailed score, submissions, likes and comments
/// live as separate items in the same partition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Sport tag, e.g. "tennis" (the API's `MatchType`, stored as a string).
    pub match_type: String,
    /// Lifecycle: "scheduled" | "in_progress" | "completed" | "cancelled".
    pub status: String,
    pub starts_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_photo_urls: Vec<String>,
    /// The agreed score. None until agreed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_score: Option<ConfirmedScoreRecord>,
    /// A score awaiting confirmation. None if none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_score: Option<PendingScoreRecord>,
    // Denormalized social counts, maintained via atomic ADD.
    #[serde(default)]
    pub like_count: u64,
    #[serde(default)]
    pub comment_count: u64,
    pub created_at: String,
}

/// `MATCH#<matchId>` / `SIDE#<sideId>` — one side of a match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchSideRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `MATCH#<matchId>` / `PLAYER#<playerId>` — a player in a match. Embeds the
/// shared membership shape as opaque JSON; `side_id` is None until assigned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchPlayerRecord {
    /// Stable player/member id — what score events reference.
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_member_of_team: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation: Option<EmbeddedInvitationRecord>,
}

/// `MATCH#<matchId>` / `DETAIL#<sport>` — the sport-specific detailed score.
///
/// The `detail` payload is intentionally `serde_json::Value`: it is a large,
/// deeply-nested, sport-polymorphic blob (a full cricket scorecard with
/// ball-by-ball deliveries, or a football event timeline) that the DAO only ever
/// stores and returns verbatim — it never reads inside it. Typing it would mean
/// porting the entire detailed-score union into the DAO for zero benefit here.
/// This is the one deliberate exception to the "type everything" rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchDetailedScoreRecord {
    pub sport: String,
    pub detail: serde_json::Value,
}

/// `MATCH#<matchId>` / `SCORESUB#<ts>#<subId>` — a score submission and its
/// responses. Score and responses are opaque JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreSubmissionRecord {
    pub submission_id: String,
    pub score: ScoreRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_side_id: Option<String>,
    /// "pending" | "confirmed" | "disputed" | "superseded".
    pub status: String,
    pub submitted_by_player_id: String,
    pub submitted_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub responses: Vec<ScoreResponseRecord>,
}

/// `MATCH#<matchId>` / `LIKE#<userId>` — a like on a match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchLikeRecord {
    pub match_id: String,
    pub user_id: String,
    pub created_at: String,
}

/// A comment on a match. Stored as a top-level comment
/// (`MATCH#<matchId>` / `COMMENT#<ts>#<cid>`) or a reply
/// (`CMT#<parentId>` / `REPLY#<ts>#<rid>`). Tombstoned comments keep the row
/// with `author_user_id`/`text` cleared and `deleted_at` set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommentRecord {
    pub comment_id: String,
    /// The match this comment belongs to (kept on replies too, for convenience).
    pub match_id: String,
    /// Parent comment id for a reply; None for a top-level comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub reply_count: u64,
}

/// `INVITATION#<invId>` / `#META` — a standalone invitation entity.
///
/// Projects to GSI1 (`UINV#<inviteeUserId>` inbox) for user-kind invitations,
/// and to GSI2 (`TOKEN#<token>`) for token-kind invitations. `kind` and
/// `context` are opaque JSON owned by the API layer (the `InvitationKind` /
/// `InvitationContext` unions).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvitationRecord {
    pub id: String,
    /// "pending" | "accepted" | "declined".
    pub status: String,
    /// The user who created/sent the invitation.
    pub invited_by_user_id: String,
    /// The invitee user id, for a user-kind invitation (drives the inbox). None
    /// for a token/external invitation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invited_user_id: Option<String>,
    /// The bearer token, for a token/external invitation (drives token lookup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_token: Option<String>,
    pub kind: InvitationKindRecord,
    /// What the invitation is to (match/team).
    pub context: InvitationContextRecord,
    pub invited_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<String>,
}

/// `USER#<uid>` / `NOTIF#<ts>#<nid>` — a notification for a user.
///
/// `kind` is the tagged notification payload (match invite / like / comment /
/// etc.) as a DAO-owned union. `actor` snapshots the triggering user's id for
/// kinds that have one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationRecord {
    pub id: String,
    pub user_id: String,
    pub is_read: bool,
    pub created_at: String,
    pub kind: NotificationKindRecord,
}

/// The kind of notification. Mirrors the API's `NotificationKind` union but is
/// DAO-owned. Snapshot display fields are stored so the feed renders without
/// extra reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationKindRecord {
    MatchInvitation {
        actor_user_id: String,
        invitation_id: String,
        match_id: String,
        match_name: String,
    },
    TeamInvitation {
        actor_user_id: String,
        invitation_id: String,
        team_id: String,
        team_name: String,
    },
    InvitationAccepted {
        actor_user_id: String,
        invitation_id: String,
        context: InvitationContextRecord,
    },
    Follow {
        actor_user_id: String,
    },
    Like {
        actor_user_id: String,
        match_id: String,
        match_name: String,
    },
    Comment {
        actor_user_id: String,
        match_id: String,
        comment_id: String,
        preview: String,
    },
}

/// `ASSET#<assetId>` / `#META` — an uploadable asset.
///
/// `status` is "pending" | "uploaded" | "failed". `url` is set once uploaded.
/// The presigned upload target is generated on read, not stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetRecord {
    pub id: String,
    /// The user who created the asset (for authorising attachment).
    pub owner_user_id: String,
    /// "profile_image" | "team_image" | "match_header".
    pub purpose: String,
    pub content_type: String,
    /// "pending" | "uploaded" | "failed".
    pub status: String,
    /// Storage object key, needed to generate presigned URLs / read the object.
    pub storage_key: String,
    /// Public URL, set once uploaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub created_at: String,
}

/// `UFEED#<viewerId>` / `FEED#<starts_at>#<matchId>` — a fan-out feed entry.
///
/// A **thin pointer**: it names what to show (`ref_type` + `ref_id`) and carries
/// only the sort key material (`starts_at`), not a denormalized copy of the
/// referenced entity. The read path hydrates the real match from its own item,
/// so feed entries never go stale. Written by the fan-out workflow, one row per
/// viewer, idempotent on `<starts_at>#<matchId>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedItemRecord {
    /// The viewer whose feed this entry belongs to.
    pub viewer_id: String,
    /// What kind of thing this points at (currently always "match").
    pub ref_type: String,
    /// The id of the referenced entity (the match id).
    pub ref_id: String,
    /// Start time of the referenced match — the feed's sort key material.
    pub starts_at: String,
    /// When this feed entry was written (for debugging / potential TTL).
    pub created_at: String,
}

/// `USER#<uid>` / `STATS#<sport>` — per-sport aggregate stats for a user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserSportStatsRecord {
    /// Sport tag, e.g. "tennis".
    pub match_type: String,
    pub matches_played: u64,
    pub wins: u64,
    // Win percentage is derived (wins / matches_played) at the API layer.
}
