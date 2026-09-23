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

/// x-macro template for `crate::sports::core_sports!` (the single source of
/// truth for which sports are fully modeled — see `crate::sports`): every
/// sport-shaped DAO aggregate, one variant/field per sport wrapping that
/// sport's own record type from `crate::sports::<sport>`.
macro_rules! define_sport_records {
    ($( $variant:ident {
        tag: $tag:literal,
        module: $module:ident,
        score: $score:ty,
        format: $format:ty,
        live_event: $live_event:ty,
        stats: $stats:ty,
        record: $record:ty $(,)?
    } ),+ $(,)?) => {
        /// A match score. Tagged union mirroring the sport's scoring shape.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "type", rename_all = "snake_case")]
        pub enum ScoreRecord {
            Simple {
                /// Points per side, keyed by side id.
                entries: HashMap<String, u32>,
            },
            Sets {
                /// Games won per set per side, keyed by side id.
                entries: HashMap<String, Vec<u32>>,
            },
            $( $variant($score), )+
        }

        /// Mirrors `agon_service::match_format::MatchFormat`, sport-first
        /// discriminated like `LiveEventPayloadRecord` — a typed DAO enum
        /// rather than opaque JSON, so a variant added on the API side and
        /// forgotten here is a compile error, not a silent runtime data
        /// loss. New sport = new variant on both sides.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "sport", rename_all = "snake_case")]
        pub enum MatchFormatRecord {
            $( $variant($format), )+
        }

        /// DAO-owned mirror of `agon_service::live_score::LiveEventInput`,
        /// sport-first discriminated. New sport = new variant on both sides.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "sport", rename_all = "snake_case")]
        pub enum LiveEventPayloadRecord {
            $( $variant($live_event), )+
        }

        /// A user's lifetime stats, one field per sport — `None` for a sport
        /// they've never played a confirmed match in. Stored inline on
        /// `UserRecord::stats`.
        ///
        /// Explicit named fields rather than a `HashMap<String, _>` keyed by
        /// sport tag: the set of sports is closed (mirrors the API's
        /// `MatchType`), and an `Option` field serializes absent the same way
        /// a missing map key would, at the same DynamoDB storage shape
        /// (`stats.cricket`, `stats.football`, ... as nested map attributes
        /// either way). What a named field buys over the map: each
        /// fully-modeled sport's own counters (cricket's `runs`/`wickets`,
        /// football's `goals`/`assists`, ...) are real, compiler-checked
        /// fields instead of stringly-keyed lookups into a counters bag.
        /// Sports with no richer box score are just the common shape.
        ///
        /// The DAO's *write* path (`Dao::stats_delta`/`update_best_figures`)
        /// doesn't construct or read this type at all — it addresses
        /// `stats.<sport>.<counter>` via raw `UpdateItem` expressions built
        /// from a runtime sport tag and counter-name strings, so it stays
        /// fully sport-agnostic regardless of how strongly-typed this read
        /// side is.
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        pub struct UserStatsRecord {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $module: Option<$stats>,
            )+
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub tennis: Option<GenericSportStatsRecord>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub badminton: Option<GenericSportStatsRecord>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub squash: Option<GenericSportStatsRecord>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub table_tennis: Option<GenericSportStatsRecord>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub other: Option<GenericSportStatsRecord>,
        }
    };
}
crate::sports::core_sports!(define_sport_records);

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

/// `DEVPAIR#<code>` / `#META` — a one-time code linking a device with no
/// practical login UI of its own (a Garmin watch, initially) to a user's
/// account. See `dao::device_pairing` for the claim flow.
///
/// `device_sub` is the identity-provider-style subject reserved for this
/// device *before* pairing succeeds — claiming the code creates its
/// `AUTH#<device_sub>` guard mapping straight to `user_id`, the same shape
/// as any other login provider's `sub` (see `AuthGuardRecord`), so every
/// existing uid-resolution path handles a device exactly like a second login
/// method on the same account, no special-casing required.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DevicePairingRecord {
    pub code: String,
    pub user_id: String,
    pub device_sub: String,
    pub created_at: String,
    pub expires_at: String,
    /// Set once the device has claimed the code. A code is single-use:
    /// `claim_device_pairing_code` only succeeds while this is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
}

/// `USER#<id>` / `#PROFILE` — the user profile item.
///
/// Counts are denormalized and maintained via atomic `ADD` (see follow ops).
/// `email` is duplicated here for reads; uniqueness is enforced by a separate
/// `EMAIL#<email>` guard item. `stats` holds per-sport aggregates inline, so a
/// profile read/batch-read returns everything in one point read — always
/// present (all `None` for a brand new user).
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
    pub stats: UserStatsRecord,
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
    pub logo_url: Option<String>,
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
/// counts. `players`, the live-scoring score record, submissions, likes and
/// comments live as separate items in the same partition; `sides` is
/// embedded directly here (not a separate `SIDE#` item per side, unlike
/// those) — a match never has more than a handful, they're never added,
/// removed, or reordered after creation, and embedding them means a page's
/// worth of matches' sides ride along for free on the same `BatchGetItem`
/// that already fetches their metas, instead of one `Query` per match (see
/// `Dao::batch_get_match_summaries`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchRecord {
    pub id: String,
    /// The user who created (organizes) the match. Immutable — a historical
    /// fact, unrelated to the transferable `Owner` role on `players` below
    /// (see `MatchPlayerRole`). Still checked directly by
    /// `caller_can_manage_match`/`caller_is_match_admin` as a stopgap for a
    /// creator who organizes without playing — they'd otherwise have no
    /// roster row to carry any role at all. A future "non-player organizers"
    /// list (tracked separately, not yet built) is the real fix for that
    /// case; this field isn't it, just today's fallback.
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
    /// Whether a self-serve joiner (a join link, or a team member via
    /// `MatchSideRecord::team_join_enabled`) may ever land unassigned rather
    /// than on a specific side. A hard ceiling, not a
    /// default: a join link's own `allow_unassigned` is ANDed with this one
    /// (see `join_scope_from_link`) — if the match says no, no link can
    /// offer it regardless of its own setting. `#[serde(default =
    /// "default_true")]` for records written before this field existed,
    /// matching the implicit behavior invites already had (an invite's
    /// `side_id` was always optional).
    #[serde(default = "default_true")]
    pub allow_unassigned: bool,
    /// Total roster size (every side plus unassigned), maintained atomically
    /// alongside each side's own `player_count` — see `Dao::join_match_tx`
    /// and `MatchAggregate::effective_max_players` for how it's used to
    /// enforce the derived overall cap. `#[serde(default)]` for records
    /// written before this field existed.
    #[serde(default)]
    pub total_player_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRecord>,
    /// This match's sides, keyed by `side_id` — a DynamoDB map, not a list,
    /// so a single side's `player_count`/`roster_preview` can be updated in
    /// place by key (`Dao::refresh_side_roster_previews`) without needing to
    /// know or preserve a position. Every match has this populated by
    /// `create_match`; there is no fallback to a separate `SIDE#` item
    /// collection (that storage predates this field and has been migrated
    /// away — see the migration script, not checked into this repo).
    pub sides: std::collections::HashMap<String, MatchSideRecord>,
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
    ///
    /// A monotonically-*increasing* reservation counter, not a mirror of the
    /// physical log's max seq once a `LIVEEVT#` has ever been deleted — see
    /// `live_tip_seq` for that instead.
    #[serde(default)]
    pub live_seq: u32,
    /// The seq of the log's current physical tip — the highest `LIVEEVT#`
    /// that actually exists — `None` if the log is empty. DAO-internal
    /// bookkeeping for `Dao::delete_live_event`'s "only the tip can be
    /// undone" guard; nothing outside `live_score_ops.rs` reads or writes
    /// this directly (it's set via raw attribute updates there, not through
    /// this struct). Deliberately a second field rather than reusing
    /// `live_seq` for both jobs: `live_seq` only ever climbs (bumped by
    /// every append *and* every delete, so appends never reuse a seq), so it
    /// stops equaling the physical tip's own seq the moment a delete ever
    /// bumps it past one — `live_tip_seq` is what stays accurate across
    /// consecutive deletes. `#[serde(default)]` covers both matches written
    /// before live scoring existed and matches whose log predates this
    /// field: for either, the first `delete_live_event` call falls back to
    /// checking `live_seq` instead (see that method).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_tip_seq: Option<u32>,
    /// Match format/rules configuration (overs per innings, half length, and
    /// so on). Embedded directly on the match record (not a separate item,
    /// unlike the live-scoring score record) because live scoring wants it
    /// on the same fetch as everything else. `None` until a format is
    /// configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<MatchFormatRecord>,
    pub created_at: String,
}

// `MatchFormatRecord` is generated by `define_sport_records!` above.

/// Shared by `MatchRecord::allow_unassigned`, `JoinLinkScopeRecord::allow_unassigned`
/// and `crate::sports::cricket::CricketFormatRecord`'s extra-ball flags — not
/// cricket- or match-specific, so it stays here rather than moving into any
/// one sport module.
pub(crate) fn default_true() -> bool {
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
    /// Cap on this side's roster. `None` = uncapped. When every side of a
    /// match has one set, the match's overall cap is derived as their sum
    /// (see `MatchAggregate::effective_max_players`) rather than stored
    /// separately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_players: Option<u32>,
    /// Whether an accepted member of this side's `team_id` may join it
    /// directly, with no invite or join link — the match admin's per-side
    /// opt-in for team self-join. Meaningless without a `team_id`.
    #[serde(default)]
    pub team_join_enabled: bool,
    /// Total players currently on this side. Denormalized alongside
    /// `roster_preview` (kept in sync on every roster-changing write — see
    /// `Dao::refresh_side_roster_previews`) so the feed can decide "show
    /// players" vs "show team" without a live players query.
    /// `#[serde(default)]` for items written before this field existed.
    #[serde(default)]
    pub player_count: u32,
    /// This side's *entire* roster, cached — but only when it fits within
    /// `ROSTER_PREVIEW_CAP`. When `player_count` exceeds the cap this is
    /// empty: a partial peek ("3 of 11") isn't useful to show, so callers
    /// should fall back to `team_id`/`name` instead. Not live — a snapshot as
    /// of the last roster-changing write.
    #[serde(default)]
    pub roster_preview: Vec<SideRosterMemberRecord>,
}

/// One player in a side's cached `roster_preview` — just enough to resolve a
/// live `Member` at read time. `user_id` is looked up fresh (name/avatar
/// hydrated via `batch_get_users`, same as everywhere else) so a stale cache
/// never shows an outdated photo; `display_name` is stored directly for an
/// external (unlinked) player, same as `MatchPlayerRecord`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SideRosterMemberRecord {
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
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
    /// This player's authority on the match — own enum, deliberately not a
    /// reuse of `TeamMemberRecord.role`, since match and team roles may
    /// diverge over time. `#[serde(default)]` (-> `Player`) for players
    /// written before this field existed.
    #[serde(default)]
    pub role: MatchPlayerRole,
    /// How this player got onto the roster: `None` means added directly by
    /// the organizer/an admin (same as today), `Some` means they added
    /// themselves — via a join link or team self-join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_via: Option<JoinSourceRecord>,
}

/// A player's authority on a match. Kept as its own type rather than reusing
/// `TeamMemberRecord.role` (a raw "admin"/"member" string) — match and team
/// roles are conceptually related but not guaranteed to stay identical, and a
/// shared type would couple them accidentally.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchPlayerRole {
    /// The match's owner — permanently so, in the sense that there's exactly
    /// one at a time, transferable via `Dao::transfer_match_ownership` (which
    /// atomically demotes the outgoing owner to `Admin`, mirroring
    /// `Dao::transfer_team_ownership`). The playing creator gets this by
    /// default. A non-playing organizer has no roster row and so can't hold
    /// it today — `MatchRecord::created_by_user_id` is the stopgap for that
    /// case (see its doc comment) until a non-player-organizers list exists.
    Owner,
    /// Full authority over the match short of transferring ownership: manage
    /// `join_policy`/caps, mint or revoke join-links, invite people.
    Admin,
    /// An ordinary roster member. Can still invite named people (today's
    /// "any participant" behavior), just not the more structural actions
    /// above.
    #[default]
    Player,
}

/// How a player joined the match, when it wasn't the organizer adding them
/// directly (`MatchPlayerRecord.joined_via: None` covers that existing case).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JoinSourceRecord {
    /// Joined via a shareable `JoinLinkRecord`.
    Link { link_id: String },
    /// Joined via team self-join (`MatchSideRecord::team_join_enabled`) — no
    /// link involved.
    SelfServe,
}

/// `MATCH#<matchId>` / `SCORE#<sport>` — the match's live-scoring score
/// record: live or finished, the same record either way (see
/// `agon_service`'s `Score` doc comment). A match being scored live keeps
/// this up to date incrementally, one event at a time; a manually-entered
/// match (or one past its last live event) just has it written directly.
/// Same `ScoreRecord` type as `Match.confirmed_score`/`pending_score` — kept
/// as a separate item (rather than written straight to the match's `#META`
/// item) purely for write-frequency and stream-isolation reasons: this
/// record can be rewritten on every single live event without touching the
/// `#META` item every DynamoDB-stream consumer (search reindexing, feed
/// fan-out) reacts to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchScoreRecord {
    pub sport: String,
    pub score: ScoreRecord,
    /// The seq of the last live event folded into `score`, if any — `None`
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
/// (see `LiveEventPayloadRecord` below). The event log is the actively-growing, most
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

// `LiveEventPayloadRecord` is generated by `define_sport_records!` above.
// Each sport's live-event record tree (`FootballLiveEventRecord`,
// `CricketLiveEventRecord`, `NetballLiveEventRecord`, and their nested types)
// lives in `crate::sports::{football,cricket,netball}` alongside that sport's
// score/format records.

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

/// `JOINLINK#<linkId>` / `#META` — a shareable, many-use join link. Unlike
/// `InvitationRecord`'s token (single-use — pre-bound to one specific roster
/// row), any number of different people may join via the same link's token,
/// bounded only by the target's own capacity (see `Dao::join_match_tx`).
///
/// Standalone and context-tagged the same way `InvitationRecord` is (reusing
/// `InvitationContextRecord` as-is) rather than embedded under `MATCH#`/
/// `TEAM#`, so the same entity serves a match join-link today and a team
/// join-link later without a new type or a migration.
///
/// Projects to GSI2 as `JOINLINK_TOKEN#<token>` — a distinct prefix from
/// `InvitationRecord`'s `TOKEN#` so the two entities never collide despite
/// sharing the index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JoinLinkRecord {
    pub id: String,
    pub token: String,
    /// What this link joins (currently always `Match`; `Team` is free for a
    /// later team-join-link feature).
    pub context: InvitationContextRecord,
    /// Which side(s)/unassigned this link may join. Match-join-specific
    /// today — a team join-link would carry its own, simpler scope type.
    pub scope: JoinLinkScopeRecord,
    pub created_by_user_id: String,
    pub created_at: String,
    /// Soft-revoke, not delete: keeps past joiners' provenance
    /// (`MatchPlayerRecord.joined_via`) resolvable. A revoked link 404s on
    /// lookup-to-join; its row otherwise remains as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

/// See `JoinLinkRecord::scope` — this link's own join rule, independent of
/// any other link on the same match. `None` `side_ids` means any of the
/// match's sides is fine; `Some(vec![])` means none are (the link only ever
/// lands unassigned); `Some([id])` auto-assigns that one side (no choice to
/// make); `Some([...])` (2+) lets the joiner pick among just those.
/// `allow_unassigned` is this link's own preference for whether landing
/// unassigned is also on offer alongside whatever `side_ids` allows — always
/// capped by the match's own `MatchRecord::allow_unassigned` at resolve time
/// (see `join_scope_from_link`), so a link can only ever be *more*
/// restrictive than the match, never less.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct JoinLinkScopeRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_ids: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub allow_unassigned: bool,
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
    /// A match now has a side your team may join directly
    /// (`MatchSideRecord::team_join_enabled`) — sent to every accepted member
    /// of that team not already on the roster, whether because the match was
    /// just created that way or an organizer just turned the setting on for
    /// an existing one. `actor_user_id` is the match's organizer
    /// (`MatchRecord::created_by_user_id`); there's no more specific "who
    /// flipped the toggle" actor available from the stream event alone.
    TeamMatchJoinable {
        actor_user_id: String,
        team_id: String,
        team_name: String,
        match_id: String,
        match_name: String,
    },
}

/// The client platform a registered push token belongs to. Distinguishes how
/// a device's token is expected to behave (e.g. web tokens can go stale on
/// service-worker reinstall) rather than changing the send path itself — FCM
/// HTTP v1 accepts all three the same way.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Web,
    Android,
    Ios,
}

/// `USER#<uid>` / `DEVICE#<token>` — a registered push destination.
///
/// The FCM registration token is the key value itself, so re-registering the
/// same token (e.g. on every app open) is a plain upsert — no separate id
/// layer, no conditional guard needed.
///
/// Future: a per-user (or per-user + notification kind / per-followed team)
/// mute preference would be looked up in the worker's push handler right
/// before the send loop — additive, doesn't change this record shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRecord {
    pub user_id: String,
    pub push_token: String,
    pub platform: DevicePlatform,
    pub created_at: String,
}

/// `USER#<uid>` / `PAIREDDEV#<device_sub>` — a device paired to this account
/// via `dao::device_pairing` (a Garmin watch, initially). Written once,
/// alongside the `AUTH#<device_sub>` guard, when a pairing code is claimed;
/// exists purely so the account owner can see what's paired and revoke it —
/// see `dao::paired_device`. Not the credential itself (that's the auth
/// guard); this is just the human-facing record of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairedDeviceRecord {
    pub user_id: String,
    pub device_sub: String,
    /// When the pairing was claimed (the device's first successful
    /// `POST /devices/pair`, not when the code was confirmed in the
    /// browser — see `dao::device_pairing`'s doc comment on that
    /// distinction).
    pub paired_at: String,
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
    /// "profile_image" | "team_logo" | "match_header".
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
    /// Up to `MAX_KNOWN_PLAYERS` user ids of this match's participants that the
    /// viewer follows — "people you know are playing", denormalized at fan-out
    /// time so a feed read never queries the match's player collection.
    /// Snapshot, not live: it reflects the audience computation's state as of
    /// the last fan-out (match creation, or an accepted invitation re-running
    /// it), not subsequent follow/unfollow activity. `#[serde(default)]` for
    /// feed items written before this field existed.
    #[serde(default)]
    pub known_player_ids: Vec<String>,
    /// How many of the match's participants the viewer follows, in total —
    /// unlike `known_player_ids`, never capped at `MAX_KNOWN_PLAYERS`, so the
    /// feed can render "+N more" beyond the hydrated list. Same snapshot/
    /// refresh characteristics as `known_player_ids`. `#[serde(default)]` for
    /// feed items written before this field existed (back-fills to 0, which
    /// undercounts pre-existing rows until their next fan-out re-run).
    #[serde(default)]
    pub known_player_count: u32,
    /// The side *this viewer* plays on, if they're themselves a participant
    /// in the match — lets their own feed card show the score confirm/dispute
    /// prompt without a live player query. `None` for a viewer in the
    /// audience only via a follow (they're not playing) or a not-yet-assigned
    /// participant. Same snapshot/refresh characteristics as
    /// `known_player_ids`. `#[serde(default)]` for feed items written before
    /// this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_side_id: Option<String>,
}

/// Lifetime counters common to every sport. Also the full shape for a sport
/// with no richer per-player box score to derive extras from (tennis,
/// badminton, squash, table tennis, "other") — each fully-modeled sport's own
/// stats record (`crate::sports::<sport>`) flattens this in and adds its own
/// fields on top, the DAO-side mirror of the API's `GenericPlayerStats`/
/// `#[oai(flatten)]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GenericSportStatsRecord {
    // Every counter below is `#[serde(default)]`: `stats_delta` only ever
    // `ADD`s a counter that's actually nonzero for a given match (see
    // `Dao::ensure_stats_sport`), so e.g. a player who has only ever won
    // never gets a `draws`/`losses` attribute written at all — the map in
    // DynamoDB is sparse by design, not just possibly stale data from before
    // a field was added.
    #[serde(default)]
    pub matches_played: u64,
    #[serde(default)]
    pub wins: u64,
    #[serde(default)]
    pub draws: u64,
    #[serde(default)]
    pub losses: u64,
    // Win percentage is derived (wins / matches_played) at the API layer.
}

/// A personal-best single-match value for one counter, plus the match it was
/// set in. See `Dao::update_best_figures` for why this only ever ratchets up.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BestFigureRecord {
    pub value: u64,
    pub match_id: String,
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
    /// 1 while the match completed with no winner (both sides tied); 0
    /// otherwise. Mutually exclusive with `won`/`lost`.
    pub drawn: u64,
    /// 1 while the match completed and the user's side was not the winner
    /// and it wasn't a draw; 0 otherwise.
    pub lost: u64,
    /// Sport-specific counters this match contributed for this user (e.g.
    /// cricket runs/wickets, football goals/assists) — empty when `played`
    /// is 0 or the sport has no per-player box score to derive them from.
    #[serde(default)]
    pub counters: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;

    /// `Simple`/`Sets` `entries` round-trip through the side_id-keyed map
    /// shape. (Data written before this shape landed no longer needs
    /// handling here — see `migrate_score_entries.py`, the one-off script
    /// that backfilled every match's `confirmed_score`/`pending_score`/
    /// score-submission history to this shape.)
    #[test]
    fn simple_map_shape_deserializes() {
        let map_shape = AttributeValue::M(HashMap::from([
            ("sideA".to_string(), AttributeValue::N("6".into())),
            ("sideB".to_string(), AttributeValue::N("3".into())),
        ]));
        let score_av = AttributeValue::M(HashMap::from([
            ("type".to_string(), AttributeValue::S("simple".into())),
            ("entries".to_string(), map_shape),
        ]));
        let rec: ScoreRecord = serde_dynamo::from_attribute_value(score_av).unwrap();
        match rec {
            ScoreRecord::Simple { entries } => {
                assert_eq!(entries.get("sideA"), Some(&6));
                assert_eq!(entries.get("sideB"), Some(&3));
            }
            _ => panic!("expected simple"),
        }
    }

    #[test]
    fn sets_map_shape_deserializes() {
        let map_shape = AttributeValue::M(HashMap::from([(
            "sideA".to_string(),
            AttributeValue::L(vec![
                AttributeValue::N("6".into()),
                AttributeValue::N("4".into()),
            ]),
        )]));
        let score_av = AttributeValue::M(HashMap::from([
            ("type".to_string(), AttributeValue::S("sets".into())),
            ("entries".to_string(), map_shape),
        ]));
        let rec: ScoreRecord = serde_dynamo::from_attribute_value(score_av).unwrap();
        match rec {
            ScoreRecord::Sets { entries } => {
                assert_eq!(entries.get("sideA"), Some(&vec![6, 4]));
            }
            _ => panic!("expected sets"),
        }
    }
}
