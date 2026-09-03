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
        /// Points per side, keyed by side id.
        entries: HashMap<String, u32>,
    },
    Sets {
        /// Games won per set per side, keyed by side id.
        entries: HashMap<String, Vec<u32>>,
    },
    Cricket {
        innings: Vec<CricketScoreInningsRecord>,
        /// The current/most recent innings' recent-ball window — `None` once
        /// there isn't a "current" innings (between innings, or the match is
        /// over) or for a result with no ball-by-ball data behind it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recent_deliveries: Option<Vec<CricketDeliveryRecord>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_ball_context: Option<NextBallContextRecord>,
        /// True once the log's last innings has ended and no following one
        /// has started yet. `None` for a result with no live log behind it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        awaiting_next_innings: Option<bool>,
    },
    Football {
        /// Goal tally, keyed by side id. `#[serde(default)]` because this
        /// field didn't exist before the tally was embedded here — a record
        /// written in that gap has no `score` to fall back to, so it
        /// deserializes as an empty tally rather than 500ing.
        #[serde(default)]
        score: HashMap<String, u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goals: Option<Vec<FootballGoalEventRecord>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cards: Option<Vec<FootballCardEventRecord>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        substitutions: Option<Vec<FootballSubstitutionEventRecord>>,
        /// The most recent period marker seen, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period: Option<FootballPeriodRecord>,
        /// When each period marker was recorded, keyed by kind.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period_times: Option<HashMap<FootballPeriodRecord, String>>,
        /// Every penalty-shootout kick recorded, in order taken.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        penalty_shootout: Option<Vec<FootballPenaltyShootoutKickRecord>>,
        /// Running shootout tally (kicks scored, not taken), keyed by side id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        penalty_shootout_score: Option<HashMap<String, u32>>,
    },
    Netball {
        /// Goal tally, keyed by side id.
        #[serde(default)]
        score: HashMap<String, u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goals: Option<Vec<NetballGoalEventRecord>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fouls: Option<Vec<NetballFoulEventRecord>>,
        /// The most recent period marker seen, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period: Option<NetballPeriodRecord>,
        /// When each period marker was recorded, keyed by kind.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period_times: Option<HashMap<NetballPeriodRecord, String>>,
        /// The score as of each quarter-end marker, keyed by kind — the
        /// *only* source of the score for a quarter-only-scored match. See
        /// `agon_service::live_score::netball::NetballPeriodEvent::score`'s
        /// doc comment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period_scores: Option<HashMap<NetballPeriodRecord, HashMap<String, u32>>>,
    },
}

/// One innings' final totals, as stored on a match's confirmed/pending
/// `Score` — mirrors the API's `CricketScoreInnings`. `batting`/`bowling`/
/// `fall_of_wickets`/`extras` are `None` for a manually-entered result with
/// no per-player detail to hand over; populated when the result came from a
/// live-scored (or backfilled) match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketScoreInningsRecord {
    pub batting_side_id: String,
    pub bowling_side_id: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: OversRecord,
    pub declared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batting: Option<Vec<CricketBattingEntryRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowling: Option<Vec<CricketBowlingEntryRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall_of_wickets: Option<Vec<CricketFallOfWicketRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<CricketExtrasRecord>,
}

/// Mirrors the API's `detailed_score::cricket::CricketBattingEntry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketBattingEntryRecord {
    pub player_id: String,
    pub runs: u32,
    pub balls_faced: u32,
    pub fours: u32,
    pub sixes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissal: Option<CricketDismissalRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batting_position: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDismissalRecord {
    pub kind: CricketDismissalKindRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fielder_player_id: Option<String>,
}

/// Mirrors the API's `detailed_score::cricket::CricketBowlingEntry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketBowlingEntryRecord {
    pub player_id: String,
    pub overs: OversRecord,
    pub maidens: u32,
    pub runs_conceded: u32,
    pub wickets: u32,
    pub wides: u32,
    pub no_balls: u32,
}

/// Mirrors the API's `detailed_score::cricket::CricketExtras`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketExtrasRecord {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

/// Mirrors the API's `detailed_score::cricket::CricketFallOfWicket`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketFallOfWicketRecord {
    pub wicket: u32,
    pub runs: u32,
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overs: Option<OversRecord>,
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// over — mirrors the API's `detailed_score::cricket::Overs`. Two integer
/// fields rather than a single float, which can't safely represent a ball
/// count that doesn't fit in one decimal digit.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Per-ladder ratings, keyed by the `rating::Ladder` string (`"squash"`
    /// today, `"tennis:doubles"` if the split in Part 2.5 ever happens).
    /// `#[serde(default)]` because every profile written before the rating
    /// system existed has no `ratings` attribute at all — and an empty map is
    /// exactly right for them: no ladder played, nothing rated.
    ///
    /// Sits *beside* `stats` rather than inside it, and is a map where
    /// `UserStatsRecord` right above uses a named field per sport. Both of
    /// those are deliberate, and the second one is a deliberate
    /// inconsistency, so:
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
    #[serde(default)]
    pub ratings: HashMap<String, RatingRecord>,
    /// Whether this account's ratings are shown to anyone but its owner.
    ///
    /// Gates **information, not access**. Eligibility for a rating-gated
    /// match always uses the true rating, `Private` or not (see
    /// `MatchRecord::rating_requirement`): the server knows your number
    /// whether or not anyone can see it, so it can enforce a window without
    /// revealing anything. The rejected alternative — capping opted-out
    /// players to low-rated games — pointed the wrong way, herding every
    /// hidden strong player into beginner lobbies, which is sandbagging in
    /// its purest form.
    ///
    /// `#[serde(default)]` → `Private` for every profile written before the
    /// field existed, which is also the right product default: you opt in to
    /// being seen, never out of it.
    #[serde(default)]
    pub rating_visibility: RatingVisibilityRecord,
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
    /// Per-ladder ratings for the team *as a unit* — same shape, and the same
    /// reasoning, as [`UserRecord::ratings`], which see.
    ///
    /// A team's rating is its own stored number rather than something derived
    /// from its current members', because a roster changes: deriving it would
    /// move a team's rating on a transfer, when nobody had played a match.
    #[serde(default)]
    pub ratings: HashMap<String, RatingRecord>,
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
    /// Whether this match counts towards ratings.
    ///
    /// `#[serde(default)]` → **false**, which is the opposite of the
    /// create-time default, and that is intended. New matches are ranked
    /// unless the organiser opts out (most amateur games are casual, and
    /// opt-in-ranked would leave every ladder too sparse to matchmake on),
    /// but that is a *product* default belonging to the create endpoint, not
    /// a deserialization default. Reading `true` here would retroactively
    /// enrol the entire pre-rating back catalogue, which would then trickle
    /// into the ladders one match at a time as old matches happened to be
    /// touched by a like or a comment. Please don't "fix" this to a
    /// `default = "…true"` — the two defaults disagreeing is the point.
    ///
    /// **Set once, at creation; locked against *change* thereafter** — once
    /// `starts_at` passes or any score is submitted, whichever comes first.
    /// The lock is a correctness requirement rather than a nicety: without it
    /// you could log a game, see that you won, and *then* enrol it in the
    /// ladder.
    ///
    /// Note what that does and does not cover, because the two are easy to
    /// conflate. It closes the *edit* path: nobody changes their mind about a
    /// match once its result is in. It does not, and cannot, constrain
    /// creation — a match logged after it was played has no moment "before the
    /// result was knowable" to lock against, and refusing to rate
    /// after-the-fact results would refuse nearly every real amateur game. So
    /// somebody logging a match they have already played does choose this flag
    /// knowing the outcome; what stops that from being a free hand is that
    /// nothing is rated until every *other* side confirms the score (see the
    /// `confirmed_score` gate in `handlers::rating::rateable`). The residual
    /// gap — the opponent agrees to the score but never to the flag — is
    /// stated on `CreateMatchInput::ranked` and is a known limitation, not an
    /// oversight.
    #[serde(default)]
    pub ranked: bool,
    /// The rating window an account must fall inside to join, if the
    /// organiser set one. `None` — the overwhelmingly common case — means
    /// open to anyone, which is why it is skipped rather than written as an
    /// empty struct on every match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating_requirement: Option<RatingRequirementRecord>,
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
    Netball(NetballFormatRecord),
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

/// Mirrors `agon_service::match_format::NetballFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballFormatRecord {
    pub num_quarters: u32,
    pub quarter_length_minutes: u32,
    pub two_point_zone: bool,
    pub extra_time: bool,
}

/// `MATCH#<matchId>` / `SIDE#<sideId>` — one side of a match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchSideRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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

/// DAO-owned mirror of `agon_service::live_score::LiveEventInput`, sport-
/// first discriminated. New sport = new variant on both sides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "sport", rename_all = "snake_case")]
pub enum LiveEventPayloadRecord {
    Football(FootballLiveEventRecord),
    Cricket(CricketLiveEventRecord),
    Netball(NetballLiveEventRecord),
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
    /// Mirrors `agon_service::detailed_score::football::FootballGoalEvent::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballCardEventRecord {
    pub side_id: String,
    pub player_id: String,
    pub color: FootballCardColorRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `FootballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
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
    /// Mirrors `FootballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootballPeriodEventRecord {
    pub period: FootballPeriodRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    /// Mirrors `agon_service::detailed_score::cricket::CricketDelivery::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
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

/// Mirrors `detailed_score::cricket::NextBallContext`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextBallContextRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub striker_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_striker_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    pub over: u32,
    pub ball: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_over_bowler_player_id: Option<String>,
    pub runs_conceded_this_over: u32,
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

// ---- Netball live events ----------------------------------------------------

/// Mirrors `agon_service::live_score::netball::NetballLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetballLiveEventRecord {
    Goal(NetballGoalEventRecord),
    Foul(NetballFoulEventRecord),
    Period(NetballPeriodEventRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballGoalEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_position: Option<NetballPositionRecord>,
    pub two_points: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `agon_service::detailed_score::netball::NetballGoalEvent::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetballPositionRecord {
    GoalShooter,
    GoalAttack,
    WingAttack,
    Centre,
    WingDefence,
    GoalDefence,
    GoalKeeper,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballFoulEventRecord {
    pub side_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>,
    /// Named `foul_kind`, not `kind` — same collision-avoidance as the API's
    /// `NetballFoulEvent::foul_kind` (`NetballLiveEventRecord`'s own
    /// `#[serde(tag = "kind")]` would otherwise fight this field over the
    /// same wire key once serde flattens the variant's fields in).
    pub foul_kind: NetballFoulKindRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    /// Mirrors `NetballGoalEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NetballFoulKindRecord {
    Contact,
    Obstruction,
    Footwork,
    Offside,
    HeldBall,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetballPeriodEventRecord {
    pub period: NetballPeriodRecord,
    /// Cumulative score per side as of this marker — always present, same
    /// reasoning as `agon_service::live_score::netball::NetballPeriodEvent`.
    pub score: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NetballPeriodRecord {
    Start,
    QuarterOneEnd,
    QuarterTwoStart,
    QuarterTwoEnd,
    QuarterThreeStart,
    QuarterThreeEnd,
    QuarterFourStart,
    FullTime,
    ExtraTimeStart,
    ExtraTimeEnd,
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

/// A user's lifetime stats, one field per sport — `None` for a sport they've
/// never played a confirmed match in. Stored inline on `UserRecord::stats`.
///
/// Explicit named fields rather than a `HashMap<String, _>` keyed by sport
/// tag: the set of sports is closed (mirrors the API's `MatchType`), so
/// there's no need to pay for a stringly-keyed map to get "only entries for
/// sports actually played" — an `Option` field serializes absent the same
/// way a missing map key would, at the same DynamoDB storage shape
/// (`stats.cricket`, `stats.football`, ... as nested map attributes either
/// way). What a named field buys over the map: `CricketStatsRecord`'s
/// `runs`/`wickets` and football's `goals`/`assists` are real, distinctly
/// named, compiler-checked fields instead of stringly-keyed lookups into a
/// generic counters bag.
///
/// The DAO's *write* path (`agon_core::dao::stats::Dao::stats_delta` /
/// `update_best_figures`) doesn't construct or read this type at all — it
/// addresses `stats.<sport>.<counter>` via raw `UpdateItem` expressions built
/// from a runtime `sport: &str` and counter-name strings, so it stays fully
/// sport-agnostic regardless of how strongly-typed the read side is.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UserStatsRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cricket: Option<CricketStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub football: Option<FootballStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tennis: Option<GenericSportStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badminton: Option<GenericSportStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squash: Option<GenericSportStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_tennis: Option<GenericSportStatsRecord>,
    /// Netball has no dedicated stats record yet — a per-player goal/foul
    /// log exists on `ScoreRecord::Netball` (see `NetballGoalEventRecord`)
    /// that a future `NetballStatsRecord` (mirroring `CricketStatsRecord`/
    /// `FootballStatsRecord`) could derive best-figures from, same as
    /// cricket/football — just not built out yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netball: Option<GenericSportStatsRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<GenericSportStatsRecord>,
}

/// Lifetime counters common to every sport. Also the full shape for a sport
/// with no richer per-player box score to derive extras from (tennis,
/// badminton, squash, table tennis, "other") — `CricketStatsRecord`/
/// `FootballStatsRecord` flatten this in and add their own fields on top, the
/// DAO-side mirror of the API's `GenericPlayerStats`/`#[oai(flatten)]`.
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

/// Lifetime cricket stats: the common counters plus a batting/bowling summary
/// derived from every confirmed match's box score, and each counter's
/// personal-best single-match figure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CricketStatsRecord {
    #[serde(flatten)]
    pub common: GenericSportStatsRecord,
    // As with `GenericSportStatsRecord`, every counter here is
    // `#[serde(default)]`: only counters that were ever actually
    // incremented get written to `stats.cricket` (see `Dao::stats_delta`),
    // so e.g. a bowler who never batted has no `runs` attribute at all.
    #[serde(default)]
    pub runs: u64,
    #[serde(default)]
    pub wickets: u64,
    #[serde(default)]
    pub fours: u64,
    #[serde(default)]
    pub sixes: u64,
    /// Legal balls faced while batting (career total).
    #[serde(default)]
    pub balls_faced: u64,
    /// Times out as a batter — divisor for batting average. Not-out innings
    /// aren't counted, same convention as the sport's own "average".
    #[serde(default)]
    pub dismissals: u64,
    /// Catches taken (as the credited fielder on any dismissal, batting side
    /// or bowling side — a catch isn't tied to which side this player was
    /// fielding for in that innings).
    #[serde(default)]
    pub catches: u64,
    /// Runs conceded while bowling (career total) — divisor for economy.
    #[serde(default)]
    pub runs_conceded: u64,
    /// Legal balls bowled (career total) — divisor for economy, and the
    /// source for the displayed "overs bowled". Summed as a raw ball count
    /// rather than `Overs`, and — critically — each contributing match's
    /// legal-ball count is computed from *that match's own*
    /// `CricketFormatRecord::balls_per_over` (5-ball, 6-ball, whatever it
    /// was), not a fixed assumption, so the accumulation itself is exact
    /// regardless of how many different formats a career spans. The only
    /// approximation left is display: turning a cross-format total back into
    /// an "X overs Y balls" figure has to pick *some* over length, since a
    /// blended career total isn't really in any one format — this uses the
    /// standard 6-ball over, the same convention real-world career bowling
    /// figures are always reported in regardless of which tournaments
    /// contributed to them.
    #[serde(default)]
    pub balls_bowled: u64,
    /// Highest score in a single match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_runs: Option<BestFigureRecord>,
    /// Best single-match bowling spell — most wickets, with the runs
    /// conceded and overs bowled in that same spell so it isn't just a bare
    /// wicket count. See `Dao::update_best_bowling_figures`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_bowling: Option<BestBowlingFiguresRecord>,
}

/// A personal-best single-match bowling spell: most wickets taken, plus the
/// runs conceded and overs bowled in that same spell — e.g. "5 wickets for
/// 32 runs off 5.4 overs", not just "5 wickets". Ranked by `wickets` alone
/// (ties aren't broken by economy). See `Dao::update_best_bowling_figures`
/// for why this only ever ratchets up, same as `BestFigureRecord`.
///
/// `overs` is the exact figure from that one match (in that match's own
/// `balls_per_over`), not re-derived from a raw ball count under some
/// assumed over length — unlike the career `balls_bowled` total above, a
/// single match's own bowling figures have no cross-format ambiguity to
/// approximate away.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BestBowlingFiguresRecord {
    pub wickets: u64,
    pub runs_conceded: u64,
    pub overs: OversRecord,
    pub match_id: String,
}

/// Lifetime football stats: the common counters plus goals/assists derived
/// from every confirmed match's goal log, and personal-best single-match
/// figures.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FootballStatsRecord {
    #[serde(flatten)]
    pub common: GenericSportStatsRecord,
    // Sparse for the same reason as `CricketStatsRecord`'s counters — only
    // ever written once actually incremented.
    #[serde(default)]
    pub goals: u64,
    #[serde(default)]
    pub assists: u64,
    /// Most goals scored in a single match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_goals: Option<BestFigureRecord>,
    /// Most goals + assists combined in a single match — a more complete
    /// "best game" than assists alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_goal_contributions: Option<BestFigureRecord>,
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

/// One account's rating on one ladder, stored inline in the `ratings` map on
/// `USER#<uid>` / `#PROFILE` (or `TEAM#<tid>` / `#META`).
///
/// `mu`/`sigma` are the engine's **native** Weng-Lin values (`μ₀ = 25`,
/// `σ₀ = 8.33`), never the 1500-centred numbers a player reads. Storing
/// native is what makes `rating::scale` and the band table retunable by
/// deploy rather than by backfill — see `rating::scale`'s module doc — and it
/// is also the only form the engine can be fed back for the next match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingRecord {
    /// Mean estimate of skill — `rating::PlayerRating::mu`.
    pub mu: f64,
    /// Standard deviation, i.e. how unsure we are — `rating::PlayerRating::sigma`.
    pub sigma: f64,
    /// How many rated matches have been folded into this rating. Drives the
    /// `Unrated`-until-placed gate (`rating::PLACEMENT_MATCHES`), and is the
    /// cheap check that a repair replay covered the same matches the
    /// incremental path did.
    pub matches_rated: u64,
    /// The `starts_at` of the most recent match folded in — when the newest
    /// rated match was **played**, not when it was rated. That is the
    /// comparison the pipeline actually needs: a match arriving with an
    /// earlier `starts_at` than this is precisely the out-of-order case that
    /// triggers repair, and confirmation times can't answer it (a Monday game
    /// is routinely confirmed after a Wednesday one). It is therefore
    /// directly comparable with `Sk::Rating`'s `played_at` segment.
    pub last_rated_at: String,
}

/// The before/after pair one rated match moved an account through.
///
/// Shared by [`RatingHistoryRecord`] and [`RatingContributionRecord`] because
/// they record the same event from two directions — "what happened to this
/// account over time" and "what this match did to each participant" — and a
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
    /// which reads at first glance like a violation of "never persist a
    /// derived display value" (the rule that keeps bands out of the table).
    /// The distinction is that a band is a statement about the *present* —
    /// retune the thresholds and every profile should re-band instantly —
    /// whereas this is a log entry, and a log records what actually happened,
    /// including what the player was actually told. If `rating::scale` were
    /// ever retuned, a recomputed delta would quietly rewrite history; this
    /// one keeps saying what the match card said on the day.
    pub display_delta: i32,
}

/// `USER#<uid>` (or `TEAM#<tid>`) / `RATING#<ladder>#<played_at>#<matchId>` —
/// one match's effect on one account's rating on one ladder.
///
/// Time-ordered by construction (the key sorts on the match's `starts_at`),
/// which is what lets one item collection be two things at once: the replay
/// source `RepairRatings` pages through in played order, and the
/// rating-over-time chart. Both key fields are duplicated into the item as
/// plain attributes, normal for a single-table design — see `dao::item`.
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

/// Whether a rated participant is an account or a team.
///
/// Lives in the record rather than in the sort key (`RATINGCONTRIB#<id>` —
/// see `Sk::RatingContribution`), so listing every participant's contribution
/// for a match stays one query. Replay needs it to know which partition to
/// write a rating back to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RatingOwnerKindRecord {
    User,
    Team,
}

/// `MATCH#<mid>` / `RATINGCONTRIB#<participantId>` — what this match
/// currently contributes to one participant's rating. Absent => the match has
/// never been rated for this participant.
///
/// The direct analogue of [`StatContributionRecord`], and it earns its keep
/// the same way: the handler compares the contribution the match's current
/// state implies against this stored one, so an unchanged redelivery writes
/// nothing and a re-score is detected as a change rather than applied twice.
///
/// It carries more than the bare delta on purpose. With `side_id` and the
/// rating each participant carried *into* the match, the whole
/// `RATINGCONTRIB#` collection for a match is a self-sufficient input to
/// `rating::group_by_side` + `rating::rate_sides` — a repair can re-rate the
/// match from these items alone, without re-reading a roster that may since
/// have changed underneath it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RatingContributionRecord {
    pub owner_kind: RatingOwnerKindRecord,
    /// The user or team id — the same value as the sort key's segment.
    pub owner_id: String,
    /// The ladder the contribution counted under (the match's ladder at the
    /// time it was applied). Kept for the same reason
    /// `StatContributionRecord::match_type` is: if a match's sport is edited,
    /// the contribution has to be backed out of the *old* ladder.
    pub ladder: String,
    /// Which side this participant played for. Part of what makes the
    /// contribution collection a replay input in its own right.
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
        self.owner_kind == other.owner_kind
            && self.owner_id == other.owner_id
            && self.ladder == other.ladder
            && self.side_id == other.side_id
            && self.played_at == other.played_at
            && self.movement == other.movement
    }
}

/// Whether an account's ratings are visible to anyone but its owner. See
/// [`UserRecord::rating_visibility`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RatingVisibilityRecord {
    /// The default, and the default for a reason: opting in to being seen is
    /// a choice, opting out of it shouldn't have to be.
    #[default]
    Private,
    Public,
}

/// The rating window a match's organiser requires of anyone joining. See
/// [`MatchRecord::rating_requirement`].
///
/// Bounds are on the **display** scale — the 1500-centred numbers — because
/// this is the one rating value a human types in, and it is compared against
/// the player's displayed floor (`rating::DisplayRating::floor`, deliberately
/// the conservative value, so an unproven account can't gate-crash on
/// variance). Storing native μ instead would mean converting an organiser's
/// "1400" on the way in and back on the way out, for no benefit.
///
/// Both bounds are individually optional, which the plan's `{min, max}` did
/// not allow for: "1400+" and "under 1600" are both natural things to ask for,
/// and forcing a pair would make callers invent sentinel bounds that then leak
/// into the API and the search filters. `Some(RatingRequirementRecord { min:
/// None, max: None })` is degenerate and means the same as `None`; the
/// eligibility gate treats them identically.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RatingRequirementRecord {
    /// Inclusive lower bound. `None` = no floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i32>,
    /// Inclusive upper bound. `None` = no ceiling. Present at all because
    /// eligibility is enforced in *both* directions — being kept out of games
    /// far below your level is as much of the point as being kept out of ones
    /// far above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i32>,
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

    /// A `Football` score written in the brief window before the `score`
    /// tally field existed (just `goals`, no aggregate) deserializes to an
    /// empty tally rather than 500ing on a missing field.
    #[test]
    fn football_score_missing_tally_field_deserializes() {
        let score_av = AttributeValue::M(HashMap::from([
            ("type".to_string(), AttributeValue::S("football".into())),
            ("goals".to_string(), AttributeValue::L(vec![])),
        ]));
        let rec: ScoreRecord = serde_dynamo::from_attribute_value(score_av).unwrap();
        match rec {
            ScoreRecord::Football { score, .. } => assert!(score.is_empty()),
            _ => panic!("expected football"),
        }
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

    /// The minimum a stored user profile item can have had before the rating
    /// system existed. Every rating field must default rather than fail the
    /// read — this is the whole reason they carry `#[serde(default)]`, and a
    /// missing-field failure here would 500 every profile read in production
    /// the moment the field shipped.
    fn legacy_user_item() -> AttributeValue {
        AttributeValue::M(HashMap::from([
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
        ]))
    }

    #[test]
    fn user_written_before_ratings_existed_deserializes_unrated_and_private() {
        let rec: UserRecord = serde_dynamo::from_attribute_value(legacy_user_item()).unwrap();
        assert!(rec.ratings.is_empty(), "no ladder played, nothing rated");
        assert_eq!(rec.rating_visibility, RatingVisibilityRecord::Private);
    }

    /// A team item written before the field existed is unrated too — same
    /// map, same default, so the two owner kinds can share one DAO path.
    #[test]
    fn team_written_before_ratings_existed_deserializes_unrated() {
        let team_av = AttributeValue::M(HashMap::from([
            ("id".to_string(), AttributeValue::S("t1".into())),
            ("name".to_string(), AttributeValue::S("Bats".into())),
            (
                "created_at".to_string(),
                AttributeValue::S("2026-01-01T00:00:00Z".into()),
            ),
        ]));
        let rec: TeamRecord = serde_dynamo::from_attribute_value(team_av).unwrap();
        assert!(rec.ratings.is_empty());
    }

    /// The one default in this change that is *not* the create-time default:
    /// every match written before `ranked` existed reads as friendly. If it
    /// read as ranked, the entire pre-rating back catalogue would enrol
    /// itself into the ladders as old matches got touched by likes and
    /// comments — see `MatchRecord::ranked`.
    #[test]
    fn match_written_before_ranked_existed_is_not_ranked() {
        let match_av = AttributeValue::M(HashMap::from([
            ("id".to_string(), AttributeValue::S("m1".into())),
            ("name".to_string(), AttributeValue::S("Tuesday".into())),
            ("description".to_string(), AttributeValue::S("".into())),
            ("match_type".to_string(), AttributeValue::S("squash".into())),
            ("status".to_string(), AttributeValue::S("completed".into())),
            (
                "starts_at".to_string(),
                AttributeValue::S("2026-01-01T00:00:00Z".into()),
            ),
            ("sides".to_string(), AttributeValue::M(HashMap::new())),
            (
                "created_at".to_string(),
                AttributeValue::S("2026-01-01T00:00:00Z".into()),
            ),
        ]));
        let rec: MatchRecord = serde_dynamo::from_attribute_value(match_av).unwrap();
        assert!(
            !rec.ranked,
            "legacy matches must not be retroactively ranked"
        );
        assert_eq!(rec.rating_requirement, None);
    }

    /// μ and σ are stored native and read back into the engine, so the
    /// round-trip has to be bit-exact — a rating that drifts on every
    /// read/write cycle would make the replay-invariance property the whole
    /// repair design rests on false. (`serde_dynamo` writes an `f64` as its
    /// shortest round-tripping decimal, which DynamoDB's 38 significant
    /// digits hold exactly; this is the test that says so out loud.)
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

    /// A rating requirement can be one-sided in either direction. The plan
    /// specified a `{min, max}` pair; "1400+" and "under 1600" are both
    /// things an organiser will want to say, and a required pair would push
    /// callers into inventing sentinel bounds.
    #[test]
    fn rating_requirement_bounds_are_individually_optional() {
        for req in [
            RatingRequirementRecord {
                min: Some(1400),
                max: None,
            },
            RatingRequirementRecord {
                min: None,
                max: Some(1600),
            },
            RatingRequirementRecord {
                min: Some(1400),
                max: Some(1600),
            },
            RatingRequirementRecord {
                min: None,
                max: None,
            },
        ] {
            let av: AttributeValue = serde_dynamo::to_attribute_value(req).unwrap();
            let back: RatingRequirementRecord = serde_dynamo::from_attribute_value(av).unwrap();
            assert_eq!(back, req);
        }
    }

    /// Redelivery must be free. A match's `#META` item is rewritten by every
    /// like and comment, so the rating handler re-runs on finished matches
    /// constantly; if `applied_at` counted as part of the contribution, each
    /// of those would look like a re-score and rewrite three items.
    #[test]
    fn a_redelivered_contribution_differs_only_in_applied_at() {
        let base = RatingContributionRecord {
            owner_kind: RatingOwnerKindRecord::User,
            owner_id: "u1".into(),
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

        // ...but a genuine re-score is not the same thing.
        let rescored = RatingContributionRecord {
            movement: RatingMovementRecord {
                mu_after: 22.4,
                display_delta: -78,
                ..base.movement
            },
            ..base.clone()
        };
        assert!(!base.has_same_effect_as(&rescored));

        // Nor is a roster edit that moved someone to the other side.
        let swapped = RatingContributionRecord {
            side_id: "sideB".into(),
            ..base.clone()
        };
        assert!(!base.has_same_effect_as(&swapped));
    }
}
