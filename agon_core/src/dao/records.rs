//! DAO record structs — the shapes stored in DynamoDB.
//!
//! Deliberately separate from the API (`poem-openapi`) models: the DAO owns its
//! persistence shape and the API layer maps to/from these. Records hold data
//! fields only; keys/GSI attributes are stamped by the `item` layer.

use std::collections::HashMap;

use crate::sports::SportRecord;
use serde::{Deserialize, Serialize};

// ===========================================================================
// Shared nested value types (DAO-owned; never the API's poem-openapi types).
// These are the structural blobs embedded within items — stored as nested
// DynamoDB maps, typed here for safety rather than as `serde_json::Value`.
// ===========================================================================

/// Where a match is played. `text` is the always-present display label — a
/// free-typed description ("Pitch 5, Leather Lane") or, once a Google Places
/// suggestion was picked, that place's formatted address. `place_id` and the
/// coordinates are only ever set together, when a suggestion was picked;
/// free text alone leaves both `None` — a maps query built from text alone
/// isn't reliable enough to point at the right place, so a client only
/// offers a "get directions" link once one of those is present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocationRecord {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    /// Google Place ID, when this location was resolved to a real place —
    /// lets a directions/map link point at that specific place's own listing
    /// rather than just a coordinate pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    /// The picked place's formatted address, kept so it can still be shown
    /// after `text` is renamed (e.g. "Pitch 2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// One header photo attached to a match: the asset it was uploaded as (so a
/// later edit can re-include, reorder, or mix it with newly uploaded photos)
/// plus its canonical serving URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderPhotoRecord {
    pub asset_id: String,
    pub url: String,
}

/// Every sport-shaped record aggregate, generated from the sport registry
/// (`crate::for_each_sport!`): one variant or field per modeled sport, holding
/// that sport's own record type from its `SportRecord` impl.
macro_rules! define_sport_records {
    (
        modeled: [$( $variant:ident { tag: $tag:literal, key: $key:ident } ),+ $(,)?],
        generic: [$( $gvariant:ident { tag: $gtag:literal, key: $gkey:ident, label: $glabel:literal } ),* $(,)?] $(,)?
    ) => {
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
            $( $variant(<crate::sports::$key::$variant as SportRecord>::ScoreRecord), )+
        }

        /// A match's configured format, discriminated by sport.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "sport", rename_all = "snake_case")]
        pub enum MatchFormatRecord {
            $( $variant(<crate::sports::$key::$variant as SportRecord>::FormatRecord), )+
        }

        /// One stored live-scoring event, discriminated by sport.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "sport", rename_all = "snake_case")]
        pub enum LiveEventPayloadRecord {
            $( $variant(<crate::sports::$key::$variant as SportRecord>::LiveEventRecord), )+
        }

        /// A user's lifetime stats, one field per sport — `None` for a sport
        /// they've never played a confirmed match in. Stored inline on
        /// `UserRecord::stats`, so each sport is a nested map attribute
        /// (`stats.cricket`, `stats.football`, ...).
        ///
        /// The DAO's write path (`Dao::stats_delta`/`update_best_figures`)
        /// doesn't use this type: it addresses `stats.<sport>.<counter>` with
        /// raw `UpdateItem` expressions built from the sport tag and counter
        /// names, so it is sport-agnostic.
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        pub struct UserStatsRecord {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $key: Option<<crate::sports::$key::$variant as SportRecord>::StatsRecord>,
            )+
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $gkey: Option<GenericSportStatsRecord>,
            )*
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub other: Option<GenericSportStatsRecord>,
        }
    };
}
crate::for_each_sport!(define_sport_records);

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
/// present (all `None` for a brand new user). `ratings` and `rating_opt_ins`
/// ride the same item for the same reason.
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
    /// Per-ladder ratings, keyed by the `rating::Ladder` string (`"squash"`
    /// today, `"tennis:doubles"` if a sport is ever split by format).
    /// `#[serde(default)]` because every profile written before ratings
    /// existed has no `ratings` attribute at all — and an empty map is exactly
    /// right for them: nothing rated.
    ///
    /// Sits *beside* `stats` rather than inside it, and is a map where
    /// `UserStatsRecord` uses a named field per sport. Both of those are
    /// deliberate, and the second one is a deliberate inconsistency, so:
    ///
    /// - **Beside**, because the two write paths have nothing in common. Stats
    ///   are integer counters moved by raw `ADD` deltas
    ///   (`Dao::stats_delta`); μ and σ are floats the engine *sets* wholesale
    ///   from its own output — there is no delta to add. Sharing an attribute
    ///   would drag rating writes through counter machinery they cannot use,
    ///   and buy nothing on the read side: both ride the same point read that
    ///   profile, feed and search hydration already do either way.
    /// - **A map**, because `UserStatsRecord`'s own doc comment justifies its
    ///   named fields with "the set of sports is closed". The set of
    ///   *ladders* is deliberately open — that a ladder is a string and not
    ///   the `Sport` enum is the whole mechanism by which a later
    ///   singles/doubles split is additive instead of a migration of every
    ///   stored rating (see `rating::Ladder`). A closed struct here would
    ///   re-close exactly what that decision opened.
    ///
    /// A ladder appears here only once a ranked match on it has been rated,
    /// so being opted in (`rating_opt_ins`) with no entry yet is normal. The
    /// reverse — a rating with no opt-in behind it — is what the rating
    /// pipeline must refuse to produce, by re-checking every participant's
    /// opt-in before it rates; nothing at this layer enforces it.
    #[serde(default)]
    pub ratings: HashMap<String, RatingRecord>,
    /// The ladders this account has consented to be rated on, each mapped to
    /// when it did so (RFC 3339). Written by `Dao::opt_in_to_rating` when a
    /// player ticks "I understand this starts my <sport> rating" on a ranked
    /// game; read through [`UserRecord::has_opted_in`].
    ///
    /// **Entries are never removed.** Opting in is not reversible — a product
    /// decision, not an omission, and the reason there is no opt-out
    /// operation — so a ladder seen here once can be relied on from then on.
    ///
    /// A map to the consent time rather than a set of ladder names. A
    /// DynamoDB string set would be the simpler write (`ADD` creates a missing
    /// set, so there would be no ensure-the-map round trip), but it throws
    /// away *when* somebody agreed, which is the one fact a consent record
    /// exists to keep. The map also makes "first tick wins" a single
    /// `attribute_not_exists` condition on the ladder's path, so ticking the
    /// box again on a later game never moves the original date.
    ///
    /// Keyed by ladder, like `ratings`, and today a ladder is exactly the
    /// sport tag. If a sport is ever split into several ladders, whether one
    /// consent covers all of them is for that change to decide; this shape
    /// does not settle it.
    ///
    /// `#[serde(default)]` because no profile written before this field
    /// existed has it, and an empty map is right for every one of them:
    /// nobody could opt in before there was anywhere to record it.
    #[serde(default)]
    pub rating_opt_ins: HashMap<String, String>,
    pub created_at: String,
}

impl UserRecord {
    /// Whether this account has opted in to being rated on `ladder` — the
    /// check every path into a ranked game makes.
    ///
    /// An exact-key lookup. Ladder names can be string prefixes of one
    /// another (`"tennis"`, `"tennis:doubles"`), and a prefix match would
    /// quietly decide the open question on `rating_opt_ins` of whether one
    /// consent covers a split sport.
    #[must_use]
    pub fn has_opted_in(&self, ladder: &str) -> bool {
        self.rating_opt_ins.contains_key(ladder)
    }
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
    /// fact, unrelated to the transferable `Owner`/`Admin` roles in
    /// `authority` (see `MatchPlayerRole`). Every match now gets the creator
    /// an explicit `Owner` entry in `authority` at creation, whether or not
    /// they play — this field is kept only as a last-resort fallback (in
    /// `caller_match_authority_role`) for matches written before `authority`
    /// existed, which have no entries there at all. `#[serde(default)]` for
    /// records written before this field existed.
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
    /// How many players take a spot on the match — every side plus
    /// unassigned, counting only `MatchPlayerRecord::occupies_slot` players
    /// (pending and declined invitees hold no spot). `Dao::join_match_tx`
    /// bumps it atomically, cap-guarded, as a join lands; every other
    /// roster-changing write (accept, add/remove/reassign) recomputes it from
    /// the roster via `Dao::refresh_side_roster_previews` — the only thing
    /// that brings it back down after a removal. See
    /// `MatchAggregate::effective_max_players` for how it enforces the
    /// derived overall cap. `#[serde(default)]` for records written before
    /// this field existed.
    #[serde(default)]
    pub total_player_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRecord>,
    /// `None` for an ordinary match; `Some(series_id)` for one generated as
    /// part of a recurring match series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
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
    /// Whether this match counts towards its players' ratings; `false` is a
    /// friendly. The organiser's explicit choice — omitted at creation means
    /// friendly (see `CreateMatchInput::ranked` for why, and why creation is
    /// unconstrained).
    ///
    /// `#[serde(default)]` → `false` for matches written before ratings
    /// existed, and it has to stay `false` whatever the product default ever
    /// becomes. Ratings are computed from a match's `#META` stream image, and
    /// that item is rewritten by far more than an edit: every like and every
    /// comment `ADD`s a counter on it. A `true` default would enrol the whole
    /// back catalogue into the ladders, one old match per like, rating
    /// players who never opted in — when opting in is the one thing a rating
    /// requires. Please don't "fix" this to match a create-time default.
    ///
    /// A ranked match always has a rating ladder (`rating::ladder_for_tag` on
    /// `match_type`), so sport `other` is never ranked. The API enforces that
    /// rather than this record, and it can't be undone later because nothing
    /// changes `match_type` after creation. The flag locks against *change*
    /// once a score is submitted, the match starts, or it leaves `scheduled` —
    /// see `ranked_lock_reason` in `agon_service`'s mapping.
    #[serde(default)]
    pub ranked: bool,
    /// The outstanding switch-to-ranked ask, if this match has one — see
    /// [`MatchRankedPromptRecord`]. `None` on every match that was created
    /// ranked, or friendly and left alone, which is nearly all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranked_prompt: Option<MatchRankedPromptRecord>,
    /// Who holds `Owner`/`Admin` authority over this match, keyed by user id
    /// — an ordinary player (or an organiser with no roster row at all) is
    /// simply absent from the map, never stored as an explicit `Player`
    /// entry. Embedded directly on `#META` (same reasoning as `sides`): it's
    /// tiny (almost always just the one owner), changes rarely (creation, a
    /// transfer, an occasional admin grant/revoke), and this way authority
    /// and participation are two genuinely independent concerns — a roster
    /// row (`MatchPlayerRecord`) only ever has to represent "who's playing",
    /// never "who's in charge", so no roster reader (stats/rating
    /// contribution, feed fan-out, headcounts) needs to know this map
    /// exists. A non-playing organiser is just a user id in this map with no
    /// corresponding `PLAYER#` row — there's no separate record type for
    /// them. `#[serde(default)]` for matches written before this field
    /// existed, which have no entries here at all;
    /// `caller_match_authority_role` falls back to treating
    /// `created_by_user_id` as `Owner` for exactly that case.
    #[serde(default)]
    pub authority: std::collections::HashMap<String, MatchPlayerRole>,
    pub created_at: String,
}

fn default_series_timezone() -> String {
    "UTC".to_string()
}

/// The outstanding switch-to-ranked ask on a match
/// ([`MatchRecord::ranked_prompt`]): one per match, covering every player it
/// asked.
///
/// One nested struct rather than a handful of sibling fields on `MatchRecord`,
/// so that a deadline, a counter and the id they belong to cannot drift apart:
/// clearing the ask is one `REMOVE`, and a half-written ask is a parse failure
/// rather than a match that expires people against a stale deadline.
///
/// `ranked` is still the flag everything else reads. This record says only
/// "some of the players in a spot haven't agreed yet", and a match carrying it
/// is already `ranked: true` — the switch takes effect immediately and the
/// prompts chase the consent, which is what keeps everyone's place.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchRankedPromptRecord {
    /// Quoted by every [`RankedPromptRecord::prompt_id`] this ask wrote, and
    /// by every answer to it. A fresh id per switch is what makes a stale
    /// answer recognisable.
    pub id: String,
    /// The ladder being consented to, as of the ask.
    pub ladder: String,
    pub opened_at: String,
    pub opened_by_user_id: String,
    /// When unanswered prompts expire, and with them the places they were
    /// holding. Host-set or defaulted to 24h before the start, and always
    /// within `(opened_at, starts_at]` — the service clamps it, because an
    /// expiry after kickoff would decide a rating question with the result
    /// already known.
    ///
    /// Also the `GSI3SK` of this match's meta item while the ask is open (see
    /// [`Self::DUE_GSI3PK`]), which is how the sweep finds matches whose
    /// deadline has passed without scanning the table.
    pub deadline: String,
    /// How many roster rows still carry an unanswered prompt for [`Self::id`].
    ///
    /// Maintained by atomic `ADD ranked_prompt.pending :delta` in the same
    /// transaction as the row it counts — the same nested-path counter pattern
    /// as `sides.<sideId>.player_count` — and never recomputed from a scan.
    ///
    /// Advisory: it exists so the feed, which reads a match's meta without its
    /// roster, can render "3 still to confirm" without a second read, and so
    /// the sweep can skip a match in one field. The authoritative question is
    /// always whether any row in a spot still
    /// [awaits this ask](MatchPlayerRecord::awaits_ranked_prompt), so a drifted
    /// counter costs a wrong subtitle, never a wrongly expired player.
    #[serde(default)]
    pub pending: u32,
}

impl MatchRankedPromptRecord {
    /// The `GSI3PK` stamped on a match's meta item while an ask is open, with
    /// [`Self::deadline`] as the `GSI3SK`. Both attributes are removed when
    /// the ask is answered out, expired or withdrawn, so the index is sparse:
    /// only matches with something outstanding are in it.
    ///
    /// A constant value, so every pending ask shares one index partition. At
    /// the scale this serves — matches with an unanswered prompt, which is a
    /// subset of matches not yet played — that is a feature: the sweep's whole
    /// job is one `Query` for `GSI3SK <= now`, which costs about half an RCU
    /// and usually returns nothing. Bucket the value by date if that ever
    /// stops being true.
    pub const DUE_GSI3PK: &'static str = "RANKEDPROMPTDUE";
}

/// `SERIES#<seriesId>` / `#META` — a recurring match series' template and
/// generation state. Every occurrence is generated as its own independent
/// `MatchRecord` (own id, own roster, own score), linked back only by
/// `MatchRecord::series_id` — this record is never read by, nor joined
/// against, the matches it produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeriesRecord {
    pub id: String,
    /// The user who created (organizes) the series. Mirrors
    /// `MatchRecord::created_by_user_id`.
    pub created_by_user_id: String,
    /// Sport tag, same meaning as `MatchRecord::match_type`.
    pub match_type: String,
    /// Template value each generated occurrence starts from.
    pub name: String,
    /// Template value each generated occurrence starts from.
    pub description: String,
    /// Template value each generated occurrence starts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRecord>,
    /// How often an occurrence recurs: "weekly" | "fortnightly" | "monthly" |
    /// "yearly".
    pub frequency: String,
    /// The recurrence day: 0 = Sunday .. 6 = Saturday.
    pub weekday: u8,
    /// The first occurrence's `starts_at` (RFC-3339), copied from that
    /// match at series-creation time. Not surfaced on the API's own
    /// `MatchSeries` read model (nothing there needs it — the first
    /// occurrence is itself an ordinary, independently-fetchable match) —
    /// this exists purely as the anchor `materialize_series` advances from
    /// (via `agon_core::series::occurrence_date`) to compute every later
    /// occurrence's date, and so it never has to look the first occurrence
    /// up by some other means to find it.
    pub first_occurrence_starts_at: String,
    /// The IANA timezone (e.g. `Europe/London`) `first_occurrence_starts_at`
    /// was picked in — `materialize_series` advances occurrences in this
    /// zone's local wall-clock time, not in UTC, so a series' time-of-day
    /// stays fixed across DST transitions instead of drifting by whatever
    /// the zone's offset changes by (see `agon_core::series::occurrence_starts_at`).
    /// Defaults to `"UTC"` when absent so a series created before this field
    /// existed still deserializes — such a series keeps its pre-existing
    /// (DST-unaware) behaviour rather than silently changing its occurrence
    /// times; fixing an already-generated series is a data migration, not
    /// something this default attempts.
    #[serde(default = "default_series_timezone")]
    pub timezone: String,
    /// When the series stops generating new occurrences. `Never` means
    /// indefinite recurrence: `materialize_series` generates occurrences
    /// only a few at a time, keeping a small rolling window materialized
    /// ahead of the series' current progress rather than ever generating
    /// everything upfront — true for a bounded (`OnDate`/`AfterN`) series
    /// too, which just additionally stops once its implied total is
    /// reached.
    pub ends: SeriesEndRecord,
    /// The sides generated for every occurrence of this series. Every
    /// occurrence is generated with these as team-linked,
    /// `team_join_enabled` sides — no ad-hoc named sides and no per-player
    /// invite list in this version of the feature.
    #[serde(default)]
    pub sides: Vec<SeriesSideRecord>,
    /// Lifecycle: "generating" | "active" | "paused" | "ended". "paused"
    /// stops further generation without touching already-created
    /// occurrences — meaningful for a `Never`-ending series, which
    /// otherwise keeps generating indefinitely.
    pub status: String,
    /// How many occurrences have been successfully created so far — lets a
    /// generation workflow resume after a partial failure.
    #[serde(default)]
    pub occurrences_generated: u32,
    pub created_at: String,
}

/// `SERIES#<seriesId>` / `OCCURRENCE#<startsAt>` — a claim on one series
/// occurrence's generation slot. Written by `materialize_series` (and, for
/// the first occurrence, by the series-creation handler) before the
/// occurrence's match is created, via `Dao::claim_series_occurrence`'s
/// conditional put: this is the idempotency guard that lets a retried
/// generation attempt discover the match id an earlier attempt already
/// committed to, rather than every attempt minting its own. Kept around
/// after generation too, as the item collection `Dao::list_series_occurrences`
/// reads to answer "what has this series generated, and when."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeriesOccurrenceRecord {
    pub series_id: String,
    /// 0-based occurrence index — matches the `occurrences_generated` value
    /// this occurrence's generation is conditioned on. Stored for display
    /// only; `starts_at` (the SK) is what actually orders the collection and
    /// what the conditional put is keyed on.
    pub index: u32,
    pub starts_at: String,
    /// The occurrence's match id — whichever attempt's conditional put won
    /// the claim is the one every subsequent read/retry converges on.
    pub match_id: String,
    pub created_at: String,
}

/// A side template for a series' generated occurrences — the minimal subset
/// of `CreateMatchSideInput` needed to recreate team-linked sides on each
/// occurrence. `client_id` is a caller-chosen id scoped to the series (not a
/// real `side_id`, which doesn't exist until an occurrence is created) and is
/// reused as that occurrence's own `CreateMatchSideInput.client_id` at
/// generation time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeriesSideRecord {
    pub client_id: String,
    pub team_id: String,
    /// Only meaningful when two sides in this template share the same
    /// `team_id` (a derby) — same exception `CreateMatchSideInput.name`
    /// documents. `None` otherwise; the linked team is the name's source of
    /// truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Same derby exception as `name`, mirroring `CreateMatchSideInput.colour`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// Cap on this side's roster. `None` = uncapped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_players: Option<u32>,
}

/// Where a [`SeriesRecord`] stops generating new occurrences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeriesEndRecord {
    OnDate { date: String },
    AfterN { count: u32 },
    Never,
}

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
    /// This side's own colour (a hex string, e.g. `"#2952D9"`), for an ad-hoc
    /// side with no `team_id`, or one sharing its `team_id` with another side
    /// (a derby, distinguished the same way a derby's two sides get their
    /// own `name`) — a linked team's own colour (once teams have one) is the
    /// source of truth otherwise. Required by the create-match API for a
    /// side without a team (the client always picks one, inferring it from a
    /// colour name in the side's own name where possible); `#[serde(default)]`
    /// covers sides written before this field existed, which have neither a
    /// colour nor a team either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
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
    /// How many players currently take a spot on this side
    /// (`MatchPlayerRecord::occupies_slot`) — the number `max_players` caps.
    /// Pending and declined invitees placed on the side aren't counted,
    /// though they do still appear in `roster_preview`. Denormalized
    /// alongside `roster_preview` (kept in sync on every roster-changing
    /// write — see `Dao::refresh_side_roster_previews`) so the feed can
    /// decide "show players" vs "show team" without a live players query.
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
    /// How this player got onto the roster: `None` means added directly by
    /// the organizer/an admin (same as today), `Some` means they added
    /// themselves — via a join link or team self-join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_via: Option<JoinSourceRecord>,
    /// Set while this player has been asked to confirm a switch to ranked and
    /// hasn't answered — see [`RankedPromptRecord`].
    ///
    /// **[`occupies_slot`](MatchPlayerRecord::occupies_slot) deliberately does
    /// not look at this field**, and that is the whole point of it. Holding a
    /// player's place while they decide is what makes switching a friendly to
    /// ranked bearable: the alternative is evicting everyone until they
    /// re-consent. A pending *invitation* cannot express it, because a pending
    /// invite takes no spot at all — so the roster row stays exactly as it
    /// was, this field rides alongside it, and the headcounts move only on the
    /// writes that have always moved them (a decline, or the deadline sweep
    /// removing the row).
    ///
    /// Only players who haven't opted in to the match's ladder are prompted.
    /// Someone already opted in is *told* the match now counts and keeps their
    /// place whatever they do, so this never appears on their row: the prompt
    /// exists to collect consent that is missing, not to re-confirm consent
    /// that already exists.
    ///
    /// `#[serde(default)]` → `None` for every row written before prompts
    /// existed, which is the truth about them: nothing was ever asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranked_prompt: Option<RankedPromptRecord>,
}

impl MatchPlayerRecord {
    /// Whether this player takes one of the match's spots: they're *in* —
    /// added by an organiser or self-served via a join link/team (no
    /// invitation), or an invitee who has accepted. A pending or declined
    /// invite takes nothing; sending an invite doesn't reserve a spot.
    ///
    /// A waitlisted player is never represented by a `MatchPlayerRecord` at
    /// all — self-served, their `MatchWaitlistEntryRecord` is the only row;
    /// an invitee who accepts onto the waitlist has their pending roster
    /// placeholder deleted rather than flipped to accepted, in the same
    /// transaction that creates the waitlist entry (see
    /// `Dao::accept_invitation_items`'s `onto_waitlist` branch). So this rule
    /// never needs to cross-reference the waitlist — every row it sees is a
    /// real roster candidate.
    ///
    /// The single source of truth `MatchRecord::total_player_count` and
    /// `MatchSideRecord::player_count` are kept in sync with: every write
    /// that can flip this rule's answer for a row (join, accept, organizer
    /// add/remove/reassign, move-in from the waitlist) applies the matching
    /// atomic `ADD`/`SUBTRACT` delta in the same transaction, rather than
    /// ever recomputing from a full scan — see `match_ops::take_slot_update`
    /// and `match_ops::headcount_delta_update`.
    pub fn occupies_slot(&self) -> bool {
        self.invitation
            .as_ref()
            .is_none_or(|inv| inv.status == "accepted")
    }

    /// Whether this row is still waiting on an answer to `prompt_id` — the
    /// test the respond endpoint and the deadline sweep both make before they
    /// touch a row.
    ///
    /// Scoped to one id on purpose. A match can be flipped friendly → ranked
    /// more than once, and a row left carrying a prompt from a withdrawn
    /// switch must not be mistaken for an answer owed on the current one: the
    /// authoritative question is never "is there a prompt?" but "is there a
    /// prompt for *this* ask?".
    #[must_use]
    pub fn awaits_ranked_prompt(&self, prompt_id: &str) -> bool {
        self.ranked_prompt
            .as_ref()
            .is_some_and(|p| p.prompt_id == prompt_id)
    }
}

/// A player's outstanding "this match is ranked now — confirm, or lose your
/// place" ask, embedded on their roster row
/// ([`MatchPlayerRecord::ranked_prompt`]).
///
/// Embedded rather than given its own `RANKEDPROMPT#<userId>` item range
/// because [`Dao::get_match`](crate::dao::Dao::get_match) is a point read of
/// the meta plus **one** Query of the `PLAYER#` prefix. A separate range would
/// add a third read to every match page view, to carry three fields that only
/// ever matter beside the roster row they hang off.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankedPromptRecord {
    /// The switch that asked, matching [`MatchRankedPromptRecord::id`] on the
    /// match's meta item.
    ///
    /// An answer quoting any other id is stale — the switch was withdrawn, or
    /// the match has been flipped since — and is refused rather than applied.
    /// That is what makes answering idempotent and a re-switch unambiguous,
    /// without either side having to reason about ordering.
    pub prompt_id: String,
    /// The ladder consent is being collected for, as of the ask (e.g.
    /// `"tennis"`). Stored rather than re-derived from the match's
    /// `match_type` so a notification can name the sport without reading the
    /// match, and so a row answered after a `match_type` edit still records
    /// what was actually agreed to.
    pub ladder: String,
    pub asked_at: String,
}

/// `MATCH#<matchId>` / `WAITLIST#<userId>` — someone queued for a match/side
/// that had no room when they tried to join or accept an invite onto it, kept
/// entirely apart from the roster (`MatchPlayerRecord`): a waitlisted person
/// (self-served, or an invitee who accepted onto the waitlist) has no roster
/// row at all until a match admin moves them in (the `move_in_from_waitlist`
/// handler) — at which point this entry is deleted and a fresh roster row is
/// created via the same cap-guarded write `join_match` uses, regardless of
/// how they got onto the waitlist.
///
/// Keyed by `user_id` rather than a minted id — unlike the roster, a waitlist
/// entry always names a real account (only an authenticated caller can join a
/// waitlist, so there's no unlinked-external case to key around), and at most
/// one entry per (match, user) is meaningful anyway. That also makes "already
/// waitlisted" a conditional-put guard on the key itself, the same pattern as
/// every other uniqueness guard in this table (see the module doc comment).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchWaitlistEntryRecord {
    pub user_id: String,
    /// The side they're waiting for, mirroring `JoinMatchInput::side_id` —
    /// `None` for unassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_id: Option<String>,
    /// Display/ordering key — longest-waiting first. Purely informational;
    /// moving someone in is always a manual admin action, never automatic.
    pub waitlisted_at: String,
    /// Set when this entry came from accepting an invite that had no room
    /// (`Dao::waitlist_join_tx` also accepts the invitation in that case, so
    /// it isn't asked again) — kept for traceability. `None` for a plain
    /// self-served waitlist join (join link / team self-join).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation_id: Option<String>,
}

/// A user's authority on a match — the value type of `MatchRecord::authority`
/// (and the API's per-player/`viewer_role` shape). Kept as its own type
/// rather than reusing `TeamMemberRecord.role` (a raw "admin"/"member"
/// string) — match and team roles are conceptually related but not
/// guaranteed to stay identical, and a shared type would couple them
/// accidentally.
///
/// `authority` only ever stores `Owner`/`Admin` entries — an ordinary player
/// is simply absent from the map, never written as an explicit `Player`
/// entry — but the API still reports `Player` for them (e.g.
/// `MatchPlayer.role`, `Match.viewer_role`), so the variant stays for that
/// resolved-but-ordinary case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchPlayerRole {
    /// The match's owner — permanently so, in the sense that there's exactly
    /// one at a time, transferable via `Dao::set_match_authority` (which
    /// atomically demotes the outgoing owner to `Admin` in the same map
    /// update, mirroring `Dao::transfer_team_ownership`). The creator gets
    /// this in `authority` at creation whether or not they play —
    /// `MatchRecord::created_by_user_id` is only a last-resort fallback for
    /// matches that predate the `authority` field.
    Owner,
    /// Full authority over the match short of transferring ownership: manage
    /// `join_policy`/caps, mint or revoke join-links, invite people.
    Admin,
    /// An ordinary roster member, or anyone else with no entry in
    /// `authority`. Can still invite named people (today's "any participant"
    /// behavior), just not the more structural actions above.
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
    /// Moved in off the waitlist by a match admin
    /// (`Dao::move_in_from_waitlist_tx`) once a spot freed up.
    Waitlist,
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
        /// The invitation's live status ("pending"/"accepted"/"declined"),
        /// flipped in place by `Dao::mark_invitation_notification_actioned`
        /// when the invitee responds, so the feed stops offering
        /// Confirm/Decline once it's been actioned. Defaults to "pending" for
        /// notifications written before this field existed.
        #[serde(default = "default_pending_invitation_status")]
        status: String,
    },
    TeamInvitation {
        actor_user_id: String,
        invitation_id: String,
        team_id: String,
        team_name: String,
        /// See `MatchInvitation::status`.
        #[serde(default = "default_pending_invitation_status")]
        status: String,
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

fn default_pending_invitation_status() -> String {
    "pending".to_string()
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
    ///
    /// Do NOT use this to decide "is the viewer going" — an unassigned
    /// participant (a match with `allow_unassigned` whose joiner hasn't
    /// picked a side) has this as `None` too despite playing. Use
    /// `viewer_is_going` for that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_side_id: Option<String>,
    /// Whether this viewer is themselves a participant in the match,
    /// regardless of whether they've been assigned a side yet — see
    /// `viewer_side_id`'s doc comment. Same snapshot/refresh characteristics.
    /// `#[serde(default)]` for feed items written before this field existed
    /// (back-fills to `false`, i.e. "not going", until the next fan-out
    /// re-run refreshes it).
    #[serde(default)]
    pub viewer_is_going: bool,
    /// Whether this viewer has a pending (not yet responded to) invitation
    /// to the match — see `AudienceMember::viewer_invitation_pending`.
    /// `#[serde(default)]` for feed items written before this field existed.
    #[serde(default)]
    pub viewer_invitation_pending: bool,
    /// Side ids this viewer could join directly via team membership — see
    /// `AudienceMember::viewer_can_join_side_ids`. `#[serde(default)]` for
    /// feed items written before this field existed.
    #[serde(default)]
    pub viewer_can_join_side_ids: Vec<String>,
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

// ===========================================================================
// Ratings
// ===========================================================================

/// One player's rating on one ladder, stored inline in the `ratings` map on
/// `USER#<uid>` / `#PROFILE`.
///
/// `mu`/`sigma` are the engine's **native** Weng-Lin values (`μ₀ = 25`,
/// `σ₀ = 8.33`), never the 1500-centred numbers a player reads. Storing
/// native is what makes `rating::scale` retunable by deploy rather than by
/// backfill (see its module doc), and it is also the only form the engine
/// can be fed back for the next match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingRecord {
    /// Mean estimate of skill — `rating::PlayerRating::mu`.
    pub mu: f64,
    /// Standard deviation, i.e. how unsure we are — `rating::PlayerRating::sigma`.
    pub sigma: f64,
    /// How many rated matches have been folded into this rating: the
    /// "· 6 games" shown beside the number from the first rated match on,
    /// and the cheap check that a repair replay covered the same matches the
    /// incremental path did.
    pub matches_rated: u64,
    /// The `starts_at` of the most recent match folded in — when the newest
    /// rated match was **played**, not when it was rated. That is the
    /// comparison the pipeline actually needs: a match arriving with an
    /// earlier `starts_at` than this is precisely the out-of-order case that
    /// triggers repair, and confirmation times can't answer it (a Monday game
    /// is routinely confirmed after a Wednesday one). It is therefore
    /// directly comparable with `Sk::Rating`'s `played_at` segment.
    ///
    /// Written as a *maximum*, never simply as the match just rated. An
    /// out-of-order match is still applied (its history entry is what the
    /// repair reads), and overwriting this with its earlier `starts_at` would
    /// make every later match look out of order too.
    pub last_rated_at: String,
}

/// The before/after pair one rated match moved a player through.
///
/// Shared by [`RatingHistoryRecord`] and [`RatingContributionRecord`] because
/// they record the same event from two directions — "what happened to this
/// player over time" and "what this match did to each participant" — and a
/// single nested map also makes the optimistic-lock guard on the contribution
/// item one `movement = :m` condition instead of five (DynamoDB compares
/// map-typed attributes as a whole; `Dao::reconcile_match_contribution` leans
/// on the same trick for its `counters` map).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RatingMovementRecord {
    pub mu_before: f64,
    pub sigma_before: f64,
    pub mu_after: f64,
    pub sigma_after: f64,
    /// The movement as the player was shown it — the "+18" on a match card
    /// (`rating::RatingUpdate::display_delta`).
    ///
    /// Stored even though it is fully derivable from `mu_before`/`mu_after`,
    /// which reads at first glance like a break from storing native values
    /// only: the displayed rating itself is never stored, and is computed
    /// through `rating::scale` on every read so that retuning the scale moves
    /// every profile at once. The distinction is that a displayed rating is a
    /// statement about the *present*, whereas this is a log entry, and a log
    /// records what actually happened, including what the player was
    /// actually told. If `rating::scale` were ever retuned, a recomputed delta
    /// would quietly rewrite history; this one keeps saying what the match
    /// card said on the day.
    pub display_delta: i32,
}

/// `USER#<uid>` / `RATING#<ladder>#<played_at>#<matchId>` — one match's
/// effect on one player's rating on one ladder.
///
/// Time-ordered by construction (the key sorts on the match's `starts_at`),
/// which is what lets one item collection be two things at once: the replay
/// source a repair pages through in played order, and the rating-over-time
/// chart. Both key fields are duplicated into the item as plain attributes,
/// normal for a single-table design — see `dao::item`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingHistoryRecord {
    /// The ladder this movement happened on (the `rating::Ladder` string).
    pub ladder: String,
    pub match_id: String,
    /// The match's `starts_at` — the key's ordering segment. See
    /// `Sk::Rating` on why played order and not confirmation order.
    pub played_at: String,
    pub movement: RatingMovementRecord,
    /// Wall clock at the moment this was applied. Distinct from `played_at`,
    /// and worth keeping alongside it: the gap between the two is exactly how
    /// far out of order a result arrived, which is the first thing anyone
    /// debugging an unexpected repair will want.
    pub applied_at: String,
}

/// `MATCH#<mid>` / `RATINGCONTRIB#<userId>` — what this match currently
/// contributes to one player's rating. Absent => the match has never been
/// rated for this player.
///
/// The direct analogue of [`StatContributionRecord`], and it earns its keep
/// the same way: the pipeline compares the contribution the match's current
/// state implies against this stored one, so an unchanged redelivery writes
/// nothing and a re-score is detected as a change rather than applied twice.
///
/// It carries more than the bare delta on purpose. `side_id` and the rating
/// each player carried *into* the match (`movement`'s `before` half) are what
/// change detection re-rates from: a match's `RATINGCONTRIB#` collection
/// keeps the beliefs every participant brought to it, which their current
/// ratings — having since absorbed later matches — no longer can. See
/// `dao::rating`'s module doc for the double count that avoids. It is not a
/// stand-in for the match itself, though: the current winner and roster
/// still come from the match record, because a re-score or a roster edit is
/// exactly the change being looked for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingContributionRecord {
    /// The player's user id — the same value as the sort key's segment,
    /// carried in the item so `Dao::list_rating_contributions` can return
    /// whole records without parsing keys.
    pub user_id: String,
    /// The ladder the contribution counted under (the match's ladder at the
    /// time it was applied). Kept for the same reason
    /// `StatContributionRecord::match_type` is: if a match's sport is edited,
    /// the contribution has to be backed out of the *old* ladder.
    pub ladder: String,
    /// Which side this player played for. A roster edit that moves someone
    /// across changes it, which is what makes that edit detectable.
    pub side_id: String,
    /// The match's `starts_at`. Duplicated from the match record so that
    /// withdrawing a contribution can address its `RATING#` history item
    /// (whose key contains it) without re-reading the match.
    pub played_at: String,
    pub movement: RatingMovementRecord,
    /// Wall clock at the moment this was applied. Deliberately *excluded*
    /// from the "has anything changed?" comparison — see
    /// [`RatingContributionRecord::has_same_effect_as`].
    pub applied_at: String,
}

impl RatingContributionRecord {
    /// Whether two contributions say the same thing about a match, ignoring
    /// `applied_at`.
    ///
    /// The exclusion is the whole point. `applied_at` is a fresh wall clock on
    /// every delivery, so comparing it would make every at-least-once
    /// redelivery look like a change — and a match's `#META` item is rewritten
    /// by every like and every comment, so redelivery is the common case, not
    /// the rare one. Including it would mean three item writes and a
    /// spurious "this match was re-scored" signal every time somebody
    /// thumbs-ups a finished game.
    #[must_use]
    pub fn has_same_effect_as(&self, other: &Self) -> bool {
        self.user_id == other.user_id
            && self.ladder == other.ladder
            && self.side_id == other.side_id
            && self.played_at == other.played_at
            && self.movement == other.movement
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;

    /// `occupies_slot` is the one rule for who takes a spot: in with no
    /// invitation, or an accepted invitee — never a pending or declined one.
    #[test]
    fn occupies_slot_counts_only_players_who_are_in() {
        let player = |status: Option<&str>| MatchPlayerRecord {
            player_id: "p".into(),
            user_id: Some("u".into()),
            display_name: None,
            side_id: None,
            is_member_of_team: None,
            invitation: status.map(|status| EmbeddedInvitationRecord {
                id: "inv".into(),
                status: status.into(),
                invited_by_user_id: "host".into(),
                invited_at: "2026-09-14T00:00:00Z".into(),
                responded_at: None,
                kind: InvitationKindRecord::User {
                    invited_user_id: "u".into(),
                },
            }),
            joined_via: None,
            ranked_prompt: None,
        };
        assert!(
            player(None).occupies_slot(),
            "added by an organiser or self-joined"
        );
        assert!(player(Some("accepted")).occupies_slot());
        assert!(!player(Some("pending")).occupies_slot());
        assert!(!player(Some("declined")).occupies_slot());
    }

    /// A player row with no `ranked_prompt`, as every row written before
    /// prompts existed reads back. Nothing was asked of them, so nothing is
    /// owed — and `awaits_ranked_prompt` says so for any id.
    #[test]
    fn a_player_row_written_before_prompts_existed_owes_no_answer() {
        let item = HashMap::from([
            ("player_id".to_string(), AttributeValue::S("p1".into())),
            ("user_id".to_string(), AttributeValue::S("u1".into())),
            ("role".to_string(), AttributeValue::S("player".into())),
        ]);
        let rec: MatchPlayerRecord =
            serde_dynamo::from_attribute_value(AttributeValue::M(item)).unwrap();

        assert!(rec.ranked_prompt.is_none());
        assert!(!rec.awaits_ranked_prompt("sw1"));
        assert!(rec.occupies_slot(), "still in, as they always were");
    }

    /// The invariant the whole switch-to-ranked flow rests on: **an
    /// outstanding prompt does not cost the player their spot.** If this ever
    /// starts failing, switching a friendly to ranked silently empties the
    /// match — and `sides.*.player_count`/`total_player_count`, which are
    /// maintained by deltas rather than recomputed, would disagree with the
    /// roster for good.
    ///
    /// The second half pins the other direction: a prompt does not *grant* a
    /// spot either. A pending invitee who somehow carries one is still out,
    /// because `occupies_slot` reads the invitation and nothing else.
    #[test]
    fn an_outstanding_prompt_neither_costs_nor_grants_a_spot() {
        let prompt = RankedPromptRecord {
            prompt_id: "sw1".into(),
            ladder: "tennis".into(),
            asked_at: "2026-10-14T09:00:00.000Z".into(),
        };
        let row = |invitation_status: Option<&str>| MatchPlayerRecord {
            player_id: "p1".into(),
            user_id: Some("u1".into()),
            display_name: None,
            side_id: Some("home".into()),
            is_member_of_team: None,
            invitation: invitation_status.map(|status| EmbeddedInvitationRecord {
                id: "inv".into(),
                status: status.into(),
                invited_by_user_id: "host".into(),
                invited_at: "2026-10-01T00:00:00Z".into(),
                responded_at: None,
                kind: InvitationKindRecord::User {
                    invited_user_id: "u1".into(),
                },
            }),
            joined_via: None,
            ranked_prompt: Some(prompt.clone()),
        };

        assert!(
            row(None).occupies_slot(),
            "an unanswered prompt holds the place; that is its purpose"
        );
        assert!(row(Some("accepted")).occupies_slot());
        assert!(
            !row(Some("pending")).occupies_slot(),
            "a prompt grants nothing an invitation hasn't"
        );
    }

    /// Answering is scoped to one ask. A row still carrying a prompt from a
    /// withdrawn switch owes nothing on the current one, and an answer quoting
    /// the old id must not be mistaken for the one owed now.
    #[test]
    fn a_prompt_is_only_owed_to_the_ask_that_wrote_it() {
        let mut rec: MatchPlayerRecord =
            serde_dynamo::from_attribute_value(AttributeValue::M(HashMap::from([
                ("player_id".to_string(), AttributeValue::S("p1".into())),
                ("role".to_string(), AttributeValue::S("player".into())),
            ])))
            .unwrap();
        rec.ranked_prompt = Some(RankedPromptRecord {
            prompt_id: "sw1".into(),
            ladder: "tennis".into(),
            asked_at: "2026-10-14T09:00:00.000Z".into(),
        });

        assert!(rec.awaits_ranked_prompt("sw1"));
        assert!(
            !rec.awaits_ranked_prompt("sw2"),
            "a later ask, not this one"
        );
        assert!(!rec.awaits_ranked_prompt(""));
    }

    /// `ranked_prompt` is skipped when absent, so the common case — every
    /// match that was created ranked, or friendly and left alone — writes the
    /// same item it always did. Worth pinning: the meta item is rewritten by
    /// every like and every comment, and a `null` attribute appearing on all
    /// of them is both wasted bytes and a pointless stream diff.
    #[test]
    fn a_match_with_no_ask_serializes_without_the_attribute() {
        let rec = MatchRecord {
            id: "m1".into(),
            created_by_user_id: "u1".into(),
            name: "Tuesday singles".into(),
            description: String::new(),
            match_type: "tennis".into(),
            status: "scheduled".into(),
            starts_at: "2026-10-20T18:00:00.000Z".into(),
            allow_unassigned: true,
            total_player_count: 2,
            location: None,
            series_id: None,
            sides: HashMap::new(),
            header_photos: Vec::new(),
            authority: HashMap::new(),
            confirmed_score: None,
            pending_score: None,
            like_count: 0,
            comment_count: 0,
            live_seq: 0,
            live_tip_seq: None,
            format: None,
            ranked: true,
            ranked_prompt: None,
            created_at: "2026-10-01T00:00:00.000Z".into(),
        };

        let item: HashMap<String, AttributeValue> =
            serde_dynamo::to_item(&rec).expect("serializes");
        assert!(
            !item.contains_key("ranked_prompt"),
            "absent, not null: {item:?}"
        );
    }

    /// An open ask survives the write/read cycle whole, and the deadline it
    /// carries is the value the sweep looks for — this item's own `GSI3SK`
    /// under [`MatchRankedPromptRecord::DUE_GSI3PK`]. The two are written by
    /// the same update, and nothing but this test ties the record's field to
    /// the index convention.
    #[test]
    fn an_open_ask_round_trips_with_the_deadline_the_sweep_queries() {
        let ask = MatchRankedPromptRecord {
            id: "sw1".into(),
            ladder: "football".into(),
            opened_at: "2026-10-14T09:00:00.000Z".into(),
            opened_by_user_id: "u1".into(),
            deadline: "2026-10-19T18:00:00.000Z".into(),
            pending: 3,
        };

        let item: HashMap<String, AttributeValue> = serde_dynamo::to_item(&ask).expect("to item");
        let back: MatchRankedPromptRecord =
            serde_dynamo::from_item(item).expect("reads back whole");

        assert_eq!(back, ask);
        assert_eq!(back.pending, 3);
        assert_eq!(MatchRankedPromptRecord::DUE_GSI3PK, "RANKEDPROMPTDUE");
        assert_eq!(
            back.deadline, "2026-10-19T18:00:00.000Z",
            "stamped as GSI3SK beside DUE_GSI3PK while the ask is open"
        );
    }

    /// `pending` defaults rather than failing, because it is the one field an
    /// atomic `ADD ranked_prompt.pending :delta` could leave behind if the ask
    /// were ever written without it. Zero is the safe reading: a sweep skips
    /// the match, and the roster — not this counter — decides who owes an
    /// answer.
    #[test]
    fn an_ask_missing_its_counter_reads_as_nothing_pending() {
        let item = HashMap::from([
            ("id".to_string(), AttributeValue::S("sw1".into())),
            ("ladder".to_string(), AttributeValue::S("tennis".into())),
            (
                "opened_at".to_string(),
                AttributeValue::S("2026-10-14T09:00:00.000Z".into()),
            ),
            (
                "opened_by_user_id".to_string(),
                AttributeValue::S("u1".into()),
            ),
            (
                "deadline".to_string(),
                AttributeValue::S("2026-10-19T18:00:00.000Z".into()),
            ),
        ]);
        let ask: MatchRankedPromptRecord =
            serde_dynamo::from_attribute_value(AttributeValue::M(item)).unwrap();
        assert_eq!(ask.pending, 0);
    }

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

    /// A match written before ratings existed has no `ranked` attribute, and
    /// must read back as a friendly. Guards `MatchRecord::ranked`'s serde
    /// default against being "aligned" with some create-time default: every
    /// like or comment rewrites `#META`, so a `true` here would enrol the old
    /// match into its ladder on its next like, rating players who never opted
    /// in.
    #[test]
    fn a_match_written_before_ratings_reads_back_as_friendly() {
        let meta = AttributeValue::M(HashMap::from([
            ("id".to_string(), AttributeValue::S("m1".into())),
            ("name".to_string(), AttributeValue::S("Tuesday".into())),
            ("description".to_string(), AttributeValue::S(String::new())),
            ("match_type".to_string(), AttributeValue::S("tennis".into())),
            ("status".to_string(), AttributeValue::S("completed".into())),
            (
                "starts_at".to_string(),
                AttributeValue::S("2025-01-01T18:00:00.000Z".into()),
            ),
            ("sides".to_string(), AttributeValue::M(HashMap::new())),
            ("like_count".to_string(), AttributeValue::N("4".into())),
            (
                "created_at".to_string(),
                AttributeValue::S("2025-01-01T00:00:00.000Z".into()),
            ),
        ]));
        let rec: MatchRecord = serde_dynamo::from_attribute_value(meta).unwrap();
        assert!(!rec.ranked);
    }

    /// `stats.<sport>` is only ever populated with the counters that have
    /// actually been incremented (see `Dao::stats_delta`/`ensure_stats_sport`),
    /// so a player who has only ever bowled (no `runs`, `fours`, `sixes`,
    /// `balls_faced`, `dismissals`, `wins`/`draws`/`losses` beyond whichever
    /// outcome actually happened, ...) has a sparse `stats.cricket` map, not
    /// one with every counter present at `0`. This must deserialize rather
    /// than 500 with "missing field" (the bug behind this test).
    #[test]
    fn sparse_cricket_stats_deserializes() {
        let stats_av = AttributeValue::M(HashMap::from([(
            "cricket".to_string(),
            AttributeValue::M(HashMap::from([
                ("matches_played".to_string(), AttributeValue::N("3".into())),
                ("wins".to_string(), AttributeValue::N("3".into())),
                ("wickets".to_string(), AttributeValue::N("5".into())),
            ])),
        )]));
        let rec: UserStatsRecord = serde_dynamo::from_attribute_value(stats_av).unwrap();
        let cricket = rec.cricket.expect("cricket stats present");
        assert_eq!(cricket.common.matches_played, 3);
        assert_eq!(cricket.common.wins, 3);
        assert_eq!(cricket.common.draws, 0);
        assert_eq!(cricket.common.losses, 0);
        assert_eq!(cricket.wickets, 5);
        assert_eq!(cricket.runs, 0);
        assert_eq!(cricket.balls_faced, 0);
        assert_eq!(cricket.balls_bowled, 0);
    }

    /// The minimum a stored user profile item can have had before ratings
    /// existed. Every rating field must default rather than fail the read —
    /// the whole reason they carry `#[serde(default)]` — because a
    /// missing-field failure here would 500 every profile read in production
    /// the moment the fields shipped.
    fn legacy_user_fields() -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("id".to_string(), AttributeValue::S("u1".into())),
            (
                "email".to_string(),
                AttributeValue::S("sofia@example.com".into()),
            ),
            ("name".to_string(), AttributeValue::S("Sofia".into())),
            (
                "created_at".to_string(),
                AttributeValue::S("2026-01-01T00:00:00Z".into()),
            ),
        ])
    }

    #[test]
    fn user_written_before_ratings_existed_is_unrated_and_opted_in_nowhere() {
        let rec: UserRecord =
            serde_dynamo::from_attribute_value(AttributeValue::M(legacy_user_fields())).unwrap();
        assert!(rec.ratings.is_empty(), "nothing rated");
        assert!(
            rec.rating_opt_ins.is_empty(),
            "nobody could opt in before the field existed"
        );
        assert!(!rec.has_opted_in("squash"));
    }

    /// `Dao::opt_in_to_rating` writes the consent time with a raw nested
    /// `SET rating_opt_ins.#ladder = :opted_in_at` of a string, while this
    /// side reads `HashMap<String, String>` — two shapes nothing but this test
    /// ties together. It builds the item the way that write leaves it and
    /// reads it back. The ladder names are prefixes of one another, to pin
    /// that `has_opted_in` is an exact-key lookup.
    #[test]
    fn an_opt_in_as_the_dao_writes_it_reads_back_for_exactly_that_ladder() {
        let mut item = legacy_user_fields();
        item.insert(
            "rating_opt_ins".to_string(),
            AttributeValue::M(HashMap::from([(
                "tennis".to_string(),
                AttributeValue::S("2026-09-15T10:00:00.000Z".into()),
            )])),
        );
        let rec: UserRecord = serde_dynamo::from_attribute_value(AttributeValue::M(item)).unwrap();

        assert!(rec.has_opted_in("tennis"));
        assert!(!rec.has_opted_in("tennis:doubles"));
        assert!(!rec.has_opted_in("squash"));
        assert_eq!(rec.rating_opt_ins["tennis"], "2026-09-15T10:00:00.000Z");
    }

    /// A rated, opted-in profile survives the write/read cycle whole — the
    /// `ratings` map of nested records and the `rating_opt_ins` map of
    /// strings both, beside the fields that were already there.
    #[test]
    fn a_rated_opted_in_profile_round_trips() {
        let rec = UserRecord {
            id: "u1".into(),
            email: "sofia@example.com".into(),
            name: "Sofia".into(),
            profile_image_url: None,
            follower_count: 3,
            following_count: 1,
            unread_count: 0,
            stats: UserStatsRecord::default(),
            ratings: HashMap::from([(
                "squash".to_string(),
                RatingRecord {
                    mu: 27.638_888_888_888_89,
                    sigma: 7.171_442_936_549_223,
                    matches_rated: 6,
                    last_rated_at: "2026-09-01T19:00:00.000Z".into(),
                },
            )]),
            rating_opt_ins: HashMap::from([
                ("squash".to_string(), "2026-08-01T12:00:00.000Z".to_string()),
                ("tennis".to_string(), "2026-09-15T10:00:00.000Z".to_string()),
            ]),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let av: AttributeValue = serde_dynamo::to_attribute_value(&rec).unwrap();
        let back: UserRecord = serde_dynamo::from_attribute_value(av).unwrap();
        assert_eq!(back, rec);
    }

    /// μ and σ are stored native and read back into the engine, so the
    /// round-trip has to be bit-exact — a rating that drifts on every
    /// read/write cycle would make every redelivery recompute a different
    /// movement and look like a re-score, and would make the optimistic lock
    /// on `ratings.<ladder>` fail against a value it just read.
    /// (`serde_dynamo` writes an `f64` as its shortest round-tripping decimal,
    /// which DynamoDB's 38 significant digits hold exactly; this is the test
    /// that says so out loud.)
    #[test]
    fn rating_record_round_trips_mu_and_sigma_exactly() {
        for (mu, sigma) in [
            (25.0, 25.0 / 3.0),
            (27.638_888_888_888_89, 7.171_442_936_549_223),
            (0.000_001, 0.000_001),
        ] {
            let rec = RatingRecord {
                mu,
                sigma,
                matches_rated: 7,
                last_rated_at: "2026-06-01T10:00:00.000Z".into(),
            };
            let av: AttributeValue = serde_dynamo::to_attribute_value(&rec).unwrap();
            let back: RatingRecord = serde_dynamo::from_attribute_value(av).unwrap();
            assert_eq!(back, rec);
        }
    }

    /// Redelivery must be free. A match's `#META` item is rewritten by every
    /// like and comment, so the rating pipeline re-runs on finished matches
    /// constantly; if `applied_at` counted as part of the contribution, each
    /// of those would look like a re-score and rewrite three items. Every
    /// *other* field must count, since each is a real change to what the
    /// match did — `dao::rating`'s write guard compares exactly these.
    #[test]
    fn a_redelivered_contribution_differs_only_in_applied_at() {
        let base = RatingContributionRecord {
            user_id: "u1".into(),
            ladder: "squash".into(),
            side_id: "sideA".into(),
            played_at: "2026-06-01T10:00:00.000Z".into(),
            movement: RatingMovementRecord {
                mu_before: 25.0,
                sigma_before: 25.0 / 3.0,
                mu_after: 27.6,
                sigma_after: 7.1,
                display_delta: 78,
            },
            applied_at: "2026-06-01T11:00:00.000Z".into(),
        };
        let redelivered = RatingContributionRecord {
            applied_at: "2026-06-02T09:30:00.000Z".into(),
            ..base.clone()
        };
        assert!(base.has_same_effect_as(&redelivered));

        let changes = [
            (
                "re-score",
                RatingContributionRecord {
                    movement: RatingMovementRecord {
                        mu_after: 22.4,
                        display_delta: -78,
                        ..base.movement
                    },
                    ..base.clone()
                },
            ),
            (
                "moved to the other side",
                RatingContributionRecord {
                    side_id: "sideB".into(),
                    ..base.clone()
                },
            ),
            (
                "rescheduled",
                RatingContributionRecord {
                    played_at: "2026-06-08T10:00:00.000Z".into(),
                    ..base.clone()
                },
            ),
            (
                "sport edited",
                RatingContributionRecord {
                    ladder: "tennis".into(),
                    ..base.clone()
                },
            ),
            (
                "another player",
                RatingContributionRecord {
                    user_id: "u2".into(),
                    ..base.clone()
                },
            ),
        ];
        for (what, changed) in changes {
            assert!(!base.has_same_effect_as(&changed), "{what}");
        }
    }
}
