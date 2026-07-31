//! DAO record structs — the shapes stored in DynamoDB.
//!
//! Deliberately separate from the API (`poem-openapi`) models: the DAO owns its
//! persistence shape and the API layer maps to/from these. Records hold data
//! fields only; keys/GSI attributes are stamped by the `item` layer.

use std::collections::HashMap;

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

/// One header photo attached to a match: the asset it was uploaded as (so a
/// later edit can re-include, reorder, or mix it with newly uploaded photos)
/// plus its canonical serving URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderPhotoRecord {
    pub asset_id: String,
    pub url: String,
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
    Cricket {
        innings: Vec<CricketScoreInningsRecord>,
    },
    Football {
        goals: Vec<FootballGoalEventRecord>,
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

/// One innings' final totals, as stored on a match's confirmed/pending
/// `Score` — mirrors the API's `CricketScoreInnings`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketScoreInningsRecord {
    pub batting_side_id: String,
    pub bowling_side_id: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: OversRecord,
    pub declared: bool,
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// over — mirrors the API's `detailed_score::cricket::Overs`. Two integer
/// fields rather than a single float, which can't safely represent a ball
/// count that doesn't fit in one decimal digit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct OversRecord {
    pub overs: u32,
    pub balls: u32,
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
/// `EMAIL#<email>` guard item. `stats` holds per-sport aggregates inline, so a
/// profile read/batch-read returns everything in one point read — always
/// present (an empty map for a brand new user) so the stats reconciler's
/// nested-attribute updates always have a map to write into.
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
    #[serde(default)]
    pub stats: HashMap<String, UserSportStatsRecord>,
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
    /// The user who created (organizes) the match. They may manage it — edit,
    /// invite, record the result — even when not playing in it themselves.
    /// `#[serde(default)]` for records written before this field existed.
    #[serde(default)]
    pub created_by_user_id: String,
    pub name: String,
    pub description: String,
    /// Sport tag, e.g. "tennis" (the API's `MatchType`, stored as a string).
    pub match_type: String,
    /// Lifecycle: "scheduled" | "in_progress" | "completed" | "cancelled".
    pub status: String,
    pub starts_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRecord>,
    /// Header photos, in display order (first = shown first). `#[serde(default)]`
    /// for records written before this field existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_photos: Vec<HeaderPhotoRecord>,
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
    /// The seq of the last appended `LIVEEVT#` (0 = no live events yet).
    /// Doubles as the optimistic-concurrency + ordering gate for
    /// `append_live_events`: a batch must state the tip it last saw, and the
    /// counter bump that reserves its seq range is conditioned on this value.
    /// `#[serde(default)]` for matches written before live scoring existed.
    #[serde(default)]
    pub live_seq: u32,
    /// Match format/rules configuration (overs per innings, half length, and
    /// so on). Embedded directly on the match record (not a separate item,
    /// unlike the detailed score) because live scoring wants it on the same
    /// fetch as everything else. `None` until a format is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<MatchFormatRecord>,
    pub created_at: String,
}

/// Mirrors `agon_service::match_format::MatchFormat`, sport-first
/// discriminated like `LiveEventPayloadRecord` — a typed DAO enum rather
/// than opaque JSON, so a variant added on the API side and forgotten here
/// is a compile error, not a silent runtime data loss. New sport = new
/// variant on both sides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "sport", rename_all = "snake_case")]
pub enum MatchFormatRecord {
    Football(FootballFormatRecord),
    Cricket(CricketFormatRecord),
}

/// Mirrors `agon_service::match_format::FootballFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballFormatRecord {
    pub half_length_minutes: u32,
    pub num_halves: u32,
    pub extra_time: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_time_half_length_minutes: Option<u32>,
    pub penalties: bool,
}

/// Mirrors `agon_service::match_format::CricketFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketFormatRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overs_per_innings: Option<u32>,
    pub innings_per_side: u32,
    pub balls_per_over: u32,
    pub no_ball_penalty_runs: u32,
    pub wide_penalty_runs: u32,
    /// `#[serde(default = "default_true")]` for records written before these
    /// two fields existed — the standard rule (extra ball) is the safe
    /// default for a match that never configured otherwise.
    #[serde(default = "default_true")]
    pub wide_is_extra_ball: bool,
    #[serde(default = "default_true")]
    pub no_ball_is_extra_ball: bool,
    pub free_hit_after_no_ball: bool,
}

fn default_true() -> bool {
    true
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

/// `MATCH#<matchId>` / `DETAIL#<sport>` — the match's detailed score: live or
/// finished, the same record either way (see `agon_service::detailed_score`'s
/// module docs). A cricket match being scored live keeps this up to date
/// incrementally, one delivery at a time; a manually-entered match (or one
/// past its last live event) just has it written directly.
///
/// The `detail` payload is intentionally `serde_json::Value`: it is a large,
/// deeply-nested, sport-polymorphic blob (a full cricket scorecard, or a
/// football event timeline) that the DAO only ever stores and returns
/// verbatim — it never reads inside it. Typing it would mean porting the
/// entire detailed-score union into the DAO for zero benefit here. This is
/// the one deliberate exception to the "type everything" rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchDetailedScoreRecord {
    pub sport: String,
    pub detail: serde_json::Value,
    /// The seq of the last live event folded into `detail`, if any — `None`
    /// for a match with no live event log behind this record (manual
    /// entry). Lets an incremental update confirm this is caught up to
    /// exactly the start of the new batch before applying it, and lets a
    /// reader know whether to trust this over a fresh derive.
    #[serde(default)]
    pub last_seq: Option<u32>,
}

/// `MATCH#<matchId>` / `LIVEEVT#<seq>` — one live-scoring event, in append
/// order. The source of truth for live scoring. Corrections are direct
/// mutations of this log — `delete_live_event` removes an item outright,
/// `amend_live_event` overwrites its `payload` in place — not a layered
/// "void" event; there's nothing else in this record to distinguish an
/// original entry from a corrected one.
///
/// `payload` is a DAO-owned mirror of `agon_service::live_score::LiveEventInput`
/// (see `LiveEventPayloadRecord` below) — unlike `MatchDetailedScoreRecord`,
/// this is *not* opaque JSON. The event log is the actively-growing, most
/// load-bearing part of the live-scoring feature, so the two enum trees are
/// kept in sync by hand: a variant added on the API side and forgotten here
/// is a compile error in this crate, not a silent runtime data loss.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiveEventRecord {
    pub seq: u32,
    pub payload: LiveEventPayloadRecord,
    pub recorded_by_user_id: String,
    /// When this actually happened on the recording device — may be well
    /// before `recorded_at` if the device was offline when it was recorded.
    pub occurred_at: String,
    /// When the server received/persisted the event.
    pub recorded_at: String,
}

/// DAO-owned mirror of `agon_service::live_score::LiveEventInput`, sport-
/// first discriminated. New sport = new variant on both sides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "sport", rename_all = "snake_case")]
pub enum LiveEventPayloadRecord {
    Football(FootballLiveEventRecord),
    Cricket(CricketLiveEventRecord),
}

// ---- Football live events --------------------------------------------------

/// Mirrors `agon_service::live_score::football::FootballLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FootballLiveEventRecord {
    Goal(FootballGoalEventRecord),
    Card(FootballCardEventRecord),
    Substitution(FootballSubstitutionEventRecord),
    Period(FootballPeriodEventRecord),
    PenaltyShootoutKick(FootballPenaltyShootoutKickRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballGoalEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assist_player_id: Option<String>,
    pub own_goal: bool,
    pub penalty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballCardEventRecord {
    pub side_id: String,
    pub player_id: String,
    pub color: FootballCardColorRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FootballCardColorRecord {
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballSubstitutionEventRecord {
    pub side_id: String,
    pub player_in_id: String,
    pub player_out_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballPeriodEventRecord {
    pub period: FootballPeriodRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FootballPeriodRecord {
    KickOff,
    HalfTime,
    SecondHalfKickOff,
    FullTime,
    ExtraTimeKickOff,
    ExtraTimeHalfTime,
    ExtraTimeSecondHalfKickOff,
    ExtraTimeFullTime,
    PenaltiesComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballPenaltyShootoutKickRecord {
    pub side_id: String,
    pub scored: bool,
}

// ---- Cricket live events ----------------------------------------------------

/// Mirrors `agon_service::live_score::cricket::CricketLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CricketLiveEventRecord {
    Delivery(CricketDeliveryRecord),
    Retire(CricketRetireEventRecord),
    InningsStart(CricketInningsStartEventRecord),
    InningsEnd(CricketInningsEndEventRecord),
}

/// Mirrors `detailed_score::cricket::CricketDelivery` (reused verbatim as the
/// API's live delivery payload).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryRecord {
    pub over: u32,
    pub ball: u32,
    pub bowler_player_id: String,
    pub striker_player_id: String,
    pub non_striker_player_id: String,
    pub runs_off_bat: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<CricketDeliveryExtraRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wicket: Option<CricketDeliveryWicketRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryExtraRecord {
    pub kind: CricketExtraKindRecord,
    pub runs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CricketExtraKindRecord {
    Wide,
    NoBall,
    Bye,
    LegBye,
    Penalty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryWicketRecord {
    pub kind: CricketDismissalKindRecord,
    pub dismissed_player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fielder_player_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CricketDismissalKindRecord {
    Bowled,
    Caught,
    LegBeforeWicket,
    RunOut,
    Stumped,
    HitWicket,
    RetiredOut,
    RetiredHurt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketRetireEventRecord {
    pub batter_player_id: String,
    pub retired_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketInningsStartEventRecord {
    pub batting_side_id: String,
    pub bowling_side_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketInningsEndEventRecord {
    pub reason: InningsEndReasonRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InningsEndReasonRecord {
    AllOut,
    OversComplete,
    Declared,
    TargetReached,
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

/// A comment on a match. The base item lives in the match partition, addressed
/// by id — a top-level comment (`MATCH#<matchId>` / `COMMENT#<cid>`) or a reply
/// (`MATCH#<matchId>` / `REPLY#<rid>`); time-ordered listing is via GSI1
/// (`MCOMMENTS#<matchId>` / `CREPLIES#<parentId>`, sort `<ts>#<id>`). Tombstoned
/// comments keep the row with `author_user_id`/`text` cleared and `deleted_at` set.
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
    /// Someone replied to a comment on a match. `comment_id` is the reply's own
    /// id; `parent_comment_id` is the top-level comment whose thread it belongs
    /// to (so the client can open the thread).
    Reply {
        actor_user_id: String,
        match_id: String,
        comment_id: String,
        parent_comment_id: String,
        preview: String,
    },
    /// A score was submitted for a match you played in. `needs_confirmation`
    /// distinguishes the two messages: `true` => your side must confirm it,
    /// `false` => informational (your side already implicitly confirmed, or the
    /// score was set directly). `actor_user_id` is the submitter.
    ScoreSubmitted {
        actor_user_id: String,
        match_id: String,
        match_name: String,
        submission_id: String,
        needs_confirmation: bool,
    },
    /// A score you submitted was confirmed by the other side(s). Sent to the
    /// submitter; `actor_user_id` is the participant whose confirmation completed
    /// it.
    ScoreConfirmed {
        actor_user_id: String,
        match_id: String,
        match_name: String,
        submission_id: String,
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
    /// Exact byte length the client declared at creation. Baked into the
    /// presigned PUT so S3 rejects any upload that isn't this size (the server
    /// validates it against a max before issuing the URL). `0` for assets created
    /// before this field existed — treated as "no length constraint".
    #[serde(default)]
    pub content_length: i64,
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

/// Aggregate stats for one sport, stored inline on `UserRecord::stats` keyed
/// by sport tag (e.g. "tennis") — the key carries the sport, so it isn't
/// duplicated in the value.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UserSportStatsRecord {
    pub matches_played: u64,
    pub wins: u64,
    // Win percentage is derived (wins / matches_played) at the API layer.
}

/// `MATCH#<mid>` / `STATCONTRIB#<uid>` — what a single match currently
/// contributes to one participant's per-sport stats. The stats reconciler
/// stores this after applying it, then on any later match change diffs the new
/// desired contribution against this to compute the delta to apply. Absent =>
/// the match has never contributed for this user (treated as zero).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatContributionRecord {
    /// Sport the contribution counted under (matches the match's type at the
    /// time it was applied). Kept so a sport change can move the counts to the
    /// right sport's counters in `UserRecord::stats`.
    pub match_type: String,
    /// 1 while the match is completed and the user played; 0 otherwise.
    pub played: u64,
    /// 1 while the user's side is the confirmed winner; 0 otherwise.
    pub won: u64,
}
