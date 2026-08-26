// The service is currently mock-backed: error response variants and the cricket
// score aggregator are part of the intended API surface but aren't constructed
// until the real DAO is wired in, so dead_code is expected for now. The enum/arg
// size lints are not worth restructuring generated-API response types over.
#![allow(dead_code)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]
#![allow(clippy::too_many_arguments)]

use std::{collections::HashMap, fs::File, io::Write};

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use clap::{Parser, Subcommand};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use poem::http::Uri;
use poem::{Endpoint, IntoResponse, Response};
use poem::{
    EndpointExt, Error, Request, Result, Route, Server,
    http::StatusCode,
    listener::TcpListener,
    middleware::{Cors, Tracing},
    web::Data,
};
use poem_openapi::auth::Bearer;
use poem_openapi::param::Query;
use poem_openapi::{
    ApiResponse, Enum, Object, OpenApi, OpenApiService, SecurityScheme, Union,
    param::Path,
    payload::{Json, PlainText},
};
use tracing::{error, info};

// Data access layer for DynamoDB — now the shared `agon_core` crate. Aliased as
// `dao` so existing `dao::…` paths in handlers keep working.
use agon_core::dao;
// JWT verification (asymmetric; Supabase JWKS + static test key).
mod auth;
use auth::{JwtClaims, JwtVerifier};
// Boundary mapping between API models and DAO records.
mod mapping;
use mapping::{
    comment_from_record, dao_internal, deleted_user_profile, derive_live_score,
    device_platform_to_record, feed_match_from_records, invitation_detail_from_record,
    invitation_from_record, invitation_status_from_str, invitation_status_str,
    live_event_from_record, match_format_sport_tag, match_format_to_record, match_from_records,
    match_score_from_record, match_score_to_record, match_status_str, match_type_tag,
    new_live_event_to_dao, notification_actor_id, notification_from_record, roster_preview_player,
    score_submission_from_record, score_to_record, search_match_from_records, team_from_records,
    team_list_item_from_record, team_member_from_record, user_profile_from_record,
};

// Object-storage integration: S3 presigned uploads + CloudFront serving URLs.
mod assets;
use assets::Assets;

mod match_format;
use match_format::MatchFormat;

mod detailed_score;
use detailed_score::{
    cricket::{
        CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDeliveryWicket,
        CricketDismissal, CricketExtras, CricketFallOfWicket, NextBallContext, Overs,
    },
    football::{
        FootballCardEvent, FootballGoalEvent, FootballPenaltyShootoutKick, FootballPeriod,
        FootballSubstitutionEvent,
    },
    netball::{NetballFoulEvent, NetballGoalEvent, NetballPeriod},
};

mod live_score;
use live_score::{
    AppendLiveEventsInput, LiveEvent, LiveEventInput, LiveScoreSnapshot, NewLiveEventInput,
};

mod membership;
use membership::{
    AddInvitationsInput, Invitation, InvitationContext, InvitationDetail, InvitationKind,
    InvitationMatchContext, InvitationStatus, Member, RespondByTokenInput,
    RespondToInvitationInput, TokenInvitation, UserInvitation, UserMember,
};

mod team;
use team::{
    AddTeamMembersInput, CreateTeamInput, Team, TeamListItem, TeamMatchMode, TeamMember, TeamRole,
    UpdateTeamInput,
};

mod notification;
use notification::{
    CommentNotification, FollowNotification, InvitationAcceptedNotification, LikeNotification,
    MatchInvitationNotification, Notification, NotificationKind, NotificationPage,
    ReplyNotification, ScoreConfirmedNotification, ScoreSubmittedNotification,
    TeamInvitationNotification, UnreadCount,
};

#[derive(SecurityScheme)]
#[oai(
    ty = "bearer",
    key_name = "authorization",
    key_in = "header",
    checker = "jwt_checker"
)]
struct AuthSchema(JwtClaims);

async fn jwt_checker(req: &Request, bearer: Bearer) -> Result<JwtClaims, poem::error::Error> {
    // The verifier is injected once at startup via `.data(..)`.
    let verifier = req
        .data::<JwtVerifier>()
        .expect("JwtVerifier missing from request data");

    verifier.verify(&bearer.token).await.map_err(|err| {
        info!("JWT invalid: {}", err.0);
        Error::from_string("Invalid JWT", StatusCode::UNAUTHORIZED)
    })
}

struct Api;

/// Lifetime stats common to every sport: matches played and the outcome
/// breakdown (win/draw/loss), plus a derived win percentage. Also the full
/// shape for a sport with no richer per-player detail to add (tennis,
/// badminton, squash, table tennis, "other") — `CricketPlayerStats`/
/// `FootballPlayerStats` flatten this in and add their own fields on top.
#[derive(Object)]
pub struct GenericPlayerStats {
    pub matches_played: i32,
    pub wins: i32,
    pub draws: i32,
    pub losses: i32,
    /// `None` when no confirmed matches have been played yet — display as
    /// "-", not 0%.
    pub win_percentage: Option<f32>,
    // TODO Elo
}

/// A personal-best single-match value for one counter, plus the match it
/// happened in — e.g. "5 wickets" and the match id of that bowling
/// performance. `None` on the containing stats until the counter's ever
/// been above zero in a single match. Only ever increases — see
/// `agon_core::dao::stats::Dao::update_best_figures` for why a downward
/// re-score or a cancelled match doesn't retroactively revise it.
#[derive(Object)]
pub struct BestFigure {
    pub value: i32,
    pub match_id: String,
}

/// A personal-best single-match bowling spell: most wickets taken, plus the
/// runs conceded and overs bowled in that same spell — e.g. "5 wickets for
/// 32 runs off 5.4 overs", not just "5 wickets". Ranked by wickets alone
/// (ties aren't broken by economy). Only ever increases — same reasoning as
/// `BestFigure`.
#[derive(Object)]
pub struct BestBowlingFigures {
    pub wickets: i32,
    pub runs_conceded: i32,
    pub overs: Overs,
    pub match_id: String,
}

/// Lifetime cricket stats: the common counters plus a batting/bowling summary
/// derived from every confirmed match's box score. `strike_rate`/
/// `batting_average`/`economy` are derived server-side (`None` when their
/// divisor is zero), the same convention as `GenericPlayerStats::
/// win_percentage`.
#[derive(Object)]
pub struct CricketPlayerStats {
    #[oai(flatten)]
    pub common: GenericPlayerStats,
    pub runs: i32,
    pub wickets: i32,
    pub fours: i32,
    pub sixes: i32,
    /// Legal balls faced while batting (career total).
    pub balls_faced: i32,
    /// Times out as a batter — divisor for `batting_average`.
    pub dismissals: i32,
    /// Catches taken.
    pub catches: i32,
    /// Runs conceded while bowling (career total) — divisor for `economy`.
    pub runs_conceded: i32,
    /// Overs bowled (career total). Each contributing match's legal-ball
    /// count is exact (computed from that match's own `balls_per_over`, 5 or
    /// 6 or otherwise, not assumed) — only turning the resulting cross-match
    /// ball total back into an "X overs Y balls" figure uses a fixed
    /// standard 6-ball over, the same convention real-world career bowling
    /// figures are always reported in regardless of which formats
    /// contributed to them.
    pub overs_bowled: Overs,
    /// Runs scored per 100 balls faced. `None` with zero balls faced.
    pub strike_rate: Option<f32>,
    /// Runs scored per dismissal — an undismissed batter has no average to
    /// show. `None` with zero dismissals.
    pub batting_average: Option<f32>,
    /// Runs conceded per over bowled. `None` with zero overs bowled.
    pub economy: Option<f32>,
    /// Highest score in a single match.
    pub best_runs: Option<BestFigure>,
    /// Best single-match bowling spell — `overs` is the exact figure from
    /// that one match (no cross-format approximation needed, unlike
    /// `overs_bowled` above).
    pub best_bowling: Option<BestBowlingFigures>,
}

/// Lifetime football stats: the common counters plus goals/assists derived
/// from every confirmed match's goal log.
#[derive(Object)]
pub struct FootballPlayerStats {
    #[oai(flatten)]
    pub common: GenericPlayerStats,
    pub goals: i32,
    pub assists: i32,
    /// Most goals scored in a single match.
    pub best_goals: Option<BestFigure>,
    /// Most goals + assists combined in a single match — a more complete
    /// "best game" than assists alone.
    pub best_goal_contributions: Option<BestFigure>,
}

/// A user's lifetime stats, one field per sport — `None` for a sport they've
/// never played a confirmed match in. Explicit per-sport fields rather than a
/// list/map so each sport's shape is self-describing: cricket and football
/// carry their own typed detail, everything else is the common shape as-is.
#[derive(Object)]
pub struct UserStats {
    pub cricket: Option<CricketPlayerStats>,
    pub football: Option<FootballPlayerStats>,
    pub tennis: Option<GenericPlayerStats>,
    pub badminton: Option<GenericPlayerStats>,
    pub squash: Option<GenericPlayerStats>,
    pub table_tennis: Option<GenericPlayerStats>,
    pub netball: Option<GenericPlayerStats>,
    pub other: Option<GenericPlayerStats>,
}

#[derive(Object)]
pub struct UserProfile {
    pub id: String,
    pub name: String,
    /// Profile image. Uploaded by the client directly to object storage
    /// (Supabase Storage); the API only stores/returns the resulting URL.
    pub profile_image: Option<Photo>,
    pub stats: UserStats,
    pub follower_count: u32,
    pub following_count: u32,
    /// Whether the requesting user follows this profile. False for your own.
    pub is_followed_by_me: bool,
}

/// The authenticated user's own view: their public `profile` plus private fields
/// (e.g. email) that only they can see. Returned by `/users/me`.
#[derive(Object)]
struct User {
    /// Private to the user. Unique across all users.
    email: String,
    /// The same public profile others see (id, name, image, stats, follower
    /// counts).
    profile: UserProfile,
}

#[derive(Object)]
struct CreateUserInput {
    /// The user's display name. The email is NOT accepted here — it's taken from
    /// the verified JWT (`email` claim), so a caller can't sign up as an
    /// arbitrary/other email.
    name: String,
}

/// Editable fields on the current user's own profile. All optional — only
/// supplied fields change. `profile_image_asset_id` references an `Asset` the
/// client created via `POST /assets`; the server rejects it unless the asset
/// is `Uploaded`, and resolves it to the stored image URL. None leaves the image
/// unchanged.
#[derive(Object)]
struct UpdateUserInput {
    name: Option<String>,
    profile_image_asset_id: Option<String>,
    // Note: no `email` field. Email is owned by the identity provider (the JWT
    // `email` claim), not user-editable through the API — accepting it here would
    // be both a no-op (the DAO doesn't update email) and misleading.
}

#[derive(Object)]
pub struct Photo {
    pub image_url: String,
    /// The asset backing this photo, when known — pass it back in
    /// `header_photo_asset_ids` to keep, reorder, or mix it with newly
    /// uploaded photos on a future edit. `None` for a profile image (single,
    /// always replaced wholesale) or a header photo attached before asset
    /// ids were tracked alongside the URL.
    pub asset_id: Option<String>,
}

/// What an upload is for. Drives the storage bucket/path and the size/type
/// constraints the server applies when issuing the presigned URL.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum UploadPurpose {
    ProfileImage,
    TeamLogo,
    MatchHeader,
}

/// Lifecycle of an uploadable asset.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum AssetStatus {
    /// A presigned URL has been issued; the bytes have not arrived yet.
    Pending,
    /// Storage notified us the object exists (and any processing is done).
    Uploaded,
    /// The upload expired or was rejected.
    Failed,
}

/// A tracked uploadable asset. Created `Pending` when requested and flipped to
/// `Uploaded` by a storage event (not by the client), so the API is the source
/// of truth for whether bytes actually landed. Resources reference an asset by
/// `id`; the API only attaches it once it is `Uploaded`. Both `POST /assets` and
/// `GET /assets/:id` return this same representation.
#[derive(Object)]
struct Asset {
    id: String,
    status: AssetStatus,
    content_type: String,
    /// Where/how to upload the bytes. Present only while `status` is `Pending`;
    /// None once `Uploaded`/`Failed`. Regenerated fresh on each read (the
    /// presigned URL is short-lived), so re-reading a pending asset yields a
    /// usable URL — that is the upload-retry mechanism.
    upload: Option<UploadTarget>,
    /// The readable URL. Populated by the server once `status` is `Uploaded`;
    /// None while `Pending`/`Failed`.
    url: Option<String>,
}

/// Create an asset. Returns the asset in `Pending` status with its `upload`
/// target populated. The client uploads the bytes directly to storage using that
/// target, then references the asset by `id` on the relevant resource (e.g.
/// `PATCH /users/me` with `profile_image_asset_id`).
#[derive(Object)]
struct CreateAssetInput {
    purpose: UploadPurpose,
    /// MIME type of the file to upload, e.g. "image/jpeg". The server validates
    /// it against the purpose and bakes it into the presigned URL.
    content_type: String,
    /// Exact byte length of the file to upload. The server rejects anything over
    /// its per-purpose max and bakes this exact length into the presigned URL, so
    /// S3 refuses an upload of any other size.
    content_length: i64,
}

/// Where and how to upload an asset's bytes. Provider-agnostic: the client just
/// replays `method` + `headers` against `upload_url`. None of these fields name a
/// specific storage provider, so the backend can swap S3/R2/GCS/Supabase without
/// changing the contract.
#[derive(Object)]
struct UploadTarget {
    /// Short-lived presigned URL to send the file bytes to.
    upload_url: String,
    /// HTTP method to use for the upload request (e.g. "PUT").
    method: String,
    /// Headers the client must include on the upload request.
    headers: Vec<UploadHeader>,
}

#[derive(Object)]
struct UploadHeader {
    name: String,
    value: String,
}

/// One side of a match. Carries optional team metadata (a side may be a
/// persistent team or an ad-hoc group of manually picked players) plus the
/// authoritative roster of who actually played for this side. `Score` entries
/// and `winner_side_id` reference `id`.
#[derive(Object)]
struct MatchSide {
    /// Stable id for this side within the match. `Score` entries and
    /// `winner_side_id` reference this; players link to it via `side_id`.
    id: String,
    /// Optional link to a persistent Team (drives "Kent vs Surrey" labelling and
    /// the pick-from-squad UI). None = ad-hoc side with manually picked players.
    team_id: Option<String>,
    /// Display name for this side, resolved fresh on every response (see
    /// `Api::hydrate_match`) — never None in practice. Priority: a custom
    /// name the creator gave the side, else the sole player's name if
    /// there's exactly one, else the team's name, else "Your
    /// side"/"Opposition" relative to the caller, else a neutral "Team
    /// A"/"Team B". Computed per-request rather than stored so it can't go
    /// stale and "your side" always means the caller.
    name: Option<String>,
    /// The linked team's logo, resolved fresh alongside `name` (see
    /// `Api::hydrate_match`) whenever `team_id` is set and that team has one.
    /// `None` for an ad-hoc side, a team with no logo uploaded, or a
    /// not-yet-resolved side — callers fall back to initials (e.g. `Avatar`'s
    /// `name`-derived placeholder) when this is absent.
    team_logo: Option<Photo>,
    /// This side's full roster, when small enough to show directly instead of
    /// just `name`/`team_id`'s logo (1v1, doubles, a small squad). `None`
    /// when the side has more players than that — render `name`/the team's
    /// logo instead. On `Match` this is resolved live from `players`; on a
    /// feed's `FeedMatch` (which never fetches the full roster) it comes from
    /// a denormalized cache refreshed whenever the roster changes, so it can
    /// occasionally lag a just-now roster change.
    roster_preview: Option<Vec<RosterPreviewPlayer>>,
}

/// A player in a side's `roster_preview` — name/avatar only, not the full
/// `MatchPlayer`/`Member` shape (no invitation status; this is a display hint,
/// not the roster management view).
#[derive(Object)]
struct RosterPreviewPlayer {
    /// Present for a linked Agon account; `None` for an unlinked "external"
    /// player (only a display name known).
    user_id: Option<String>,
    name: String,
    avatar_url: Option<String>,
}

/// A player in a match. Held in a flat match-level list (not nested under a
/// side) so an invited player can exist before being assigned to a side. The
/// shared `Member` (Agon user or external, with optional invitation) plus
/// match-specific context. Score events reference the player's `Member` id,
/// which is stable across External -> User acceptance.
#[derive(Object)]
struct MatchPlayer {
    member: Member,
    /// The side this player is on (references `MatchSide.id`). None until they
    /// accept and are assigned a side; once accepted a player must have a side.
    side_id: Option<String>,
    /// True if this player is a member of their side's team; false = a ringer.
    /// None when unassigned or the side has no team.
    is_member_of_team: Option<bool>,
}

/// Match score. Tagged union so each sport's scoring shape is modelled
/// explicitly; clients switch on `type` to pick a renderer. Add new variants
/// (e.g. golf) without breaking existing clients.
///
/// One type serves three roles for football/cricket: the confirmable result
/// embedded on `Match.confirmed_score`/`pending_score`, the persisted
/// live-scoring record, and `GET /matches/:id/score`'s live-poll response —
/// live or finished, confirmed or not, it's the same shape either way. Every
/// field beyond the settled headline (goal tally; per-innings totals) is
/// `Option`: `None` for a bare manually-entered result with no richer detail
/// behind it, `Some(...)` (possibly containing empty collections) once
/// there's live or backfilled detail to carry.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
enum Score {
    /// Single number per side: basketball, rugby, and any other sport with no
    /// richer native shape.
    Simple(SimpleScore),
    /// Set-based: tennis, volleyball, badminton.
    Sets(SetsScore),
    /// Per-innings runs/wickets/overs (plus optional per-player detail), the
    /// result of a completed cricket match — live-scored or manually entered.
    /// Carries enough to render the completed scorecard tile — and to derive
    /// the result margin ("won by 4 wickets" / "by 100 runs") — without a
    /// separate fetch of the match's live event log.
    Cricket(CricketScore),
    /// Goals scored (plus optional cards/substitutions), the result of a
    /// completed football match — live-scored or manually entered. Carries
    /// enough to render the feed/detail goal ticker without a separate fetch.
    Football(FootballScore),
    /// Goals scored (plus optional fouls and per-quarter breakdown), the
    /// result of a completed netball match — live-scored (either
    /// event-by-event or quarter-only) or manually entered.
    Netball(NetballScore),
}

#[derive(Object)]
struct SimpleScore {
    /// Points per side, keyed by side id — exactly one entry per side, so a
    /// map rather than a `Vec<{side_id, ...}>` list.
    entries: HashMap<String, u32>,
}

#[derive(Object)]
struct SetsScore {
    /// Games won per set per side, keyed by side id. Each side's list is
    /// index-aligned with every other side's — the same index is the same
    /// set (e.g. `["side_a": [6, 4, 7], "side_b": [4, 6, 5]]`).
    entries: HashMap<String, Vec<u32>>,
}

#[derive(Object)]
struct CricketScore {
    /// One entry per innings played, in the order they were played.
    innings: Vec<CricketScoreInnings>,
    /// The current/most recent innings' recent-ball window, for a "this
    /// over"/recent-balls read. `None` once there isn't a current innings
    /// (between innings, or the match is over) or for a result with no
    /// ball-by-ball detail behind it — there's nothing to show. Bounded
    /// (`detailed_score::cricket::RECENT_DELIVERIES_LIMIT`), not the whole
    /// innings — a finished match's complete ball-by-ball history reads the
    /// live event log directly (paginated — `GET /matches/:id/live/events`)
    /// instead of this field.
    recent_deliveries: Option<Vec<CricketDelivery>>,
    /// What's known about who's at the crease/bowling for the *next*
    /// delivery. `None` once there isn't a next delivery to give context for
    /// (between innings, match over, or no live detail at all).
    next_ball_context: Option<NextBallContext>,
    /// True once the log's last innings has ended and no following one has
    /// started yet (i.e. between innings, or nothing's been recorded).
    /// `None` for a result with no live log behind it.
    awaiting_next_innings: Option<bool>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — `next_ball_context`'s striker/non-striker/bowler, each
    /// innings' batting/bowling/fall-of-wicket entries, `recent_deliveries` —
    /// keyed by that same (match-scoped) player id. Look a player up here
    /// instead of scanning `Match.players`, which a feed/search card's
    /// trimmed match type doesn't carry at all. Resolved separately from
    /// everything else on this type: `score_from_record`/
    /// `CricketScore::from_events` (the DAO-only paths) always leave this
    /// empty, since neither has access to player records;
    /// `Api::hydrate_score_players` fills it afterward with one targeted
    /// `Dao::batch_get_match_players` lookup for exactly the ids this score
    /// references, not a full roster query. Not persisted (no counterpart on
    /// `ScoreRecord`).
    players: HashMap<String, RosterPreviewPlayer>,
}

/// One innings' totals, plus optional per-player detail. The totals
/// (`runs`/`wickets`/`overs`/`declared`) are always present — enough for a
/// completed-match tile — while `batting`/`bowling`/`fall_of_wickets`/
/// `extras` are `None` for a result with no per-player detail to hand over
/// (a manually-entered result with no card, or one that doesn't include
/// fall-of-wickets — see `CricketBattingEntry`/`CricketBowlingEntry` etc. in
/// `detailed_score::cricket`, reused here verbatim) and populated for a
/// live-scored (or backfilled) match. Never includes ball-by-ball deliveries
/// — that stays in the live event log for a match that wants it.
#[derive(Object)]
struct CricketScoreInnings {
    /// The batting side for this innings (references MatchSide.id).
    batting_side_id: String,
    /// The bowling/fielding side for this innings.
    bowling_side_id: String,
    /// Total runs scored in the innings.
    runs: u32,
    /// Wickets lost (0-10).
    wickets: u32,
    /// Overs bowled, e.g. 19 overs + 4 balls into the 20th.
    overs: Overs,
    /// Whether the innings was declared closed rather than bowled/timed out.
    declared: bool,
    batting: Option<Vec<CricketBattingEntry>>,
    bowling: Option<Vec<CricketBowlingEntry>>,
    fall_of_wickets: Option<Vec<CricketFallOfWicket>>,
    extras: Option<CricketExtras>,
}

impl CricketScoreInnings {
    /// The state before any delivery has been recorded in this innings —
    /// only ever constructed on a live-scoring path (`InningsStart`), so the
    /// optional card fields start populated (`Some`, empty) rather than
    /// `None`: there's a live innings behind this from the moment it exists,
    /// even before its first ball.
    fn opening(batting_side_id: String, bowling_side_id: String) -> Self {
        CricketScoreInnings {
            batting_side_id,
            bowling_side_id,
            runs: 0,
            wickets: 0,
            overs: Overs { overs: 0, balls: 0 },
            declared: false,
            batting: Some(Vec::new()),
            bowling: Some(Vec::new()),
            fall_of_wickets: Some(Vec::new()),
            extras: Some(CricketExtras::default()),
        }
    }
}

/// A football match's result: the goal tally, plus optional richer detail.
#[derive(Object)]
struct FootballScore {
    /// Goal tally, keyed by side id — exactly one entry per side, so a map
    /// rather than the `Vec<{side_id, ...}>` shape used where order or
    /// repeats matter (e.g. `goals`).
    score: HashMap<String, u32>,
    /// Every goal scored (normal + extra time; penalty-shootout kicks are
    /// tracked separately and never appear here), if there's a goal-by-goal
    /// breakdown to hand over. Reuses `detailed_score::football::
    /// FootballGoalEvent` verbatim.
    goals: Option<Vec<FootballGoalEvent>>,
    cards: Option<Vec<FootballCardEvent>>,
    substitutions: Option<Vec<FootballSubstitutionEvent>>,
    /// The most recent period marker seen, if any. `None` for a result with
    /// no live detail behind it.
    period: Option<FootballPeriod>,
    /// When each period marker was recorded, keyed by kind — one entry per
    /// `FootballPeriod` variant seen so far (a marker recorded twice
    /// overwrites, it doesn't append). Historical facts, not "current
    /// state" — nothing here goes blank once the match is over.
    period_times: Option<HashMap<FootballPeriod, chrono::DateTime<chrono::Utc>>>,
    /// Every penalty-shootout kick recorded, in order taken. Separate from
    /// `goals`/`score` — a shootout kick never counts as a match goal, only
    /// towards `penalty_shootout_score` — since the scoreline it decides
    /// (e.g. "1-1, Riverside win 4-3 on penalties") keeps the 90/120-minute
    /// score and the shootout tally visually distinct, same as it's reported
    /// in the real world.
    penalty_shootout: Option<Vec<FootballPenaltyShootoutKick>>,
    /// Running shootout tally (kicks scored, not kicks taken) per side,
    /// derived from `penalty_shootout` the same way `score` is derived from
    /// `goals`. Keyed by side id, same reasoning as `score`.
    penalty_shootout_score: Option<HashMap<String, u32>>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — `goals`' scorer/assist, `cards`' player, `substitutions`' in/
    /// out — keyed by that same (match-scoped) player id. Same mechanism and
    /// rationale as `CricketScore.players`.
    players: HashMap<String, RosterPreviewPlayer>,
}

/// A netball match's result: the goal tally, plus optional richer detail.
/// See `live_score::netball`'s doc comment for how the two live-scoring
/// methods (event-by-event, quarter-only) both fold into this one shape.
#[derive(Object)]
struct NetballScore {
    /// Goal tally, keyed by side id — exactly one entry per side, same
    /// "map, not a list" convention as `FootballScore.score`. In
    /// event-by-event mode this is folded from `goals`; in quarter-only mode
    /// it's just whatever the last `Period` marker said.
    score: HashMap<String, u32>,
    /// Every goal scored, if there's a goal-by-goal breakdown to hand over —
    /// `None` for a quarter-only-scored or manually-entered result, which
    /// has no such detail. Reuses `detailed_score::netball::NetballGoalEvent`
    /// verbatim.
    goals: Option<Vec<NetballGoalEvent>>,
    /// Non-scoring infringements, for stat display — same role as
    /// `FootballScore.cards`. `None` for a quarter-only-scored or
    /// manually-entered result.
    fouls: Option<Vec<NetballFoulEvent>>,
    /// The most recent period marker seen, if any. `None` for a result with
    /// no live detail behind it.
    period: Option<NetballPeriod>,
    /// When each period marker was recorded, keyed by kind — same convention
    /// as `FootballScore.period_times`.
    period_times: Option<HashMap<NetballPeriod, chrono::DateTime<chrono::Utc>>>,
    /// The score *as of* each quarter-end marker — this is what lets a
    /// client render "Q1 12-9, Q2 22-18, ..." regardless of which
    /// live-scoring method produced it (see `live_score::netball::
    /// NetballPeriodEvent::score`'s doc comment).
    period_scores: Option<HashMap<NetballPeriod, HashMap<String, u32>>>,
    /// Live name/avatar for every player id referenced anywhere else in this
    /// score — goals' scorer, fouls' player — keyed by that same
    /// (match-scoped) player id. Same mechanism and rationale as
    /// `FootballScore.players`.
    players: HashMap<String, RosterPreviewPlayer>,
}

/// The sport a match was played in. Determines the expected `Score` shape
/// (e.g. racket sports use `Score::Sets`, football uses `Score::Football`,
/// and cricket uses `Score::Cricket`). Extend as more sports are supported.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum MatchType {
    Tennis,
    Badminton,
    Squash,
    TableTennis,
    Football,
    Cricket,
    Netball,
    /// Fallback for sports not yet modelled explicitly.
    Other,
}

/// Lifecycle state of a match. Independent of score confirmation: a `Completed`
/// match may still have an unconfirmed score.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum MatchStatus {
    /// Created, not yet played. `starts_at` is in the future.
    Scheduled,
    /// Currently being played (and, optionally, scored live).
    InProgress,
    /// Finished.
    Completed,
    /// Called off.
    Cancelled,
}

/// A geographic location. Optional on a match.
#[derive(Object)]
struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Object)]
struct Match {
    id: String,
    name: String,
    description: String,
    match_type: MatchType,
    status: MatchStatus,
    /// When the match starts / started. Used as the scheduled time for upcoming
    /// matches and the played time for past ones.
    starts_at: chrono::DateTime<chrono::Utc>,
    /// Where the match is / was played. Optional.
    location: Option<Location>,
    header_photos: Vec<Photo>,
    /// The opposing sides (always present — score needs them).
    sides: Vec<MatchSide>,
    /// Flat roster of everyone in the match. Each player links to a side via
    /// `side_id` (None while invited but not yet assigned).
    players: Vec<MatchPlayer>,
    /// The agreed, official result. Present once a submission is fully
    /// confirmed; None until then.
    confirmed_score: Option<ConfirmedScore>,
    /// A submitted score awaiting confirmation, shown to participants as a
    /// "confirm this result?" prompt. May be present alongside `confirmed_score`
    /// when a correction to an already-agreed score has been proposed.
    pending_score: Option<PendingScore>,
    /// Like/comment counts and the viewer's own like state, so feed and detail
    /// cards render without extra requests.
    social: MatchSocial,
    /// Sport-specific format/rules (half length, overs limit, penalty runs,
    /// ...), if configured. `None` means the creator didn't set one — clients
    /// should fall back to their own sensible per-sport defaults.
    format: Option<MatchFormat>,
}

/// Social engagement summary for a match. Counts plus whether the requesting
/// user has liked it, for rendering feed/detail cards in one fetch.
#[derive(Object)]
struct MatchSocial {
    like_count: u32,
    comment_count: u32,
    /// Whether the requesting user has liked this match.
    i_liked: bool,
}

/// A comment on a match. Threads are two levels: a top-level comment
/// (`parent_id` None) may have replies (`parent_id` set); replies cannot
/// themselves be replied to.
#[derive(Object)]
struct Comment {
    id: String,
    /// The top-level comment this is a reply to. None = a top-level comment.
    parent_id: Option<String>,
    /// The author's profile, for rendering name/avatar inline. None on a
    /// tombstone (a deleted comment kept because it has replies).
    author: Option<UserProfile>,
    /// The comment body. None on a tombstone — clients render "[deleted]".
    text: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    /// When the comment was last edited. None if never edited — clients can show
    /// an "edited" marker when this is present.
    edited_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Number of replies. Always 0 for a reply (replies can't be replied to).
    reply_count: u32,
    /// When the comment was deleted. When set, this is a tombstone: `author` and
    /// `text` are null and the client shows "[deleted]", but the row is kept so
    /// its replies remain visible. A deleted comment with no replies is removed
    /// entirely rather than tombstoned.
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Object)]
struct CreateCommentInput {
    text: String,
    /// To reply, set this to a top-level comment's id. Omit for a top-level
    /// comment. Replying to a reply is rejected.
    parent_id: Option<String>,
}

#[derive(Object)]
struct UpdateCommentInput {
    text: String,
}

/// One page of comments. `next_cursor` absent => end.
#[derive(Object)]
struct CommentPage {
    items: Vec<Comment>,
    next_cursor: Option<String>,
}

/// The settled result of a match.
#[derive(Object)]
struct ConfirmedScore {
    score: Score,
    /// Explicit winner so clients never reverse-engineer the result from sets.
    winner_side_id: Option<String>,
}

/// A score awaiting confirmation. Carries the submission id (so a participant
/// can confirm/dispute exactly this submission) and the per-side confirmation
/// progress so far.
#[derive(Object)]
struct PendingScore {
    submission_id: String,
    score: Score,
    winner_side_id: Option<String>,
    /// Which sides have confirmed so far. A submission becomes the
    /// `confirmed_score` once every side has confirmed.
    confirmations: Vec<ScoreConfirmation>,
}

/// One side's confirmation of a submitted score.
#[derive(Object)]
struct ScoreConfirmation {
    side_id: String,
    /// The player (member id) who confirmed on the side's behalf.
    confirmed_by_player_id: String,
    confirmed_at: chrono::DateTime<chrono::Utc>,
}

/// Status of a single score submission in the history.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum ScoreSubmissionStatus {
    /// Awaiting confirmations.
    Pending,
    /// Every side confirmed — this is (or was) the agreed score.
    Confirmed,
    /// A side disputed it; superseded by a later submission.
    Disputed,
    /// Replaced by a newer submission before being resolved.
    Superseded,
}

/// A historical score submission and the responses it received. Surfaced via the
/// score-history endpoint, not on `Match` (which shows only the resolved
/// confirmed/pending scores).
#[derive(Object)]
struct ScoreSubmission {
    id: String,
    score: Score,
    winner_side_id: Option<String>,
    status: ScoreSubmissionStatus,
    /// Member id of the player who submitted this score.
    submitted_by_player_id: String,
    submitted_at: chrono::DateTime<chrono::Utc>,
    /// Confirm/dispute responses this submission received, in order.
    responses: Vec<ScoreSubmissionResponse>,
}

#[derive(Object)]
struct ScoreSubmissionResponse {
    side_id: String,
    responded_by_player_id: String,
    response: ScoreResponseKind,
    responded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Enum, Debug)]
#[oai(rename_all = "snake_case")]
enum ScoreResponseKind {
    Confirm,
    Dispute,
}

/// Confirm or dispute a specific score submission. Targeting a submission id
/// (rather than "the current score") prevents responding to a score that has
/// since been replaced.
#[derive(Object)]
struct RespondToScoreInput {
    response: ScoreResponseKind,
}

/// An ad-hoc player to add to a match at update time (e.g. a ringer who turned
/// up but was never invited). No invitation is created.
#[derive(Object)]
struct AddMatchPlayerInput {
    /// Existing Agon user to add, if known.
    user_id: Option<String>,
    /// Display name, for an external person with no account.
    display_name: Option<String>,
    /// The side they played for. None to add them unassigned.
    side_id: Option<String>,
}

/// Reassign an existing player to a side (or correct who played for whom).
#[derive(Object)]
struct SetPlayerSideInput {
    /// The player's member id.
    player_id: String,
    /// The side to place them on. None to unassign.
    side_id: Option<String>,
}

/// Rename an existing side. `name: None` clears any custom name, falling
/// back to the priority chain in `MatchSide::name`'s doc comment (the sole
/// player's name, then the linked team's name, then a neutral default) —
/// the same validation as at create time applies: a name alongside a
/// `team_id` is only allowed when another side shares that team.
#[derive(Object)]
struct UpdateMatchSideNameInput {
    side_id: String,
    name: Option<String>,
}

/// A side to create as part of a new match. The server assigns the real side id;
/// `client_id` lets the request reference this side from `invites` and `score`.
#[derive(Object)]
struct CreateMatchSideInput {
    /// Caller-chosen temporary id, unique within the request, used to wire up
    /// invites and score entries to this side before real ids exist.
    client_id: String,
    team_id: Option<String>,
    /// A custom name for this side. Rejected (validation error) alongside a
    /// `team_id` unless another side in the same request shares that team —
    /// the team is normally the source of truth for the side's name, but two
    /// sides sharing one team need a name each to be told apart.
    name: Option<String>,
}

/// An invitation to create with the match. `side_client_id` references a
/// `CreateMatchSideInput.client_id` (None = invite without assigning a side).
#[derive(Object)]
struct CreateMatchInviteInput {
    side_client_id: Option<String>,
    /// Real Agon users, referenced by their own (already-known, stable) user
    /// id — a create-time score's goal/card/batting detail can reference one
    /// of these ids directly, no separate client id needed.
    invited_user_ids: Vec<String>,
    /// Guests with no Agon account, each given a caller-chosen temporary id
    /// (unique within the request) so a create-time score's detail can
    /// reference them too, the same way `CreateMatchSideInput.client_id`
    /// lets a score reference a side before it has a real id.
    invited_externals: Vec<CreateMatchExternalInviteInput>,
}

#[derive(Object)]
struct CreateMatchExternalInviteInput {
    client_id: String,
    name: String,
}

#[derive(Object)]
struct CreateMatchInput {
    name: String,
    description: String,
    match_type: MatchType,
    /// When the match starts / started. Must be in the future when no `score` is
    /// supplied (an upcoming, Scheduled match) and in the past when a `score` is
    /// supplied (an already-played, Completed match).
    starts_at: chrono::DateTime<chrono::Utc>,
    location: Option<Location>,
    /// The opposing sides. At least two are required.
    sides: Vec<CreateMatchSideInput>,
    /// Players to invite up front. Optional — more can be added later.
    invites: Vec<CreateMatchInviteInput>,
    /// If set, the caller is added to the match as an already-accepted player on
    /// this side (references a `CreateMatchSideInput.client_id`). This is how the
    /// creator opts to play in their own match — no self-invitation is created.
    /// None means the caller creates the match but doesn't play in it.
    creator_side_client_id: Option<String>,
    /// If present, the match is created already played (status `Completed`) and
    /// the score enters the confirmation flow. `side_id`s reference the created
    /// sides' `client_id`s. Absent => an upcoming match.
    score: Option<Score>,
    winner_side_id: Option<String>,
    /// Header images for the match. Each references an `Asset` the caller created
    /// via `POST /assets` (purpose `match_header`) and uploaded; the server
    /// rejects any that aren't `Uploaded` and owned by the caller, and resolves
    /// them to stored URLs. Omit/null for no header.
    header_photo_asset_ids: Option<Vec<String>>,
    /// Sport-specific format/rules. Optional — omit for the app's own
    /// defaults; must match `match_type`'s sport if supplied.
    format: Option<MatchFormat>,
}

/// The organiser's one-stop update for a match: edit metadata, reconcile the
/// actual line-up, and/or record the result — all in one validated, atomic call.
/// All fields optional; only supplied fields take effect. The server validates
/// the *resulting* state (legal status transition, every scored player has a
/// side, a cancelled match can't be scored, etc.) and rejects the whole request
/// on any violation.
///
/// Score handling is set-vs-submission aware: a supplied `score` only creates a
/// new score submission when it differs from the current one — re-sending the
/// same score while editing, say, the time is a no-op for the score. A genuinely
/// new/changed score creates a submission and (re)starts confirmation.
/// Confirming/disputing a submission is a *different actor's* action and lives on
/// its own endpoint, not here.
#[derive(Object)]
struct UpdateMatchInput {
    name: Option<String>,
    description: Option<String>,
    starts_at: Option<chrono::DateTime<chrono::Utc>>,
    location: Option<Location>,
    /// Move the match through its lifecycle (e.g. cancel).
    status: Option<MatchStatus>,
    /// Ad-hoc players who actually played but weren't invited (e.g. ringers).
    added_players: Option<Vec<AddMatchPlayerInput>>,
    /// Reassign existing players to sides (late changes to who played for whom).
    side_assignments: Option<Vec<SetPlayerSideInput>>,
    /// Drop players from the roster entirely, by member id (e.g. someone added
    /// by mistake, or who dropped out before the match). Unlike `side_assignments`
    /// with a `None` side, this removes the player record itself rather than
    /// just unassigning them from a side.
    removed_player_ids: Option<Vec<String>>,
    /// Rename one or more of the match's sides — the custom name given at
    /// creation (or a previous edit here). Only the sides listed are
    /// touched; every other side's name is left alone.
    side_names: Option<Vec<UpdateMatchSideNameInput>>,
    /// The result. Creates a score submission when changed; for a not-yet-played
    /// match this also completes it. `side_id`s reference the match's sides.
    /// Required to complete a match — there is no server-side fallback if
    /// it's omitted, even for a live-scored match (see `override_live_score`
    /// for submitting one the server disagrees with).
    score: Option<Score>,
    /// For a live-scored football/cricket/netball match: submit `score` even
    /// though it doesn't match what the server derives from the match's own
    /// persisted live detail. Without this, a mismatch is rejected (409)
    /// rather than silently accepted, so `score` staying in sync with live
    /// scoring is the default; this is the explicit escape hatch for a
    /// deliberate correction. Checked whenever the match has live detail
    /// recorded, not just when the submission completes the match — so a
    /// score submitted while live scoring is still in progress (e.g. from
    /// the general result editor instead of finishing the live-scored game)
    /// is held to the same standard. Ignored when there's no live detail to
    /// disagree with.
    override_live_score: Option<bool>,
    winner_side_id: Option<String>,
    /// A separate, fuller version of `score` to persist as the match's
    /// live-scoring record (e.g. attaching a goal-by-goal/ball-by-ball
    /// breakdown to a manual entry without it being what `score` itself
    /// carries). Writes straight to the same record `GET /matches/:id/score`
    /// reads and live scoring incrementally updates — has no bearing on
    /// confirmation, which is `score`'s job alone.
    detailed_score: Option<Score>,
    /// Replace the match's header images. Each references an `Asset` (purpose
    /// `match_header`) the caller created and uploaded. `Some([])` clears the
    /// headers; `None` leaves them unchanged. Any asset not `Uploaded`/owned by
    /// the caller rejects the whole request.
    header_photo_asset_ids: Option<Vec<String>>,
    /// Replace the match's format/rules. `None` leaves it unchanged; must
    /// match the match's sport if supplied.
    format: Option<MatchFormat>,
}

/// A single entry in the feed. Modelled as a union so new item types
/// (member joined, achievements, etc.) can be added without breaking clients.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
enum FeedItem {
    Match(FeedMatch),
}

/// A match as it appears in the feed — everything [`Match`] has except the
/// full roster (`players`), which the feed never renders and so never fetches
/// (see `Dao::batch_get_match_summaries`). In its place, `known_participants`:
/// a capped, denormalized hint ("people you follow are playing") computed at
/// fan-out time. For the full roster, fetch the match itself via
/// `GET /matches/:match_id`.
#[derive(Object)]
struct FeedMatch {
    id: String,
    name: String,
    description: String,
    match_type: MatchType,
    status: MatchStatus,
    /// When the match starts / started.
    starts_at: chrono::DateTime<chrono::Utc>,
    location: Option<Location>,
    header_photos: Vec<Photo>,
    /// The opposing sides (always present — score needs them).
    sides: Vec<MatchSide>,
    /// Up to a few participants the viewer follows, so the card can show
    /// "you know who's playing" without the full roster. Always populated
    /// with `is_followed_by_me: true` (that's why they're on the list) —
    /// empty if the viewer doesn't follow any participant directly (e.g. they
    /// only follow an involved team), or for the viewer's own matches.
    known_participants: Vec<UserProfile>,
    /// How many of the match's participants the viewer follows, in total.
    /// `known_participants` itself is capped (see
    /// `agon_core::dao::audience::MAX_KNOWN_PLAYERS`), so this is what a
    /// client uses to render "+N more" beyond the hydrated list.
    known_participants_count: u32,
    /// The side *the caller themselves* plays on, if they're a participant in
    /// this match — `None` if they're not playing (they're seeing this card
    /// via a follow) or not yet assigned a side. Lets a client resolve the
    /// score confirm/dispute prompt without the full roster `Match` carries.
    viewer_side_id: Option<String>,
    confirmed_score: Option<ConfirmedScore>,
    pending_score: Option<PendingScore>,
    social: MatchSocial,
    format: Option<MatchFormat>,
}

/// One page of the feed. `next_cursor` is an opaque token; when it is
/// absent/null the client has reached the end of the feed.
#[derive(Object)]
struct FeedPage {
    items: Vec<FeedItem>,
    next_cursor: Option<String>,
}

/// A queried `participant`'s result in a match — resolved from the search
/// index's outcome buckets (see `agon_core::search::SearchClient::search_matches`),
/// no extra lookup.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum MatchOutcome {
    Won,
    Lost,
    Draw,
}

/// A match as it appears in discovery/search results — the same trimmed
/// shape as the feed's `FeedMatch` (no full roster; `MatchSide.roster_preview`
/// covers "show players directly" for small sides), minus the feed-specific
/// `known_participants` (a per-viewer fan-out concept that doesn't apply to
/// a generic search hit) and `viewer_side_id` (no per-viewer audience to
/// source it from cheaply here — see `outcome` below for what a `participant`
/// filter *can* get for free instead).
#[derive(Object)]
struct SearchMatch {
    id: String,
    name: String,
    description: String,
    match_type: MatchType,
    status: MatchStatus,
    starts_at: chrono::DateTime<chrono::Utc>,
    location: Option<Location>,
    header_photos: Vec<Photo>,
    sides: Vec<MatchSide>,
    /// The `participant` query parameter's result in this match — `None` if
    /// no `participant` filter was supplied, or if the match has no
    /// confirmed result yet (not a draw, just undecided).
    outcome: Option<MatchOutcome>,
    confirmed_score: Option<ConfirmedScore>,
    pending_score: Option<PendingScore>,
    social: MatchSocial,
    format: Option<MatchFormat>,
}

/// One page of matches from discovery/search. `next_cursor` absent => end.
#[derive(Object)]
struct MatchPage {
    items: Vec<SearchMatch>,
    next_cursor: Option<String>,
}

/// One page of users (e.g. followers / following). `next_cursor` absent => end.
#[derive(Object)]
struct UserPage {
    items: Vec<UserProfile>,
    next_cursor: Option<String>,
}

/// One page of invitations (the inbox). `next_cursor` absent => end.
#[derive(Object)]
struct InvitationPage {
    items: Vec<InvitationDetail>,
    next_cursor: Option<String>,
}

/// One page of teams (my teams / search). `next_cursor` absent => end.
#[derive(Object)]
struct TeamPage {
    items: Vec<TeamListItem>,
    next_cursor: Option<String>,
}

/// One page of a team's members (`GET /teams/{team_id}/members`).
/// `next_cursor` absent => end.
#[derive(Object)]
struct TeamMemberPage {
    items: Vec<TeamMember>,
    next_cursor: Option<String>,
}

#[derive(ApiResponse)]
enum GetUserResponse {
    #[oai(status = 200)]
    User(Json<User>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetUserProfileResponse {
    #[oai(status = 200)]
    User(Json<UserProfile>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum CreateUserResponse {
    #[oai(status = 200)]
    User(Json<User>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum UpdateUserResponse {
    #[oai(status = 200)]
    User(Json<User>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum CreateAssetResponse {
    #[oai(status = 200)]
    Asset(Json<Asset>),

    /// The content type isn't allowed for the requested purpose.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetAssetResponse {
    #[oai(status = 200)]
    Asset(Json<Asset>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum UpdateTeamResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum RemoveTeamMemberResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum RevokeInvitationResponse {
    /// The invitation was revoked (or was already gone).
    #[oai(status = 204)]
    Ok,

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The caller is not allowed to revoke this invitation.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),
}

#[derive(ApiResponse)]
enum SearchUsersResponse {
    #[oai(status = 200)]
    Users(Json<Vec<UserProfile>>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetFeedResponse {
    #[oai(status = 200)]
    Feed(Json<FeedPage>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetMatchResponse {
    #[oai(status = 200)]
    Match(Json<Match>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListMatchesResponse {
    #[oai(status = 200)]
    Matches(Json<MatchPage>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum CreateMatchResponse {
    #[oai(status = 200)]
    Match(Json<Match>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum UpdateMatchResponse {
    #[oai(status = 200)]
    Match(Json<Match>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// The caller is not a participant in this match, so may not edit it.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The submitted `score` doesn't match what the server derives from the
    /// match's own persisted live detail. Refresh and resubmit, or set
    /// `override_live_score` to submit it anyway.
    #[oai(status = 409)]
    Conflict(PlainText<String>),
}

#[derive(ApiResponse)]
enum SubmitScoreResponse {
    #[oai(status = 200)]
    Submission(Json<ScoreSubmission>),

    /// The score is invalid (e.g. references a player without a side, or the
    /// match is Scheduled/Cancelled and cannot be scored).
    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum RespondToScoreResponse {
    #[oai(status = 200)]
    Submission(Json<ScoreSubmission>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The caller is not a participant of the side they are responding for, or
    /// the submission has already been superseded.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListScoreSubmissionsResponse {
    #[oai(status = 200)]
    Submissions(Json<Vec<ScoreSubmission>>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

/// Result of liking/unliking a match.
#[derive(ApiResponse)]
enum LikeResponse {
    /// The like now exists (like) or no longer exists (unlike).
    #[oai(status = 204)]
    Ok,

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListLikesResponse {
    #[oai(status = 200)]
    Users(Json<UserPage>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListCommentsResponse {
    #[oai(status = 200)]
    Comments(Json<CommentPage>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum CreateCommentResponse {
    #[oai(status = 200)]
    Comment(Json<Comment>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum UpdateCommentResponse {
    #[oai(status = 200)]
    Comment(Json<Comment>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The caller is not the comment's author.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),
}

#[derive(ApiResponse)]
enum DeleteCommentResponse {
    #[oai(status = 204)]
    Ok,

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The caller is not the comment's author.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListNotificationsResponse {
    #[oai(status = 200)]
    Notifications(Json<NotificationPage>),
}

#[derive(ApiResponse)]
enum UnreadCountResponse {
    #[oai(status = 200)]
    Count(Json<UnreadCount>),
}

#[derive(ApiResponse)]
enum MarkNotificationReadResponse {
    #[oai(status = 204)]
    Ok,

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

/// The client platform a registered push token belongs to.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum DevicePlatform {
    Web,
    Android,
    Ios,
}

#[derive(Object)]
struct RegisterDeviceInput {
    /// The FCM registration token issued to this device/browser.
    push_token: String,
    platform: DevicePlatform,
}

#[derive(Object)]
struct UnregisterDeviceInput {
    push_token: String,
}

#[derive(ApiResponse)]
enum RegisterDeviceResponse {
    #[oai(status = 204)]
    Ok,

    /// The user already has the maximum number of registered devices.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum UnregisterDeviceResponse {
    #[oai(status = 204)]
    Ok,

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetMatchScoreResponse {
    #[oai(status = 200)]
    Score(Json<Score>),

    /// The match exists but has no score recorded.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum AppendLiveEventsResponse {
    #[oai(status = 200)]
    Ok(Json<LiveScoreSnapshot>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// Only a participant may record live events for a match.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// `expected_last_seq` doesn't match the match's current log tip —
    /// another device advanced it, or this is a stale retry. The caller
    /// should re-fetch the log/state and reconcile before retrying.
    #[oai(status = 409)]
    Conflict(PlainText<String>),
}

/// One page of a match's live event log, oldest first. `next_cursor` absent
/// => end.
#[derive(Object)]
struct LiveEventPage {
    items: Vec<LiveEvent>,
    next_cursor: Option<String>,
}

/// The match's current live-scoring counter — `GET
/// /matches/:match_id/live/seq`. A client with no cached mutation response
/// to seed `expected_last_seq` from (a fresh page load, a different device)
/// needs this: the physical event log's own max seq (`GET
/// /matches/:match_id/live/events`) is *not* a safe substitute once any
/// event has ever been undone — it permanently understates the true
/// counter from that point on (see `Dao::delete_live_event`'s doc comment).
#[derive(Object)]
struct LiveSeq {
    last_seq: u32,
}

#[derive(ApiResponse)]
enum GetLiveSeqResponse {
    #[oai(status = 200)]
    Ok(Json<LiveSeq>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListLiveEventsResponse {
    #[oai(status = 200)]
    Events(Json<LiveEventPage>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum DeleteLiveEventResponse {
    /// Deleted; the returned state has already been recomputed without it.
    #[oai(status = 200)]
    Ok(Json<LiveScoreSnapshot>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// Either the match or that specific seq doesn't exist.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum AmendLiveEventResponse {
    /// Amended; the returned state has already been recomputed with the
    /// corrected content.
    #[oai(status = 200)]
    Ok(Json<LiveScoreSnapshot>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// Either the match or that specific seq doesn't exist.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum CreateTeamResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 400)]
    ValidationError(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListTeamsResponse {
    #[oai(status = 200)]
    Teams(Json<TeamPage>),
}

#[derive(ApiResponse)]
enum GetTeamResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListTeamMembersResponse {
    #[oai(status = 200)]
    Members(Json<TeamMemberPage>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum AddTeamMembersResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum AddInvitationsResponse {
    #[oai(status = 200)]
    Invitations(Json<Vec<Invitation>>),

    /// The caller is not a participant in this match, so may not invite others.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),

    /// The team or match being invited to was not found.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListInvitationsResponse {
    #[oai(status = 200)]
    Invitations(Json<InvitationPage>),
}

#[derive(ApiResponse)]
enum GetInvitationResponse {
    #[oai(status = 200)]
    Invitation(Json<InvitationDetail>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum RespondToInvitationResponse {
    #[oai(status = 200)]
    Invitation(Json<Invitation>),

    /// The invitation does not exist.
    #[oai(status = 404)]
    NotFound(PlainText<String>),

    /// The caller is not the user this invitation targets.
    #[oai(status = 403)]
    Forbidden(PlainText<String>),
}

#[derive(ApiResponse)]
enum RespondByTokenResponse {
    #[oai(status = 200)]
    Invitation(Json<Invitation>),

    /// No invitation matches the supplied token.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

/// Result of a follow/unfollow action.
#[derive(ApiResponse)]
enum FollowResponse {
    /// The follow edge now exists (follow) or no longer exists (unfollow).
    #[oai(status = 204)]
    Ok,

    /// The user or team being followed was not found.
    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListFollowsResponse {
    #[oai(status = 200)]
    Users(Json<UserPage>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[OpenApi]
impl Api {
    #[oai(path = "/ping", method = "get")]
    async fn ping(&self) -> Result<PlainText<String>> {
        Ok(PlainText("Pong".to_string()))
    }

    /// Resolve the authenticated caller's stable internal user id from their JWT
    /// `sub` (via the `AUTH#<sub>` mapping). This is the id everything downstream
    /// keys off — never the raw `sub`, so the auth provider can change without
    /// touching stored data. Returns 401 if the `sub` maps to no user (i.e. the
    /// caller is authenticated but hasn't completed signup via `POST /users`).
    async fn require_uid(&self, dao: &dao::Dao, jwt: &JwtClaims) -> Result<String> {
        dao.get_user_id_by_sub(&jwt.sub)
            .await
            .map_err(dao_internal)?
            .ok_or_else(|| Error::from_string("user not found", StatusCode::UNAUTHORIZED))
    }

    #[oai(path = "/users/me", method = "get")]
    async fn get_current_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<GetUserResponse> {
        info!("Getting current user");
        // Resolve sub -> internal id. A caller who hasn't signed up yet maps to
        // nothing → 404 (the existing contract for /users/me).
        let uid = match dao
            .get_user_id_by_sub(&jwt_data.sub)
            .await
            .map_err(dao_internal)?
        {
            Some(id) => id,
            None => {
                return Ok(GetUserResponse::NotFound(PlainText(
                    "user not found".into(),
                )));
            }
        };
        let record = match dao.get_user(&uid).await.map_err(dao_internal)? {
            Some(r) => r,
            None => {
                return Ok(GetUserResponse::NotFound(PlainText(
                    "user not found".into(),
                )));
            }
        };
        // Own profile: not "followed by me".
        let profile = user_profile_from_record(&record, false);
        Ok(GetUserResponse::User(Json(User {
            email: record.email,
            profile,
        })))
    }

    #[oai(path = "/users/me", method = "patch")]
    async fn update_current_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<UpdateUserInput>,
    ) -> Result<UpdateUserResponse> {
        info!("Updating current user profile");
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Resolve an attached asset id to its stored URL (must be Uploaded, owned
        // by the caller, and of `profile_image` purpose). Some(Some(url)) = set
        // image; None = leave as-is.
        let resolved_image: Option<Option<String>> = match &input.profile_image_asset_id {
            Some(asset_id) => {
                let ids = std::slice::from_ref(asset_id);
                match resolve_asset_urls(dao, &uid, "profile_image", ids).await? {
                    Ok(resolved) => Some(resolved.into_iter().next().map(|(_, url)| url)),
                    Err(msg) => {
                        return Ok(UpdateUserResponse::ValidationError(PlainText(msg)));
                    }
                }
            }
            None => None,
        };

        dao.update_user_profile(
            &uid,
            input.name.as_deref(),
            resolved_image.as_ref().map(|o| o.as_deref()),
        )
        .await
        .map_err(|e| match e {
            dao::DaoError::NotFound(_) => {
                Error::from_string("user not found", StatusCode::NOT_FOUND)
            }
            other => dao_internal(other),
        })?;

        // Return the updated profile.
        let record = dao
            .get_user(&uid)
            .await
            .map_err(dao_internal)?
            .ok_or_else(|| Error::from_string("user not found", StatusCode::NOT_FOUND))?;
        let profile = user_profile_from_record(&record, false);
        Ok(UpdateUserResponse::User(Json(User {
            email: record.email,
            profile,
        })))
    }

    #[oai(path = "/assets", method = "post")]
    async fn create_asset(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateAssetInput>,
    ) -> Result<CreateAssetResponse> {
        info!("Creating asset for content type {}", input.content_type);
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Validate the content type against the purpose (images only for now).
        if !input.content_type.starts_with("image/") {
            return Ok(CreateAssetResponse::ValidationError(PlainText(
                "content_type not allowed for this purpose".into(),
            )));
        }

        // Enforce a size limit up front: the declared length must be positive and
        // within the max. It's baked into the presigned PUT below, so S3 also
        // rejects an upload whose actual size differs from what was declared.
        if input.content_length <= 0 || input.content_length > MAX_UPLOAD_BYTES {
            return Ok(CreateAssetResponse::ValidationError(PlainText(format!(
                "content_length must be between 1 and {MAX_UPLOAD_BYTES} bytes"
            ))));
        }

        let id = new_id();
        // Provider-agnostic object key. A storage event later flips the asset to
        // Uploaded and sets the URL — none of the provider details leak here.
        let storage_key = format!("{}/{}", upload_purpose_str(&input.purpose), id);
        let record = dao::records::AssetRecord {
            id: id.clone(),
            owner_user_id: uid,
            purpose: upload_purpose_str(&input.purpose).to_string(),
            content_type: input.content_type,
            content_length: input.content_length,
            status: String::from("pending"),
            storage_key,
            url: None,
            created_at: now_iso(),
        };
        dao.create_asset(&record).await.map_err(dao_internal)?;
        Ok(CreateAssetResponse::Asset(Json(
            asset_from_record(assets, &record).await,
        )))
    }

    #[oai(path = "/assets/:asset_id", method = "get")]
    async fn get_asset(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(asset_id): Path<String>,
    ) -> Result<GetAssetResponse> {
        info!("Getting asset {asset_id}");
        match dao.get_asset(&asset_id).await.map_err(dao_internal)? {
            // Pending assets get a fresh presigned upload target on each read
            // (the previous one may have expired) — that is the retry mechanism.
            Some(record) => Ok(GetAssetResponse::Asset(Json(
                asset_from_record(assets, &record).await,
            ))),
            None => Ok(GetAssetResponse::NotFound(PlainText(
                "asset not found".into(),
            ))),
        }
    }

    #[oai(path = "/users/:user_id", method = "get")]
    async fn get_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
    ) -> Result<GetUserProfileResponse> {
        info!("Getting user {user_id}");
        let record = match dao.get_user(&user_id).await.map_err(dao_internal)? {
            Some(r) => r,
            None => {
                return Ok(GetUserProfileResponse::NotFound(PlainText(
                    "user not found".into(),
                )));
            }
        };
        let caller_uid = self.require_uid(dao, &jwt_data).await?;
        let is_followed = if caller_uid == user_id {
            false
        } else {
            dao.is_following_user(&caller_uid, &user_id)
                .await
                .map_err(dao_internal)?
        };
        Ok(GetUserProfileResponse::User(Json(
            user_profile_from_record(&record, is_followed),
        )))
    }

    #[oai(path = "/users", method = "post")]
    async fn create_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateUserInput>,
    ) -> Result<CreateUserResponse> {
        info!("Creating user");
        let input = input.0;
        // Identity comes entirely from the verified JWT: `sub` is the id, `email`
        // is the trusted email claim. We do NOT accept an email from the body —
        // that would let any authenticated caller sign up as an arbitrary email.
        let email = match jwt_data.email.clone() {
            Some(email) => email,
            None => {
                return Ok(CreateUserResponse::ValidationError(PlainText(
                    "authentication token has no email claim".into(),
                )));
            }
        };
        // The internal id is freshly minted and stable — it is NOT the JWT `sub`.
        // The `sub` is mapped to it via the AUTH# guard so the auth provider can
        // change without rewriting any user-keyed data.
        let record = dao::records::UserRecord {
            id: new_id(),
            email,
            name: input.name,
            profile_image_url: None,
            follower_count: 0,
            following_count: 0,
            unread_count: 0,
            stats: dao::records::UserStatsRecord::default(),
            created_at: now_iso(),
        };
        match dao.create_user(&jwt_data.sub, &record).await {
            Ok(()) => {}
            Err(dao::DaoError::Conflict(_)) => {
                return Ok(CreateUserResponse::ValidationError(PlainText(
                    "a user with that email or subject already exists".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        }
        let profile = user_profile_from_record(&record, false);
        Ok(CreateUserResponse::User(Json(User {
            email: record.email,
            profile,
        })))
    }

    #[oai(path = "/users/search", method = "get")]
    async fn search_users(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(search): Data<&agon_core::search::SearchClient>,
        AuthSchema(jwt_data): AuthSchema,
        #[oai(name = "q")] Query(query): Query<String>,
    ) -> Result<SearchUsersResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Searching users with query: {}", query);

        // Search the users index for matching ids, then hydrate full profiles
        // from DynamoDB (the index stores only id/name). This response is a plain
        // list (no cursor), so we return one page at the default limit.
        let q = agon_core::search::SearchQuery {
            q: query,
            limit: page_limit(None),
            ..Default::default()
        };
        let hits = search
            .search(agon_core::search::Index::Users, &q)
            .await
            .map_err(search_internal)?;

        let profiles = self
            .hydrate_user_profiles(dao, &hits.ids, Some(&uid))
            .await?;
        Ok(SearchUsersResponse::Users(Json(profiles)))
    }

    #[oai(path = "/feed", method = "get")]
    async fn get_user_feed(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 20 —
        /// tighter than other list endpoints, see `FEED_MAX_PAGE_LIMIT`).
        Query(limit): Query<Option<u32>>,
        /// Only include items at or after this time (inclusive).
        Query(from): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Only include items at or before this time (inclusive).
        Query(to): Query<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<GetFeedResponse> {
        info!("Getting caller's social feed");
        let uid = self.require_uid(dao, &jwt_data).await?;

        // The feed is always the authenticated caller's own social feed (matches
        // from people/teams they follow). No user_id / sport filtering here —
        // that is the match-discovery endpoint (GET /matches), served by search.

        let limit = feed_page_limit(limit);

        // Reject an inverted date range early.
        if let (Some(from), Some(to)) = (from, to)
            && from > to
        {
            return Ok(GetFeedResponse::ValidationError(PlainText(
                "`from` must be before `to`".to_string(),
            )));
        }

        // Read the caller's fan-out feed partition (UFEED#<caller>), newest
        // first. The cursor is the DAO's opaque LastEvaluatedKey (400 if
        // malformed). Feed entries are thin pointers, so hydrate each referenced
        // match from DynamoDB — entries never carry stale copies.
        let page = match dao.list_feed(&uid, cursor.as_deref(), limit).await {
            Ok(p) => p,
            Err(dao::DaoError::Malformed(_)) => {
                return Ok(GetFeedResponse::ValidationError(PlainText(
                    "Invalid cursor".to_string(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        };

        // (match_id, known_player_ids) for entries that pass the date filter —
        // `known_player_ids`/`viewer_side_id` are already on the feed entry
        // (denormalized at fan-out time), never fetched here.
        struct EligibleEntry {
            match_id: String,
            known_player_ids: Vec<String>,
            known_player_count: u32,
            viewer_side_id: Option<String>,
        }
        let mut eligible: Vec<EligibleEntry> = Vec::with_capacity(page.items.len());
        for entry in &page.items {
            // Apply the optional date range on the entry's start time (cheap;
            // avoids hydrating matches outside the window).
            if !within_range(&entry.starts_at, from, to) {
                continue;
            }
            // Currently every feed entry references a match.
            if entry.ref_type != "match" {
                continue;
            }
            eligible.push(EligibleEntry {
                match_id: entry.ref_id.clone(),
                known_player_ids: entry.known_player_ids.clone(),
                known_player_count: entry.known_player_count,
                viewer_side_id: entry.viewer_side_id.clone(),
            });
        }
        let match_ids: Vec<String> = eligible.iter().map(|e| e.match_id.clone()).collect();

        // Hydrate every referenced match's meta + sides (never players — the
        // feed doesn't render the full roster, see `FeedMatch`) and this
        // viewer's likes, in two round-trips total, run concurrently. Neither
        // call's DB request count grows with page size.
        let (summaries, liked) = tokio::try_join!(
            dao.batch_get_match_summaries(&match_ids),
            dao.batch_has_liked_matches(&match_ids, &uid),
        )
        .map_err(dao_internal)?;

        // One more round-trip: every user this page needs to hydrate by name/
        // avatar — the union of each entry's `known_participants` and each
        // match's small per-side `roster_preview`s (both id-only; see
        // `MatchSideRecord::roster_preview`'s doc comment on why a live name/
        // avatar beats a possibly-stale cached one). `batch_get_users` chunks
        // internally if the union happens to exceed one `BatchGetItem`, but
        // page size is capped specifically so it normally won't need to.
        let mut user_ids: Vec<String> = Vec::new();
        for entry in &eligible {
            user_ids.extend(entry.known_player_ids.iter().cloned());
        }
        for summary in summaries.values() {
            for side in &summary.sides {
                user_ids.extend(side.roster_preview.iter().filter_map(|p| p.user_id.clone()));
            }
        }
        let users = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;

        // Side names/logos need the same team-meta fallback `Match` gets (via
        // `hydrate_matches`) — without it a team-linked side with no custom
        // name would render blank. `roster_preview` already covers the
        // "sole player's name" case, so only team meta needs a batch fetch.
        let team_ids: Vec<String> = summaries
            .values()
            .flat_map(|s| s.sides.iter().filter_map(|s| s.team_id.clone()))
            .collect();
        let team_metas = self.batch_team_metas(dao, &team_ids).await?;

        let mut built: Vec<FeedMatch> = Vec::with_capacity(eligible.len());
        for entry in &eligible {
            if let Some(summary) = summaries.get(&entry.match_id) {
                let i_liked = liked.contains(&entry.match_id);
                let known_participants = entry
                    .known_player_ids
                    .iter()
                    .filter_map(|uid| users.get(uid))
                    .map(|record| user_profile_from_record(record, true))
                    .collect();
                let mut m = feed_match_from_records(
                    &summary.match_,
                    &summary.sides,
                    &users,
                    known_participants,
                    entry.known_player_count,
                    entry.viewer_side_id.clone(),
                    i_liked,
                );
                Self::resolve_side_names_from_cache(
                    &mut m.sides,
                    entry.viewer_side_id.as_deref(),
                    &team_metas,
                );
                sign_feed_match_headers(assets, &mut m);
                built.push(m);
            }
        }

        // `confirmed_score`/`pending_score`'s scorer/batter names — same
        // batched-across-the-page treatment as everything else here, not a
        // per-match live-score fetch (see
        // `hydrate_confirmed_pending_score_players`'s doc comment).
        let mut score_refs: Vec<_> = built
            .iter_mut()
            .map(|m| (m.id.as_str(), &mut m.confirmed_score, &mut m.pending_score))
            .collect();
        self.hydrate_confirmed_pending_score_players(dao, &mut score_refs)
            .await?;

        let items = built.into_iter().map(FeedItem::Match).collect();

        Ok(GetFeedResponse::Feed(Json(FeedPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/matches", method = "get")]
    async fn list_matches(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(search): Data<&agon_core::search::SearchClient>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        /// Free-text query over match name / participants.
        #[oai(name = "q")]
        Query(query): Query<Option<String>>,
        /// Only matches this user played in (member id or user id).
        Query(participant): Query<Option<String>>,
        /// Only matches these teams were involved in (a side's `team_id`).
        /// One id powers a team profile's matches tab, the same way
        /// `participant` powers a user profile's; two or more, combined via
        /// `team_match`, power team-vs-team lookups (e.g. head-to-head).
        // `Vec<T>` query params are `required` by default in the generated
        // schema even though an absent one just parses as empty — `default`
        // marks it optional there too (falling back to `Vec::default()`,
        // the same empty-vec behavior), so existing callers that don't pass
        // `team_id` at all (e.g. a plain `participant` search) still
        // typecheck against the generated client.
        #[oai(name = "team_id", default)]
        Query(team_ids): Query<Vec<String>>,
        /// How multiple `team_id` values combine: `any` (default) matches a
        /// game involving at least one of them, `all` matches a game
        /// involving every one of them (head-to-head between exactly two
        /// teams is the `all` case). Ignored with fewer than two `team_id`s.
        Query(team_match): Query<Option<TeamMatchMode>>,
        /// Only matches of this sport.
        Query(match_type): Query<Option<MatchType>>,
        /// Only matches at or after this time (inclusive).
        Query(from): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Only matches at or before this time (inclusive).
        Query(to): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListMatchesResponse> {
        info!("Searching matches");
        let caller_uid = self.require_uid(dao, &jwt_data).await?;

        // Match discovery is served by the search index (Meilisearch), NOT
        // DynamoDB — it supports arbitrary combinations of text / participant /
        // team / sport / date-range with date sorting. This is distinct from
        // GET /feed (the caller's social feed). Powers the profile "recent
        // activity" view (participant = the profile's user), a team profile's
        // matches tab (one team_id), team-vs-team / head-to-head lookups (two+
        // team_ids with team_match=all), and general match search.
        if let (Some(from), Some(to)) = (from, to)
            && from > to
        {
            return Ok(ListMatchesResponse::ValidationError(PlainText(
                "`from` must be before `to`".to_string(),
            )));
        }

        let offset = match search_offset(cursor.as_deref()) {
            Ok(o) => o,
            Err(()) => {
                return Ok(ListMatchesResponse::ValidationError(PlainText(
                    "Invalid cursor".to_string(),
                )));
            }
        };

        // Build the Meilisearch filter from the supplied facets. The date range
        // filters on `starts_at_ts` (a numeric Unix timestamp), because
        // Meilisearch's `>=` / `<=` operators are numeric-only — they can't
        // compare the ISO-8601 `starts_at` string (that raised
        // `invalid_search_filter: invalid float literal`).
        let mut clauses: Vec<String> = Vec::new();
        if let Some(mt) = &match_type {
            clauses.push(format!("sport = \"{}\"", match_type_tag(mt)));
        }
        if let Some(p) = &participant {
            clauses.push(format!("participant_ids = \"{p}\""));
        }
        if !team_ids.is_empty() {
            // `team_ids` is a filterable array attribute, so each `team_ids =
            // "<id>"` clause independently asks "does this match's team_ids
            // array contain <id>". ANDing several such clauses together
            // therefore requires the array to contain *every* id — exactly
            // head-to-head ("both these teams played in this match") — while
            // ORing requires just one — "either of these teams played".
            let op = match team_match {
                Some(TeamMatchMode::All) => "AND",
                Some(TeamMatchMode::Any) | None => "OR",
            };
            let team_clause = team_ids
                .iter()
                .map(|t| format!("team_ids = \"{t}\""))
                .collect::<Vec<_>>()
                .join(&format!(" {op} "));
            // Parenthesize so this doesn't interact with the top-level `AND`
            // joining it to the other facets below.
            clauses.push(format!("({team_clause})"));
        }
        if let Some(from) = from {
            clauses.push(format!("starts_at_ts >= {}", from.timestamp()));
        }
        if let Some(to) = to {
            clauses.push(format!("starts_at_ts <= {}", to.timestamp()));
        }
        let filter = (!clauses.is_empty()).then(|| clauses.join(" AND "));

        let q = agon_core::search::SearchQuery {
            q: query.unwrap_or_default(),
            filter,
            sort: vec!["starts_at_ts:desc".to_string()],
            offset,
            limit: page_limit(limit),
        };
        // `participant` doubles as the outcome subject: the same id already
        // scoping the filter is who `search_matches` resolves won/lost/draw
        // for, off the same hit — no second lookup.
        let hits = search
            .search_matches(&q, participant.as_deref())
            .await
            .map_err(search_internal)?;
        let match_ids: Vec<String> = hits.items.iter().map(|h| h.id.clone()).collect();

        // Hydrate each match's meta + sides (never players — search results
        // don't render the full roster, see `SearchMatch`) and this caller's
        // like state, in two round-trips total instead of a `get_match` +
        // `has_liked_match` per hit.
        let (summaries, liked) = tokio::try_join!(
            dao.batch_get_match_summaries(&match_ids),
            dao.batch_has_liked_matches(&match_ids, &caller_uid),
        )
        .map_err(dao_internal)?;

        // Side `roster_preview` entries' live name/avatar, and team meta for
        // the side-name/side-logo fallback chain (same two batch reads the
        // feed makes; no per-viewer `known_participants` here, so no user-id
        // union from that source).
        let mut user_ids: Vec<String> = Vec::new();
        for summary in summaries.values() {
            for side in &summary.sides {
                user_ids.extend(side.roster_preview.iter().filter_map(|p| p.user_id.clone()));
            }
        }
        let team_ids: Vec<String> = summaries
            .values()
            .flat_map(|s| s.sides.iter().filter_map(|s| s.team_id.clone()))
            .collect();
        let (users, team_metas) = tokio::try_join!(
            async { dao.batch_get_users(&user_ids).await.map_err(dao_internal) },
            async { self.batch_team_metas(dao, &team_ids).await },
        )?;

        let mut items: Vec<SearchMatch> = Vec::with_capacity(hits.items.len());
        for hit in &hits.items {
            if let Some(summary) = summaries.get(&hit.id) {
                let i_liked = liked.contains(&hit.id);
                let mut m = search_match_from_records(
                    &summary.match_,
                    &summary.sides,
                    &users,
                    hit.outcome,
                    i_liked,
                );
                // No per-viewer `viewer_side_id` for a search hit, so no
                // "Your side"/"Opposition" — falls to team name / Team A/B.
                Self::resolve_side_names_from_cache(&mut m.sides, None, &team_metas);
                sign_search_match_headers(assets, &mut m);
                items.push(m);
            }
        }

        // `confirmed_score`/`pending_score`'s scorer/batter names — same
        // batched-across-the-page treatment as everything else here, not a
        // per-match live-score fetch (see
        // `hydrate_confirmed_pending_score_players`'s doc comment).
        let mut score_refs: Vec<_> = items
            .iter_mut()
            .map(|m| (m.id.as_str(), &mut m.confirmed_score, &mut m.pending_score))
            .collect();
        self.hydrate_confirmed_pending_score_players(dao, &mut score_refs)
            .await?;

        Ok(ListMatchesResponse::Matches(Json(MatchPage {
            items,
            next_cursor: search_cursor(hits.next_offset),
        })))
    }

    #[oai(path = "/matches", method = "post")]
    async fn create_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateMatchInput>,
    ) -> Result<CreateMatchResponse> {
        info!("Creating match {}", input.name);
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Resolve any header images to asset id + stored URL (must be
        // uploaded, owned by the caller, and of `match_header` purpose).
        let header_asset_ids = input.header_photo_asset_ids.clone().unwrap_or_default();
        let header_photos =
            match resolve_asset_urls(dao, &uid, "match_header", &header_asset_ids).await? {
                Ok(resolved) => resolved
                    .into_iter()
                    .map(|(asset_id, url)| dao::records::HeaderPhotoRecord { asset_id, url })
                    .collect::<Vec<_>>(),
                Err(msg) => return Ok(CreateMatchResponse::ValidationError(PlainText(msg))),
            };

        // A match needs at least two sides for a score to be meaningful.
        if input.sides.len() < 2 {
            return Ok(CreateMatchResponse::ValidationError(PlainText(
                "a match must have at least two sides".to_string(),
            )));
        }

        // A side's name normally comes from its team, so a client-supplied
        // name alongside a `team_id` is redundant — except when two sides
        // share the *same* team (e.g. an intra-squad practice match), where
        // the team name alone can't tell the sides apart and a name is the
        // only way to distinguish them.
        for side in &input.sides {
            if side.team_id.is_none() || side.name.is_none() {
                continue;
            }
            let team_shared = input
                .sides
                .iter()
                .any(|other| other.client_id != side.client_id && other.team_id == side.team_id);
            if !team_shared {
                return Ok(CreateMatchResponse::ValidationError(PlainText(format!(
                    "side `{}` can't have both a name and a team unless another side shares that team",
                    side.client_id
                ))));
            }
        }

        // A supplied format must be for this match's own sport — a football
        // match can't carry cricket's overs-per-innings setting, say.
        if let Some(fmt) = &input.format {
            let tag = match_format_sport_tag(fmt);
            if tag != match_type_tag(&input.match_type) {
                return Ok(CreateMatchResponse::ValidationError(PlainText(format!(
                    "format is for `{tag}` but match is `{}`",
                    match_type_tag(&input.match_type)
                ))));
            }
        }

        // The `starts_at` time must be consistent with whether a result is being
        // recorded: a match created with a score is already played (Completed),
        // so it must have started in the past; one without a score is upcoming
        // (Scheduled), so it must start in the future. This keeps the two create
        // modes ("scheduled" vs "complete") honest.
        let now_ts = chrono::Utc::now();
        if input.score.is_some() {
            if input.starts_at > now_ts {
                return Ok(CreateMatchResponse::ValidationError(PlainText(
                    "a completed match's time must be in the past".to_string(),
                )));
            }
        } else if input.starts_at <= now_ts {
            return Ok(CreateMatchResponse::ValidationError(PlainText(
                "a scheduled match's time must be in the future".to_string(),
            )));
        }

        let now = now_iso();
        let match_id = new_id();

        // Assign a real side id per input side and remember the client_id -> id
        // mapping so invites and the score can be re-pointed at real ids.
        // Keyed by side_id (not a Vec) — sides are embedded on the match
        // record as a map, see `MatchRecord::sides`.
        let mut side_ids: std::collections::HashMap<String, String> = Default::default();
        let mut sides: std::collections::HashMap<String, dao::records::MatchSideRecord> =
            Default::default();
        for side in &input.sides {
            let side_id = new_id();
            side_ids.insert(side.client_id.clone(), side_id.clone());
            sides.insert(
                side_id.clone(),
                dao::records::MatchSideRecord {
                    side_id,
                    team_id: side.team_id.clone(),
                    // Validated above: a name is only allowed here without a
                    // team, or alongside a team shared with another side (to
                    // tell the two apart) — never a lone team-assigned side.
                    name: side.name.clone(),
                    // `Dao::create_match` recomputes both from the players
                    // list it's given, in the same transaction — placeholders.
                    player_count: 0,
                    roster_preview: Vec::new(),
                },
            );
        }

        // Build a player + invitation per invitee. Externals get a minted token
        // and a standalone invitation record; users get a user-kind invitation.
        // Also remember each invitee's "reference" (their own user id for a
        // real Agon user, or their caller-chosen `client_id` for a guest) ->
        // minted player id, so a create-time score's goal/card/batting detail
        // can be re-pointed at real player ids below, the same way side ids
        // already are.
        let mut player_records: Vec<dao::records::MatchPlayerRecord> = Vec::new();
        let mut invitation_records: Vec<dao::records::InvitationRecord> = Vec::new();
        let mut player_ids: std::collections::HashMap<String, String> = Default::default();

        // If the creator opted to play, add them as an already-accepted player on
        // the chosen side — no invitation (they're the source of truth, not an
        // invitee, so there's no pending self-invite / notification to accept).
        // Remember their player id + side so a create-time score submission can be
        // attributed to them and their side pre-confirmed.
        let mut creator_player_id: Option<String> = None;
        let mut creator_side_id: Option<String> = None;
        if let Some(client_id) = &input.creator_side_client_id {
            let Some(side_id) = side_ids.get(client_id).cloned() else {
                return Ok(CreateMatchResponse::ValidationError(PlainText(
                    "creator_side_client_id references an unknown side".into(),
                )));
            };
            let player_id = new_id();
            creator_player_id = Some(player_id.clone());
            creator_side_id = Some(side_id.clone());
            player_ids.insert(uid.clone(), player_id.clone());
            player_records.push(dao::records::MatchPlayerRecord {
                player_id,
                user_id: Some(uid.clone()),
                display_name: None,
                side_id: Some(side_id),
                is_member_of_team: None,
                invitation: None,
            });
        }
        for invite in &input.invites {
            let side_id = invite
                .side_client_id
                .as_ref()
                .and_then(|c| side_ids.get(c).cloned());
            for user_id in &invite.invited_user_ids {
                let (player, inv) = build_invited_player(
                    &match_id,
                    &input.name,
                    &uid,
                    side_id.clone(),
                    Some(user_id.clone()),
                    None,
                    &now,
                );
                player_ids.insert(user_id.clone(), player.player_id.clone());
                player_records.push(player);
                invitation_records.push(inv);
            }
            for external in &invite.invited_externals {
                let (player, inv) = build_invited_player(
                    &match_id,
                    &input.name,
                    &uid,
                    side_id.clone(),
                    None,
                    Some(external.name.clone()),
                    &now,
                );
                player_ids.insert(external.client_id.clone(), player.player_id.clone());
                player_records.push(player);
                invitation_records.push(inv);
            }
        }

        // A side with no players has no player to fall back on for its display
        // name (see `Api::resolve_side_names`'s priority chain: custom name ->
        // sole player's name -> team's name -> ...), so it needs a team or an
        // explicit name to be identifiable at all — e.g. recording a result
        // against an opposition you don't know the roster of.
        for side in &input.sides {
            let side_id = &side_ids[&side.client_id];
            let has_players = player_records
                .iter()
                .any(|p| p.side_id.as_deref() == Some(side_id.as_str()));
            if !has_players && side.team_id.is_none() && side.name.is_none() {
                return Ok(CreateMatchResponse::ValidationError(PlainText(format!(
                    "side `{}` has no players, so it needs a name or a team",
                    side.client_id
                ))));
            }
        }

        // Resolve a create-time score's client-side ids to real side/player ids
        // (reject an unknown reference). This becomes a PENDING submission, not
        // a confirmed score: the reported result awaits the other side(s)'
        // confirmation via `POST /matches/:id/score-submissions/:sid/respond`
        // (mirrors the post-creation flow). The submitter's side is implicitly
        // confirmed.
        let (resolved_score, resolved_winner) = match &input.score {
            Some(score) => match resolve_score_ids(score, &side_ids, &player_ids) {
                Some(resolved) => (
                    Some(score_to_record(&resolved)),
                    input
                        .winner_side_id
                        .as_ref()
                        .and_then(|c| side_ids.get(c).cloned()),
                ),
                None => {
                    return Ok(CreateMatchResponse::ValidationError(PlainText(
                        "score references an unknown side or player".into(),
                    )));
                }
            },
            None => (None, None),
        };

        // A supplied score means the match was already played (Completed);
        // otherwise it is Scheduled. Confirmation is independent of status — a
        // Completed match may still carry an unconfirmed (pending) score.
        let status = if resolved_score.is_some() {
            "completed"
        } else {
            "scheduled"
        };

        // A create-time score becomes a PENDING submission attributed to the
        // creator, with the creator's side pre-confirmed (mirrors the respond
        // endpoint, which requires every side to confirm and treats the
        // submitter's side as implicitly confirmed). It only promotes to a
        // confirmed score once the other side(s) confirm. Submitting a score
        // therefore requires the creator to be a participant (someone must own
        // the submission); reject a score without `creator_side_client_id`.
        let (pending_score, pending_submission) = match resolved_score {
            Some(score_rec) => {
                let (Some(player_id), Some(side_id)) =
                    (creator_player_id.clone(), creator_side_id.clone())
                else {
                    return Ok(CreateMatchResponse::ValidationError(PlainText(
                        "a score can only be submitted by a participant; set \
                         creator_side_client_id to record the result"
                            .into(),
                    )));
                };
                let submission_id = new_id();
                let confirmation = dao::records::ScoreConfirmationRecord {
                    side_id: side_id.clone(),
                    confirmed_by_player_id: player_id.clone(),
                    confirmed_at: now.clone(),
                };
                let pending = dao::records::PendingScoreRecord {
                    submission_id: submission_id.clone(),
                    score: score_rec.clone(),
                    winner_side_id: resolved_winner.clone(),
                    confirmations: vec![confirmation],
                };
                let submission = dao::records::ScoreSubmissionRecord {
                    submission_id,
                    score: score_rec,
                    winner_side_id: resolved_winner.clone(),
                    status: String::from("pending"),
                    submitted_by_player_id: player_id.clone(),
                    submitted_at: now.clone(),
                    // Seed the submitter's side as confirmed, so once the other
                    // side responds "confirm" the submission is fully confirmed.
                    responses: vec![dao::records::ScoreResponseRecord {
                        side_id,
                        responded_by_player_id: player_id,
                        response: String::from("confirm"),
                        responded_at: now.clone(),
                    }],
                };
                (Some(pending), Some(submission))
            }
            None => (None, None),
        };

        let match_record = dao::records::MatchRecord {
            id: match_id.clone(),
            created_by_user_id: uid.clone(),
            name: input.name,
            description: input.description,
            match_type: match_type_tag(&input.match_type).to_string(),
            status: status.to_string(),
            starts_at: input
                .starts_at
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            location: input.location.map(|l| dao::records::LocationRecord {
                latitude: l.latitude,
                longitude: l.longitude,
            }),
            header_photos,
            sides,
            confirmed_score: None,
            pending_score,
            like_count: 0,
            comment_count: 0,
            live_seq: 0,
            live_tip_seq: None,
            format: input.format.as_ref().map(match_format_to_record),
            created_at: now.clone(),
        };

        match dao.create_match(&match_record, &player_records).await {
            Ok(()) => {}
            Err(dao::DaoError::Conflict(msg)) => {
                return Ok(CreateMatchResponse::ValidationError(PlainText(msg)));
            }
            Err(e) => return Err(dao_internal(e)),
        }

        // Persist the standalone invitation entities (best-effort; the match is
        // already created). These drive the invitee inbox / token acceptance.
        for inv in &invitation_records {
            dao.create_invitation(inv).await.map_err(dao_internal)?;
        }

        // Persist the create-time score submission (the pending score's history
        // record), so the other side can respond to it by id.
        if let Some(submission) = &pending_submission {
            dao.put_score_submission(&match_id, submission)
                .await
                .map_err(dao_internal)?;
        }

        // Stable order for the response (`hydrate_match` below re-resolves
        // each side's real name/roster_preview live from `player_records`
        // anyway — this only needs the right side_id/team_id/name per side).
        let mut sides_for_response: Vec<dao::records::MatchSideRecord> =
            match_record.sides.values().cloned().collect();
        sides_for_response.sort_by(|a, b| a.side_id.cmp(&b.side_id));

        let mut m = match_from_records(&match_record, &sides_for_response, &player_records, false);
        sign_match_headers(assets, &mut m);
        let m = self.hydrate_match(dao, m, &uid).await?;
        Ok(CreateMatchResponse::Match(Json(m)))
    }

    #[oai(path = "/matches/:match_id", method = "get")]
    async fn get_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<GetMatchResponse> {
        info!("Getting match {match_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(GetMatchResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        let i_liked = dao
            .has_liked_match(&match_id, &uid)
            .await
            .map_err(dao_internal)?;
        let mut m = match_from_records(&agg.match_, &agg.sides, &agg.players, i_liked);
        sign_match_headers(assets, &mut m);
        let m = self.hydrate_match(dao, m, &uid).await?;
        Ok(GetMatchResponse::Match(Json(m)))
    }

    #[oai(path = "/matches/:match_id", method = "patch")]
    async fn update_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(assets): Data<&Assets>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<UpdateMatchInput>,
    ) -> Result<UpdateMatchResponse> {
        info!("Updating match {match_id}");
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Load the current state (404 if missing).
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(UpdateMatchResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };

        // The creator (organizer) or a participant may edit the match.
        if !caller_can_manage_match(&agg, &uid) {
            return Ok(UpdateMatchResponse::Forbidden(PlainText(
                "only a participant can edit this match".into(),
            )));
        }

        if let Some(fmt) = &input.format {
            // The format is locked once the match has left `scheduled`: it
            // can no longer be changed once scoring has started (live events
            // or a submitted score both move it to `in_progress`/`completed`)
            // or once the match is complete.
            if agg.match_.status != "scheduled" {
                return Ok(UpdateMatchResponse::ValidationError(PlainText(
                    "match format cannot be changed once scoring has started".into(),
                )));
            }

            // A supplied format must be for this match's own sport.
            let tag = match_format_sport_tag(fmt);
            if tag != agg.match_.match_type {
                return Ok(UpdateMatchResponse::ValidationError(PlainText(format!(
                    "format is for `{tag}` but match is `{}`",
                    agg.match_.match_type
                ))));
            }
        }

        // Resolve any replacement header images to asset id + stored URL
        // (must be uploaded, owned by the caller, `match_header` purpose).
        // `None` = leave unchanged. The caller sends the *complete* desired
        // set each time (there's no append/remove-by-id on the server), so a
        // client that wants to keep existing photos re-sends their asset ids
        // (from `Match.header_photos[].asset_id`) alongside any new ones.
        let header_photos: Option<Vec<dao::records::HeaderPhotoRecord>> = match &input
            .header_photo_asset_ids
        {
            Some(ids) => match resolve_asset_urls(dao, &uid, "match_header", ids).await? {
                Ok(resolved) => Some(
                    resolved
                        .into_iter()
                        .map(|(asset_id, url)| dao::records::HeaderPhotoRecord { asset_id, url })
                        .collect(),
                ),
                Err(msg) => return Ok(UpdateMatchResponse::ValidationError(PlainText(msg))),
            },
            None => None,
        };

        // A cancelled match can't be scored.
        let resulting_status = input
            .status
            .as_ref()
            .map(match_status_str)
            .unwrap_or(agg.match_.status.as_str());
        if resulting_status == "cancelled" && input.score.is_some() {
            return Ok(UpdateMatchResponse::ValidationError(PlainText(
                "a cancelled match cannot be scored".into(),
            )));
        }

        // Set of valid side ids on this match, to validate score references.
        let valid_sides: std::collections::HashSet<&str> =
            agg.sides.iter().map(|s| s.side_id.as_str()).collect();

        // Renaming a side: every referenced side must exist, and — mirroring
        // `create_match`'s validation — a side already linked to a team can
        // only get a custom name when another side shares that same team
        // (otherwise the team is the source of truth for the name).
        if let Some(renames) = &input.side_names {
            for rename in renames {
                let Some(side) = agg.sides.iter().find(|s| s.side_id == rename.side_id) else {
                    return Ok(UpdateMatchResponse::ValidationError(PlainText(format!(
                        "side `{}` is not part of this match",
                        rename.side_id
                    ))));
                };
                if let (Some(team_id), Some(_)) = (&side.team_id, &rename.name) {
                    let team_shared = agg.sides.iter().any(|other| {
                        other.side_id != side.side_id && other.team_id.as_deref() == Some(team_id)
                    });
                    if !team_shared {
                        return Ok(UpdateMatchResponse::ValidationError(PlainText(format!(
                            "side `{}` can't have both a name and a team unless another side \
                             shares that team",
                            rename.side_id
                        ))));
                    }
                }
            }
        }

        // A side with no players (after this request's roster edits, if any)
        // needs a team or an explicit name to remain identifiable — same rule
        // `create_match` enforces up front. Only worth projecting when this
        // request can actually change a side's player count or name; an
        // unrelated edit (e.g. renaming the match) leaves existing sides alone.
        if input.added_players.is_some()
            || input.removed_player_ids.is_some()
            || input.side_assignments.is_some()
            || input.side_names.is_some()
        {
            // Project each existing player's resulting side_id: drop removed
            // ids, then apply this request's reassignments (unlisted players
            // keep their current side). `added_players` contribute their own
            // side_id straight away — they don't exist in `agg.players` yet.
            let removed: std::collections::HashSet<&str> = input
                .removed_player_ids
                .iter()
                .flatten()
                .map(String::as_str)
                .collect();
            let reassigned: std::collections::HashMap<&str, &Option<String>> = input
                .side_assignments
                .iter()
                .flatten()
                .map(|a| (a.player_id.as_str(), &a.side_id))
                .collect();

            let mut final_side_ids: Vec<Option<String>> = agg
                .players
                .iter()
                .filter(|p| !removed.contains(p.player_id.as_str()))
                .map(|p| match reassigned.get(p.player_id.as_str()) {
                    Some(side_id) => (*side_id).clone(),
                    None => p.side_id.clone(),
                })
                .collect();
            final_side_ids.extend(
                input
                    .added_players
                    .iter()
                    .flatten()
                    .map(|p| p.side_id.clone()),
            );

            for side in &agg.sides {
                let has_players = final_side_ids
                    .iter()
                    .any(|sid| sid.as_deref() == Some(side.side_id.as_str()));
                if has_players {
                    continue;
                }
                // A rename in this request wins over the side's existing
                // custom name (mirrors how the rename itself is applied).
                let effective_name = input
                    .side_names
                    .iter()
                    .flatten()
                    .find(|r| r.side_id == side.side_id)
                    .map(|r| r.name.clone())
                    .unwrap_or_else(|| side.name.clone());
                if side.team_id.is_none() && effective_name.is_none() {
                    return Ok(UpdateMatchResponse::ValidationError(PlainText(format!(
                        "side `{}` would have no players, so it needs a name or a team",
                        side.side_id
                    ))));
                }
            }
        }

        // Completing a live-scored football/cricket/netball match still
        // requires an explicit `score` from the client (see
        // `LiveScoringPage`/`CricketLiveScoringPage`'s `finishMatch`, which
        // now builds one locally from the same persisted live detail before
        // sending it) — the server never fills one in silently. What it does
        // instead is cross-check: derive its own score from the match's
        // persisted live detail and, unless `override_live_score` says
        // otherwise, reject a client score that disagrees with it, so
        // confirmed results stay in sync with live scoring by default rather
        // than by convention.
        //
        // This check isn't limited to the PATCH that completes the match: it
        // fires for any submitted `score` as long as the match has live
        // detail recorded, whatever `status` this request carries (or
        // doesn't). A match still `in_progress` under live scoring can be
        // scored over by a plain "add/edit result" submission just as easily
        // as by one that explicitly completes it — that score differing from
        // the live detail is the same mistake either way (a separate score
        // quietly displacing the live-scored one instead of finishing it),
        // so it gets the same guard rather than only catching it at
        // completion time.
        let mut derived_winner_side_id: Option<String> = None;
        if input.score.is_some()
            && matches!(
                agg.match_.match_type.as_str(),
                "football" | "cricket" | "netball"
            )
            && let Some(record) = dao
                .get_match_score(&match_id, &agg.match_.match_type)
                .await
                .map_err(dao_internal)?
        {
            let derived_score = match_score_from_record(&record);
            let side_ids: Vec<String> = agg.sides.iter().map(|s| s.side_id.clone()).collect();
            derived_winner_side_id = winner_from_score(&derived_score, &side_ids);

            if let Some(client_score) = &input.score
                && score_to_record(client_score) != score_to_record(&derived_score)
                && !input.override_live_score.unwrap_or(false)
            {
                return Ok(UpdateMatchResponse::Conflict(PlainText(
                    "the submitted score doesn't match the match's live result — refresh \
                     and resubmit, or set override_live_score to submit it anyway"
                        .into(),
                )));
            }
        }
        let effective_score = input.score.as_ref();
        let effective_winner_side_id = input.winner_side_id.clone().or(derived_winner_side_id);

        // A match can't land on `completed` with no score at all — supplied,
        // or already recorded from an earlier submission (this PATCH might
        // just be re-asserting `completed`, or editing something unrelated,
        // on a match that already has one).
        if resulting_status == "completed"
            && effective_score.is_none()
            && agg.match_.confirmed_score.is_none()
            && agg.match_.pending_score.is_none()
        {
            return Ok(UpdateMatchResponse::ValidationError(PlainText(
                "a completed match needs a score".into(),
            )));
        }

        // A supplied score creates a new submission only when it differs from
        // the current confirmed score. For a not-yet-played match, a new score
        // also completes it.
        let mut status_override: Option<&str> = input.status.as_ref().map(match_status_str);

        // A score submitted here is a PENDING submission, not a confirmed one:
        // it awaits the other side(s)' confirmation via
        // `POST /matches/:id/score-submissions/:sid/respond`, exactly like a
        // create-time score (mirrors `create_match`/`respond_to_score_submission`).
        // This is what makes a resubmission after a dispute require fresh
        // approval instead of silently overriding the rejection.
        let mut pending_score: Option<dao::records::PendingScoreRecord> = None;

        if let Some(score) = effective_score {
            // Validate every scored side exists on the match.
            let score_sides: Vec<&str> = match score {
                Score::Simple(s) => s.entries.keys().map(|k| k.as_str()).collect(),
                Score::Sets(s) => s.entries.keys().map(|k| k.as_str()).collect(),
                Score::Cricket(s) => s
                    .innings
                    .iter()
                    .flat_map(|i| [i.batting_side_id.as_str(), i.bowling_side_id.as_str()])
                    .collect(),
                Score::Football(s) => s.score.keys().map(|k| k.as_str()).collect(),
                Score::Netball(s) => s.score.keys().map(|k| k.as_str()).collect(),
            };
            if score_sides.iter().any(|sid| !valid_sides.contains(sid)) {
                return Ok(UpdateMatchResponse::ValidationError(PlainText(
                    "score references a side that is not part of this match".into(),
                )));
            }

            let new_record = score_to_record(score);
            let differs = agg
                .match_
                .confirmed_score
                .as_ref()
                .map(|cs| cs.score != new_record)
                .unwrap_or(true);

            if differs {
                // The submission is attributed to the caller's own player/side
                // (needed so their side can be pre-confirmed, same as at
                // create time), so the caller must be an assigned participant.
                let caller_player = agg
                    .players
                    .iter()
                    .find(|p| p.user_id.as_deref() == Some(uid.as_str()));
                let (caller_player_id, caller_side_id) = match caller_player.and_then(|p| {
                    p.side_id
                        .as_ref()
                        .map(|sid| (p.player_id.clone(), sid.clone()))
                }) {
                    Some(pair) => pair,
                    None => {
                        return Ok(UpdateMatchResponse::ValidationError(PlainText(
                            "a score can only be submitted by a participant assigned \
                             to a side"
                                .into(),
                        )));
                    }
                };

                let submission_id = new_id();
                let submitted_at = now_iso();
                let confirmation = dao::records::ScoreConfirmationRecord {
                    side_id: caller_side_id.clone(),
                    confirmed_by_player_id: caller_player_id.clone(),
                    confirmed_at: submitted_at.clone(),
                };
                let pending = dao::records::PendingScoreRecord {
                    submission_id: submission_id.clone(),
                    score: new_record.clone(),
                    winner_side_id: effective_winner_side_id.clone(),
                    confirmations: vec![confirmation],
                };
                let submission = dao::records::ScoreSubmissionRecord {
                    submission_id,
                    score: new_record,
                    winner_side_id: effective_winner_side_id.clone(),
                    status: String::from("pending"),
                    submitted_by_player_id: caller_player_id.clone(),
                    submitted_at: submitted_at.clone(),
                    // Seed the submitter's side as confirmed, so once the other
                    // side(s) confirm, the submission becomes fully confirmed.
                    responses: vec![dao::records::ScoreResponseRecord {
                        side_id: caller_side_id,
                        responded_by_player_id: caller_player_id,
                        response: String::from("confirm"),
                        responded_at: submitted_at,
                    }],
                };
                dao.put_score_submission(&match_id, &submission)
                    .await
                    .map_err(dao_internal)?;

                pending_score = Some(pending);

                // A not-yet-played match becomes Completed when first scored.
                if status_override.is_none() && agg.match_.status != "completed" {
                    status_override = Some("completed");
                }
            }
        }

        // Persist a manually-supplied live-scoring record (a match entered
        // directly, without live scoring). For a live-scored match, there's
        // nothing to do here even at completion: `GET /matches/:id/score`
        // reads the same record live scoring has been keeping incrementally
        // up to date all along (see `append_live_events`), so completing the
        // match is just the status flip below.
        if let Some(ds) = &input.detailed_score {
            let record = match_score_to_record(ds, None);
            dao.put_match_score(&match_id, &record)
                .await
                .map_err(dao_internal)?;
        }

        // Side renames, validated above — folded into the same `UpdateItem`
        // call as the rest of the metadata below so the two are atomic
        // together (both land, or neither does).
        let side_name_updates: Vec<(String, Option<String>)> = input
            .side_names
            .iter()
            .flatten()
            .map(|r| (r.side_id.clone(), r.name.clone()))
            .collect();

        // Apply metadata + resolved score + side renames in one update.
        dao.update_match_meta(
            &match_id,
            input.name.as_deref(),
            input.description.as_deref(),
            status_override,
            input
                .starts_at
                .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                .as_deref(),
            None,
            pending_score.map(Some),
            header_photos,
            input.format.as_ref().map(match_format_to_record),
            &side_name_updates,
        )
        .await
        .map_err(|e| match e {
            dao::DaoError::NotFound(_) => {
                Error::from_string("match not found", StatusCode::NOT_FOUND)
            }
            other => dao_internal(other),
        })?;

        // Roster: add ad-hoc players (no invitation) then apply side reassigns.
        if let Some(added) = &input.added_players {
            for p in added {
                let player = dao::records::MatchPlayerRecord {
                    player_id: new_id(),
                    user_id: p.user_id.clone(),
                    display_name: p.display_name.clone(),
                    side_id: p.side_id.clone(),
                    is_member_of_team: None,
                    invitation: None,
                };
                dao.put_match_player(&match_id, &player)
                    .await
                    .map_err(dao_internal)?;
            }
        }
        if let Some(removed_ids) = &input.removed_player_ids {
            dao.remove_match_players(&match_id, removed_ids)
                .await
                .map_err(dao_internal)?;
        }
        if let Some(assignments) = &input.side_assignments {
            // Reassign an existing player's side. Fetch current roster (after
            // any adds/removes above) to preserve the player's other fields.
            let current = dao.get_match(&match_id).await.map_err(dao_internal)?;
            if let Some(agg) = current {
                for a in assignments {
                    if let Some(existing) = agg.players.iter().find(|p| p.player_id == a.player_id)
                    {
                        let mut updated = existing.clone();
                        updated.side_id = a.side_id.clone();
                        dao.put_match_player(&match_id, &updated)
                            .await
                            .map_err(dao_internal)?;
                    }
                }
            }
        }
        // A roster change can move players between sides (or add/remove them) —
        // refresh each side's cached roster preview once from the now-current
        // roster, rather than per player_id above.
        if input.added_players.is_some()
            || input.side_assignments.is_some()
            || input.removed_player_ids.is_some()
        {
            dao.refresh_side_roster_previews(&match_id)
                .await
                .map_err(dao_internal)?;
        }

        // Return the updated aggregate.
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(UpdateMatchResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        let i_liked = dao
            .has_liked_match(&match_id, &uid)
            .await
            .map_err(dao_internal)?;
        let mut m = match_from_records(&agg.match_, &agg.sides, &agg.players, i_liked);
        sign_match_headers(assets, &mut m);
        let m = self.hydrate_match(dao, m, &uid).await?;
        Ok(UpdateMatchResponse::Match(Json(m)))
    }

    /// Both sports' persisted records are kept incrementally correct by
    /// every live-scoring append (see `apply_live_events_incrementally`), so
    /// this trusts the persisted record directly, with a full refold only as
    /// a recovery path for a missing record. Manual entry (no live log at
    /// all) always reads the persisted record too — there's nothing else
    /// for it to be caught up with. Not status-gated: this serves the same
    /// `Score` whether the match is still being scored or long finished.
    #[oai(path = "/matches/:match_id/score", method = "get")]
    async fn get_match_score(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<GetMatchScoreResponse> {
        info!("Getting score for match {match_id}");

        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(GetMatchScoreResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        let sport = agg.match_.match_type.as_str();

        let record = dao
            .get_match_score(&match_id, sport)
            .await
            .map_err(dao_internal)?;
        let mut score = match record {
            Some(record) => match_score_from_record(&record),
            None => {
                // No persisted record — recover by folding the event log, if
                // there is one (e.g. a live-scored match whose record was
                // somehow missing). Persist the result so subsequent reads,
                // and the next incremental append, have a fresh checkpoint
                // to build on.
                let records = dao
                    .list_live_events(&match_id)
                    .await
                    .map_err(dao_internal)?;
                if records.is_empty() {
                    return Ok(GetMatchScoreResponse::NotFound(PlainText(
                        "match has no score".into(),
                    )));
                }
                let Some(score) = derive_live_score(sport, &records, agg.match_.format.as_ref())
                else {
                    return Ok(GetMatchScoreResponse::NotFound(PlainText(
                        "match has no score".into(),
                    )));
                };
                // `agg.match_.live_seq`, not `max(records.seq)` — see
                // `derive_live_snapshot`'s doc comment for why the latter can
                // undercount once an undo has removed the log's former tip.
                self.persist_score(dao, &match_id, &score, Some(agg.match_.live_seq))
                    .await;
                score
            }
        };

        self.hydrate_score_players(dao, &match_id, &mut score)
            .await?;
        Ok(GetMatchScoreResponse::Score(Json(score)))
    }

    /// Resolve live name/avatar for every player id referenced anywhere in a
    /// cricket or football score (`CricketScore.players`/
    /// `FootballScore.players`) — striker/non-striker/bowler, batting/
    /// bowling cards, goal scorers/assists, cards, substitutions, and so on
    /// (see [`score_player_ids`]). One targeted `BatchGetItem` for exactly
    /// those ids (`Dao::batch_get_match_players`) — not the full roster
    /// `Query` `Match.players` needs — plus `batch_get_users` for whichever
    /// are linked accounts, same "live name/avatar over stored" rule
    /// `feed_roster_preview` uses. No-op for `Score::Simple`/`Score::Sets`
    /// (neither references players by id) or once a score references no
    /// players at all.
    async fn hydrate_score_players(
        &self,
        dao: &dao::Dao,
        match_id: &str,
        score: &mut Score,
    ) -> Result<()> {
        let player_ids = score_player_ids(score);
        if player_ids.is_empty() {
            return Ok(());
        }

        let match_players = dao
            .batch_get_match_players(match_id, &player_ids)
            .await
            .map_err(dao_internal)?;
        let user_ids: Vec<String> = match_players
            .values()
            .filter_map(|p| p.user_id.clone())
            .collect();
        let users = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;

        let resolved: HashMap<String, RosterPreviewPlayer> = match_players
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    roster_preview_player(p.user_id.as_deref(), p.display_name.as_deref(), &users),
                )
            })
            .collect();
        set_score_players(score, resolved);
        Ok(())
    }

    /// Resolve `players` on every match's `confirmed_score`/`pending_score`
    /// across a whole feed/search page in one shot — the completed/disputed-
    /// result counterpart to [`Self::hydrate_score_players`] (which handles
    /// a single match's *live* score, `GET /matches/:id/score`). Doing this
    /// per match here would reintroduce the exact per-page-size cost this
    /// whole endpoint was rewritten to avoid (see `batch_get_match_summaries`'s
    /// doc comment), so instead every match's referenced player ids
    /// (`score_player_ids`, paired with that match's own id) go into one
    /// cross-match `BatchGetItem` (`Dao::batch_get_players_across_matches`)
    /// regardless of how many distinct matches are on the page.
    async fn hydrate_confirmed_pending_score_players(
        &self,
        dao: &dao::Dao,
        matches: &mut [(&str, &mut Option<ConfirmedScore>, &mut Option<PendingScore>)],
    ) -> Result<()> {
        let mut keys: Vec<(String, String)> = Vec::new();
        for (match_id, confirmed, pending) in matches.iter() {
            if let Some(cs) = confirmed {
                keys.extend(
                    score_player_ids(&cs.score)
                        .into_iter()
                        .map(|pid| (match_id.to_string(), pid)),
                );
            }
            if let Some(ps) = pending {
                keys.extend(
                    score_player_ids(&ps.score)
                        .into_iter()
                        .map(|pid| (match_id.to_string(), pid)),
                );
            }
        }
        if keys.is_empty() {
            return Ok(());
        }

        let match_players = dao
            .batch_get_players_across_matches(&keys)
            .await
            .map_err(dao_internal)?;
        let user_ids: Vec<String> = match_players
            .values()
            .filter_map(|p| p.user_id.clone())
            .collect();
        let users = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;

        for (match_id, confirmed, pending) in matches.iter_mut() {
            if let Some(cs) = confirmed {
                resolve_score_players_for_match(&mut cs.score, match_id, &match_players, &users);
            }
            if let Some(ps) = pending {
                resolve_score_players_for_match(&mut ps.score, match_id, &match_players, &users);
            }
        }
        Ok(())
    }

    /// Append a batch of live-scoring events (1 to
    /// [`dao::live_score_ops::MAX_LIVE_EVENTS_PER_BATCH`]) to a match's live
    /// event log, atomically. `expected_last_seq` gates ordering and
    /// idempotency: a device with an offline backlog resubmits with whatever
    /// tip it last saw, and a mismatch (another device moved the log on, or
    /// this is a stale retry) comes back as `409 Conflict` rather than
    /// silently reordering or duplicating events.
    #[oai(path = "/matches/:match_id/live/events", method = "post")]
    async fn append_live_events(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<AppendLiveEventsInput>,
    ) -> Result<AppendLiveEventsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        let input = input.0;
        info!(
            "Appending {} live event(s) to match {match_id} from seq {}",
            input.events.len(),
            input.expected_last_seq
        );

        if input.events.is_empty() {
            return Ok(AppendLiveEventsResponse::ValidationError(PlainText(
                "events must not be empty".into(),
            )));
        }
        if input.events.len() > dao::live_score_ops::MAX_LIVE_EVENTS_PER_BATCH {
            return Ok(AppendLiveEventsResponse::ValidationError(PlainText(
                format!(
                    "batch of {} exceeds the {}-event limit per request; split larger \
                     offline backlogs into multiple calls",
                    input.events.len(),
                    dao::live_score_ops::MAX_LIVE_EVENTS_PER_BATCH
                ),
            )));
        }

        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(AppendLiveEventsResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };

        // Only a participant may record live events — same gate as editing
        // the match (`update_match`). Confirmation now covers the detail
        // these events fold into, so a non-participant writing them isn't
        // just noise, it's data someone else's confirmation would vouch for.
        if !caller_can_manage_match(&agg, &uid) {
            return Ok(AppendLiveEventsResponse::Forbidden(PlainText(
                "only a participant can record live events for this match".into(),
            )));
        }

        let sport = agg.match_.match_type.clone();

        // Every event in the batch must match the match's own sport — a
        // client bug (or stale roster) shouldn't be able to write a football
        // event onto a cricket match's log.
        for (i, e) in input.events.iter().enumerate() {
            let tag = mapping::live_event_sport_tag(&e.event);
            if tag != sport {
                return Ok(AppendLiveEventsResponse::ValidationError(PlainText(
                    format!("event {i} has sport `{tag}` but match is `{sport}`"),
                )));
            }
        }

        let recorded_at = now_iso();
        let new_events: Vec<dao::live_score_ops::NewLiveEvent> = input
            .events
            .iter()
            .map(|e| new_live_event_to_dao(e, &uid, &recorded_at))
            .collect();

        let new_last_seq = match dao
            .append_live_events(&match_id, input.expected_last_seq, &new_events)
            .await
        {
            Ok(new_last_seq) => new_last_seq,
            Err(dao::DaoError::Conflict(msg)) => {
                return Ok(AppendLiveEventsResponse::Conflict(PlainText(msg)));
            }
            Err(e) => return Err(dao_internal(e)),
        };

        // Recording a live event is what "starting" a match means, whatever
        // the sport (kickoff for football, the first ball for cricket) — flip
        // a still-scheduled match to `in_progress` here rather than relying
        // on each sport's client to remember a separate status PATCH.
        if agg.match_.status == "scheduled" {
            dao.update_match_meta(
                &match_id,
                None,
                None,
                Some("in_progress"),
                None,
                None,
                None,
                None,
                None,
                &[],
            )
            .await
            .map_err(dao_internal)?;
        }

        // Applies just the new events to the last-known checkpoint, whatever
        // the sport — falls back to a full refold itself if the checkpoint
        // is missing or behind (e.g. this is the match's first event, or a
        // correction landed between the last append and this one).
        let snapshot = match self
            .apply_live_events_incrementally(
                dao,
                &match_id,
                &sport,
                agg.match_.format.as_ref(),
                &input.events,
                input.expected_last_seq,
                new_last_seq,
            )
            .await?
        {
            Some(snapshot) => Some(snapshot),
            None => {
                self.derive_live_snapshot(
                    dao,
                    &match_id,
                    &sport,
                    agg.match_.format.as_ref(),
                    new_last_seq,
                )
                .await?
            }
        };

        match snapshot {
            Some(snapshot) => Ok(AppendLiveEventsResponse::Ok(Json(snapshot))),
            None => Ok(AppendLiveEventsResponse::ValidationError(PlainText(
                format!("sport `{sport}` does not support live scoring"),
            ))),
        }
    }

    /// The incremental fast path for an append, either sport: applies just
    /// the newly-appended events to the persisted checkpoint, rather than
    /// refolding the whole log. Returns `None` (meaning "fall back to a full
    /// refold") when there's no usable checkpoint to build on — missing,
    /// unparseable, or not caught up to exactly the start of this batch (a
    /// correction landing between the last append and this one, or the very
    /// first event this match has ever recorded).
    async fn apply_live_events_incrementally(
        &self,
        dao: &dao::Dao,
        match_id: &str,
        sport: &str,
        format: Option<&dao::records::MatchFormatRecord>,
        new_events: &[NewLiveEventInput],
        expected_last_seq: u32,
        new_last_seq: u32,
    ) -> Result<Option<LiveScoreSnapshot>> {
        let Some(record) = dao
            .get_match_score(match_id, sport)
            .await
            .map_err(dao_internal)?
        else {
            return Ok(None);
        };
        if record.last_seq != Some(expected_last_seq) {
            return Ok(None);
        }
        let mut score = match_score_from_record(&record);

        match &mut score {
            Score::Cricket(s) => {
                let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) =
                    mapping::cricket_format_args(format);
                for e in new_events {
                    let LiveEventInput::Cricket(event) = &e.event else {
                        // Sport mismatch is already rejected earlier in
                        // `append_live_events`; unreachable here in practice.
                        return Ok(None);
                    };
                    s.apply_event(
                        e.occurred_at,
                        event,
                        balls_per_over,
                        wide_is_extra_ball,
                        no_ball_is_extra_ball,
                    );
                }
            }
            Score::Football(s) => {
                for e in new_events {
                    let LiveEventInput::Football(event) = &e.event else {
                        return Ok(None);
                    };
                    s.apply_event(e.occurred_at, event);
                }
            }
            Score::Netball(s) => {
                for e in new_events {
                    let LiveEventInput::Netball(event) = &e.event else {
                        return Ok(None);
                    };
                    s.apply_event(e.occurred_at, event);
                }
            }
            // Live scoring only ever creates a `Cricket`/`Football`/`Netball`
            // record for this match_id/sport pair — unreachable in practice.
            Score::Simple(_) | Score::Sets(_) => return Ok(None),
        }

        self.persist_score(dao, match_id, &score, Some(new_last_seq))
            .await;

        Ok(Some(LiveScoreSnapshot {
            last_seq: new_last_seq,
            score,
        }))
    }

    /// The match's current live-scoring counter — what a client with no
    /// cached mutation response should seed `expected_last_seq` from (a
    /// fresh page load, or a device that's never scored this match before).
    /// Reads `MatchRecord.live_seq` directly off the same `get_match` call
    /// every other match read already makes — no extra table read beyond
    /// that. Deliberately *not* derived from the physical event log's max
    /// seq the way `GET /matches/:match_id/live/events` would suggest:
    /// once any event has ever been undone, `live_seq` permanently outruns
    /// that log's own max (see `Dao::delete_live_event`'s doc comment), so
    /// a client that seeded its append token from the log instead would
    /// send a stale `expected_last_seq` on every append from then on and
    /// get rejected with `Conflict` forever.
    #[oai(path = "/matches/:match_id/live/seq", method = "get")]
    async fn get_live_seq(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<GetLiveSeqResponse> {
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(GetLiveSeqResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        Ok(GetLiveSeqResponse::Ok(Json(LiveSeq {
            last_seq: agg.match_.live_seq,
        })))
    }

    /// The raw live event log, oldest first — for reconstructing the full
    /// scorecard client-side (see `inningsDeliveriesFromEvents`, used by a
    /// completed match's run-progression graph) or for the scorer's own
    /// delete/amend picker. Paginated: a full multi-innings, unlimited-overs
    /// match can run to thousands of events, which is fine for the
    /// per-item-safe log itself but not something to ship as one response.
    #[oai(path = "/matches/:match_id/live/events", method = "get")]
    async fn list_live_events(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListLiveEventsResponse> {
        if dao
            .get_match(&match_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(ListLiveEventsResponse::NotFound(PlainText(
                "match not found".into(),
            )));
        }

        let page = dao
            .list_live_events_page(&match_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let items: Vec<LiveEvent> = page.items.iter().map(live_event_from_record).collect();
        Ok(ListLiveEventsResponse::Events(Json(LiveEventPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    /// Delete a single live event outright — but only the current tip
    /// ("undo the last thing I recorded"). An arbitrary-position delete
    /// would need real conflict resolution to reconcile against a device's
    /// own offline queue (see `live_score`'s module docs); undoing only the
    /// tip doesn't, since there's nothing downstream of it that could have
    /// been built on the thing being removed. History is genuinely removed,
    /// not marked; the returned detail reflects the log as if the event had
    /// never been recorded.
    ///
    /// The tip check itself now lives entirely in `Dao::delete_live_event`
    /// (atomic against the live `live_seq` counter, not a value read a
    /// moment earlier here) — a `Conflict` back from it means `seq` wasn't
    /// the tip at commit time, surfaced the same way as any other "the log
    /// moved on" case.
    #[oai(path = "/matches/:match_id/live/events/:seq", method = "delete")]
    async fn delete_live_event(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(seq): Path<u32>,
    ) -> Result<DeleteLiveEventResponse> {
        self.require_uid(dao, &jwt_data).await?;
        info!("Deleting live event {seq} on match {match_id}");

        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(DeleteLiveEventResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };

        let new_tip = match dao.delete_live_event(&match_id, seq).await {
            Ok(new_tip) => new_tip,
            Err(dao::DaoError::NotFound(_)) => {
                return Ok(DeleteLiveEventResponse::NotFound(PlainText(
                    "live event not found".into(),
                )));
            }
            Err(dao::DaoError::Conflict(_)) => {
                return Ok(DeleteLiveEventResponse::ValidationError(PlainText(
                    "only the most recently recorded event can be undone".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        };

        match self
            .derive_live_snapshot(
                dao,
                &match_id,
                &agg.match_.match_type,
                agg.match_.format.as_ref(),
                // The DAO's freshly-bumped counter, not `agg.match_.live_seq`
                // (fetched before the delete — now stale, since the delete
                // itself advances the counter).
                new_tip,
            )
            .await?
        {
            Some(snapshot) => Ok(DeleteLiveEventResponse::Ok(Json(snapshot))),
            None => Ok(DeleteLiveEventResponse::ValidationError(PlainText(
                format!(
                    "sport `{}` does not support live scoring",
                    agg.match_.match_type
                ),
            ))),
        }
    }

    /// Amending an event in place isn't supported: it would need real
    /// conflict resolution to reconcile against a device's own offline
    /// queue, the way appends and tip-only deletes don't. The correction
    /// path for "recorded the wrong facts" is delete-and-reappend, currently
    /// restricted to the tip — see `delete_live_event`.
    #[oai(path = "/matches/:match_id/live/events/:seq", method = "patch")]
    async fn amend_live_event(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(seq): Path<u32>,
        input: Json<LiveEventInput>,
    ) -> Result<AmendLiveEventResponse> {
        self.require_uid(dao, &jwt_data).await?;
        let _ = (match_id, seq, input);
        Ok(AmendLiveEventResponse::ValidationError(PlainText(
            "amending a live event in place isn't supported; delete and re-append instead \
             (only the most recently recorded event can be deleted)"
                .into(),
        )))
    }

    /// Derives the current score by folding the full event log — the slow
    /// path: bootstrapping a match's first live-scoring record, recovering
    /// from a missing persisted record, and rebuilding after an undo
    /// (removing the tip isn't a single incremental step the way appending
    /// is — see `apply_live_events_incrementally` for that fast path).
    /// Persists the result so subsequent reads and the next incremental
    /// append have a fresh checkpoint to build on. Returns `None` if `sport`
    /// doesn't support live scoring.
    ///
    /// `live_seq` is the authoritative tip — the match's `live_seq` counter —
    /// and must come from the caller rather than be recomputed as
    /// `max(records.seq)` here. An append and a delete (see
    /// `Dao::delete_live_event`) both advance the real counter without
    /// necessarily leaving a matching physical event at every number (a
    /// delete's own seq, and the number it bumps past, are both permanently
    /// skipped), so the physical log's max can lag behind it. Persisting
    /// that lower, recomputed value as the checkpoint's `last_seq` — the
    /// same value the client then caches as its next `expected_last_seq` —
    /// would desync it from the real counter, so every subsequent append's
    /// conditional update would fail with a `Conflict` (see
    /// `append_live_events`'s `live_seq = :expected` check).
    async fn derive_live_snapshot(
        &self,
        dao: &dao::Dao,
        match_id: &str,
        sport: &str,
        format: Option<&dao::records::MatchFormatRecord>,
        live_seq: u32,
    ) -> Result<Option<LiveScoreSnapshot>> {
        let records = dao.list_live_events(match_id).await.map_err(dao_internal)?;

        let Some(score) = derive_live_score(sport, &records, format) else {
            return Ok(None);
        };

        self.persist_score(dao, match_id, &score, Some(live_seq))
            .await;

        Ok(Some(LiveScoreSnapshot {
            last_seq: live_seq,
            score,
        }))
    }

    /// Best-effort persisted-record write, shared by the incremental append
    /// path and the full-refold path — never fails the caller's request;
    /// the record is always safely recomputable from the event log if this
    /// write is lost or the record goes stale.
    async fn persist_score(
        &self,
        dao: &dao::Dao,
        match_id: &str,
        score: &Score,
        last_seq: Option<u32>,
    ) {
        let record = match_score_to_record(score, last_seq);
        if let Err(e) = dao.put_match_score(match_id, &record).await {
            error!("Failed to persist live-scoring record for match {match_id}: {e}");
        }
    }

    #[oai(path = "/matches/:match_id/score-submissions", method = "get")]
    async fn list_score_submissions(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<ListScoreSubmissionsResponse> {
        info!("Listing score submissions for match {match_id}");

        // 404 if the match itself is missing.
        if dao
            .get_match(&match_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(ListScoreSubmissionsResponse::NotFound(PlainText(
                "match not found".into(),
            )));
        }

        // Full history (newest first). The endpoint is not paginated, so drain
        // the pages.
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = dao
                .list_score_submissions(&match_id, cursor.as_deref(), MAX_PAGE_LIMIT)
                .await
                .map_err(dao_internal)?;
            items.extend(page.items.iter().map(score_submission_from_record));
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        // Stored oldest-first; present newest-first.
        items.reverse();
        Ok(ListScoreSubmissionsResponse::Submissions(Json(items)))
    }

    #[oai(
        path = "/matches/:match_id/score-submissions/:submission_id/respond",
        method = "post"
    )]
    async fn respond_to_score_submission(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(submission_id): Path<String>,
        input: Json<RespondToScoreInput>,
    ) -> Result<RespondToScoreResponse> {
        info!(
            "Responding to score submission {submission_id} on match {match_id}: {:?}",
            input.response
        );

        let uid = self.require_uid(dao, &jwt_data).await?;
        // The match tells us the sides and which side the caller plays for.
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(RespondToScoreResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        // Caller's player row (by linked user id), and the side they're on.
        let caller_player = agg
            .players
            .iter()
            .find(|p| p.user_id.as_deref() == Some(uid.as_str()));
        let (caller_player_id, caller_side_id) = match caller_player.and_then(|p| {
            p.side_id
                .as_ref()
                .map(|sid| (p.player_id.clone(), sid.clone()))
        }) {
            Some(pair) => pair,
            None => {
                return Ok(RespondToScoreResponse::Forbidden(PlainText(
                    "only an assigned participant may respond to the score".into(),
                )));
            }
        };

        let mut submission = match dao
            .get_score_submission(&match_id, &submission_id)
            .await
            .map_err(dao_internal)?
        {
            Some(s) => s,
            None => {
                return Ok(RespondToScoreResponse::NotFound(PlainText(
                    "score submission not found".into(),
                )));
            }
        };
        // Can only respond to a pending submission.
        if submission.status != "pending" {
            return Ok(RespondToScoreResponse::Forbidden(PlainText(
                "this submission is no longer pending".into(),
            )));
        }

        let now = now_iso();
        match input.0.response {
            ScoreResponseKind::Dispute => {
                submission
                    .responses
                    .push(dao::records::ScoreResponseRecord {
                        side_id: caller_side_id,
                        responded_by_player_id: caller_player_id,
                        response: String::from("dispute"),
                        responded_at: now.clone(),
                    });
                submission.status = String::from("disputed");
                dao.put_score_submission(&match_id, &submission)
                    .await
                    .map_err(dao_internal)?;
                // A disputed submission clears the pending score on the match.
                dao.update_match_meta(
                    &match_id,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(None),
                    None,
                    None,
                    &[],
                )
                .await
                .map_err(dao_internal)?;
            }
            ScoreResponseKind::Confirm => {
                // Record this side's confirmation (idempotent per side).
                if !submission
                    .responses
                    .iter()
                    .any(|r| r.side_id == caller_side_id)
                {
                    submission
                        .responses
                        .push(dao::records::ScoreResponseRecord {
                            side_id: caller_side_id.clone(),
                            responded_by_player_id: caller_player_id,
                            response: String::from("confirm"),
                            responded_at: now.clone(),
                        });
                }
                // Every side except the submitter's must confirm. The submitter's
                // side counts as implicitly confirmed (they proposed it).
                let confirmed_sides: std::collections::HashSet<&str> = submission
                    .responses
                    .iter()
                    .filter(|r| r.response == "confirm")
                    .map(|r| r.side_id.as_str())
                    .collect();
                let all_confirmed = agg
                    .sides
                    .iter()
                    .all(|side| confirmed_sides.contains(side.side_id.as_str()));

                if all_confirmed {
                    submission.status = String::from("confirmed");
                }
                dao.put_score_submission(&match_id, &submission)
                    .await
                    .map_err(dao_internal)?;

                if all_confirmed {
                    // Promote to the match's confirmed score and clear pending.
                    let confirmed = dao::records::ConfirmedScoreRecord {
                        score: submission.score.clone(),
                        winner_side_id: submission.winner_side_id.clone(),
                    };
                    dao.update_match_meta(
                        &match_id,
                        None,
                        None,
                        None,
                        None,
                        Some(confirmed),
                        Some(None),
                        None,
                        None,
                        &[],
                    )
                    .await
                    .map_err(dao_internal)?;
                }
            }
        }

        Ok(RespondToScoreResponse::Submission(Json(
            score_submission_from_record(&submission),
        )))
    }

    #[oai(path = "/matches/:match_id/likes", method = "post")]
    async fn like_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<LikeResponse> {
        info!("Liking match {match_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        // Idempotent create of the caller -> match like edge (bumps like_count).
        dao.like_match(&match_id, &uid, &now_iso())
            .await
            .map_err(dao_internal)?;
        Ok(LikeResponse::Ok)
    }

    #[oai(path = "/matches/:match_id/likes", method = "delete")]
    async fn unlike_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<LikeResponse> {
        info!("Unliking match {match_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        // Idempotent removal of the caller -> match like edge.
        dao.unlike_match(&match_id, &uid)
            .await
            .map_err(dao_internal)?;
        Ok(LikeResponse::Ok)
    }

    #[oai(path = "/matches/:match_id/likes", method = "get")]
    async fn list_match_likes(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListLikesResponse> {
        info!("Listing likes for match {match_id}");
        let page = dao
            .list_match_likes(&match_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|l| l.user_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids, None).await?;
        Ok(ListLikesResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/matches/:match_id/comments", method = "get")]
    async fn list_match_comments(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListCommentsResponse> {
        info!("Listing comments for match {match_id}");
        let page = dao
            .list_comments(&match_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let items = self.hydrate_comments(dao, page.items).await?;
        Ok(ListCommentsResponse::Comments(Json(CommentPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/matches/:match_id/comments", method = "post")]
    async fn create_match_comment(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<CreateCommentInput>,
    ) -> Result<CreateCommentResponse> {
        info!("Creating comment on match {match_id}");
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;
        if input.text.trim().is_empty() {
            return Ok(CreateCommentResponse::ValidationError(PlainText(
                "comment text must not be empty".into(),
            )));
        }

        let record = dao::records::CommentRecord {
            comment_id: new_id(),
            match_id: match_id.clone(),
            parent_id: input.parent_id.clone(),
            author_user_id: Some(uid.clone()),
            text: Some(input.text),
            created_at: now_iso(),
            edited_at: None,
            deleted_at: None,
            reply_count: 0,
        };

        // A reply targets a top-level comment: validate the parent exists and is
        // itself top-level (no replying to a reply). A top-level comment lives at
        // `COMMENT#<id>`; a reply at `REPLY#<id>`. If the parent id resolves to a
        // reply, it's a (rejected) second-level reply; if it resolves to neither,
        // the parent doesn't exist.
        if let Some(parent_id) = &input.parent_id {
            if dao
                .get_comment(&match_id, parent_id)
                .await
                .map_err(dao_internal)?
                .is_none()
            {
                // Not a top-level comment. Is it a reply (→ 400) or absent (→ 404)?
                if dao
                    .get_reply(&match_id, parent_id)
                    .await
                    .map_err(dao_internal)?
                    .is_some()
                {
                    return Ok(CreateCommentResponse::ValidationError(PlainText(
                        "cannot reply to a reply".into(),
                    )));
                }
                return Ok(CreateCommentResponse::NotFound(PlainText(
                    "parent comment not found".into(),
                )));
            }
            dao.create_reply(&record).await.map_err(dao_internal)?;
        } else {
            dao.create_comment(&record).await.map_err(dao_internal)?;
        }

        // Author is the caller.
        let author = self.try_user_profile(dao, &uid).await?;
        Ok(CreateCommentResponse::Comment(Json(comment_from_record(
            &record, author,
        ))))
    }

    #[oai(
        path = "/matches/:match_id/comments/:comment_id/replies",
        method = "get"
    )]
    async fn list_comment_replies(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(comment_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListCommentsResponse> {
        info!("Listing replies to comment {comment_id} on match {match_id}");
        let page = dao
            .list_replies(&comment_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let items = self.hydrate_comments(dao, page.items).await?;
        Ok(ListCommentsResponse::Comments(Json(CommentPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/matches/:match_id/comments/:comment_id", method = "patch")]
    async fn update_match_comment(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(comment_id): Path<String>,
        input: Json<UpdateCommentInput>,
    ) -> Result<UpdateCommentResponse> {
        info!("Updating comment {comment_id} on match {match_id}");
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;
        if input.text.trim().is_empty() {
            return Ok(UpdateCommentResponse::ValidationError(PlainText(
                "comment text must not be empty".into(),
            )));
        }

        let existing = match dao
            .get_comment(&match_id, &comment_id)
            .await
            .map_err(dao_internal)?
        {
            Some(c) => c,
            None => {
                return Ok(UpdateCommentResponse::NotFound(PlainText(
                    "comment not found".into(),
                )));
            }
        };
        if existing.author_user_id.as_deref() != Some(uid.as_str()) {
            return Ok(UpdateCommentResponse::Forbidden(PlainText(
                "only the author can edit this comment".into(),
            )));
        }

        let edited_at = now_iso();
        dao.edit_comment(&match_id, &comment_id, &input.text, &edited_at)
            .await
            .map_err(dao_internal)?;

        let mut updated = existing;
        updated.text = Some(input.text);
        updated.edited_at = Some(edited_at);
        let author = self.try_user_profile(dao, &uid).await?;
        Ok(UpdateCommentResponse::Comment(Json(comment_from_record(
            &updated, author,
        ))))
    }

    #[oai(path = "/matches/:match_id/comments/:comment_id", method = "delete")]
    async fn delete_match_comment(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        Path(comment_id): Path<String>,
    ) -> Result<DeleteCommentResponse> {
        info!("Deleting comment {comment_id} on match {match_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        let existing = match dao
            .get_comment(&match_id, &comment_id)
            .await
            .map_err(dao_internal)?
        {
            Some(c) => c,
            None => {
                return Ok(DeleteCommentResponse::NotFound(PlainText(
                    "comment not found".into(),
                )));
            }
        };
        if existing.author_user_id.as_deref() != Some(uid.as_str()) {
            return Ok(DeleteCommentResponse::Forbidden(PlainText(
                "only the author can delete this comment".into(),
            )));
        }

        // Tombstone if it has replies (keep the thread); hard-delete otherwise.
        if existing.reply_count > 0 {
            dao.tombstone_comment(&match_id, &comment_id, &now_iso())
                .await
                .map_err(dao_internal)?;
        } else {
            match dao.delete_comment_hard(&match_id, &comment_id).await {
                Ok(()) => {}
                // Deleted by a concurrent request between the check above and
                // here.
                Err(dao::DaoError::NotFound(_)) => {
                    return Ok(DeleteCommentResponse::NotFound(PlainText(
                        "comment not found".into(),
                    )));
                }
                Err(e) => return Err(dao_internal(e)),
            }
        }
        Ok(DeleteCommentResponse::Ok)
    }

    #[oai(path = "/users/me/teams", method = "get")]
    async fn list_my_teams(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListTeamsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Listing teams for {uid}");
        let page = dao
            .list_user_teams(&uid, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;

        // Each membership row carries its team_id; hydrate the team meta for the
        // display name + follower count. TODO: BatchGet these (N+1 for now).
        let is_following = false; // being a member doesn't imply following
        let mut items = Vec::with_capacity(page.items.len());
        for membership in page.items {
            if let Some(team) = dao
                .get_team_meta(&membership.team_id)
                .await
                .map_err(dao_internal)?
            {
                items.push(team_list_item_from_record(&team, is_following));
            }
        }
        Ok(ListTeamsResponse::Teams(Json(TeamPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/teams/search", method = "get")]
    async fn search_teams(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(search): Data<&agon_core::search::SearchClient>,
        AuthSchema(_jwt_data): AuthSchema,
        #[oai(name = "q")] Query(query): Query<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListTeamsResponse> {
        info!("Searching teams with query: {query}");

        let offset = match search_offset(cursor.as_deref()) {
            Ok(o) => o,
            Err(()) => {
                return Ok(ListTeamsResponse::Teams(Json(TeamPage {
                    items: vec![],
                    next_cursor: None,
                })));
            }
        };
        let q = agon_core::search::SearchQuery {
            q: query,
            offset,
            limit: page_limit(limit),
            ..Default::default()
        };
        let hits = search
            .search(agon_core::search::Index::Teams, &q)
            .await
            .map_err(search_internal)?;

        // Hydrate team meta (name + follower count) for each hit. Being able to
        // find a team via search doesn't imply the caller follows it.
        // TODO: BatchGet these (N+1 for now).
        let mut items = Vec::with_capacity(hits.ids.len());
        for id in &hits.ids {
            if let Some(team) = dao.get_team_meta(id).await.map_err(dao_internal)? {
                items.push(team_list_item_from_record(&team, false));
            }
        }
        Ok(ListTeamsResponse::Teams(Json(TeamPage {
            items,
            next_cursor: search_cursor(hits.next_offset),
        })))
    }

    #[oai(path = "/teams", method = "post")]
    async fn create_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateTeamInput>,
    ) -> Result<CreateTeamResponse> {
        info!("Creating team {}", input.name);
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Resolve the attached asset (must be Uploaded, owned by the caller, and
        // of `team_logo` purpose) to its stored URL — same check `PATCH
        // /users/me` runs for a profile picture.
        let logo_url = match &input.logo_asset_id {
            Some(asset_id) => {
                let ids = std::slice::from_ref(asset_id);
                match resolve_asset_urls(dao, &uid, "team_logo", ids).await? {
                    Ok(resolved) => resolved.into_iter().next().map(|(_, url)| url),
                    Err(msg) => {
                        return Ok(CreateTeamResponse::ValidationError(PlainText(msg)));
                    }
                }
            }
            None => None,
        };

        let now = now_iso();
        let team = dao::records::TeamRecord {
            id: new_id(),
            name: input.name,
            logo_url,
            invite_token: Some(new_id()),
            follower_count: 0,
            created_at: now.clone(),
        };
        // The creator becomes the first member with the Admin role (already an
        // Agon user, so no invitation to accept).
        let creator = dao::records::TeamMemberRecord {
            team_id: team.id.clone(),
            membership_id: new_id(),
            user_id: Some(uid.clone()),
            display_name: None,
            role: String::from("admin"),
            invitation: None,
            created_at: now,
        };
        match dao.create_team(&team, &creator).await {
            Ok(()) => {}
            Err(dao::DaoError::Conflict(msg)) => {
                return Ok(CreateTeamResponse::ValidationError(PlainText(msg)));
            }
            Err(e) => return Err(dao_internal(e)),
        }

        // Bundle initial invites into creation — each invitee gets a pending
        // roster slot + standalone invitation, exactly like a later `POST
        // /teams/:team_id/invitations` call (see `invite_to_team`). The
        // response is team meta only (see `Team`'s doc comment), so — unlike
        // before members moved to their own paginated endpoint — nothing
        // here needs to re-fetch the roster just to build it.
        if !input.invited_user_ids.is_empty() || !input.invited_external_names.is_empty() {
            self.invite_to_team(
                dao,
                &team.id,
                &team.name,
                &uid,
                &input.invited_user_ids,
                &input.invited_external_names,
            )
            .await?;
        }

        Ok(CreateTeamResponse::Team(Json(team_from_records(
            &team, false,
        ))))
    }

    #[oai(path = "/teams/:team_id", method = "get")]
    async fn get_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
    ) -> Result<GetTeamResponse> {
        info!("Getting team {team_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        match dao.get_team_meta(&team_id).await.map_err(dao_internal)? {
            Some(meta) => {
                let is_followed_by_me = dao
                    .is_following_team(&uid, &team_id)
                    .await
                    .map_err(dao_internal)?;
                Ok(GetTeamResponse::Team(Json(team_from_records(
                    &meta,
                    is_followed_by_me,
                ))))
            }
            None => Ok(GetTeamResponse::NotFound(PlainText(
                "team not found".into(),
            ))),
        }
    }

    /// A team's members, paginated (`Dao::list_team_members`, a
    /// `begins_with(SK, "MEMBER#")` range query on the team's own partition).
    /// Split out from `Team` itself (see its doc comment) so a large roster
    /// doesn't grow the team response unboundedly, and so a client can page
    /// through it the same way it already does a user's followers.
    #[oai(path = "/teams/:team_id/members", method = "get")]
    async fn list_team_members(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListTeamMembersResponse> {
        info!("Listing members of team {team_id}");
        if dao
            .get_team_meta(&team_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(ListTeamMembersResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        }
        let page = dao
            .list_team_members(&team_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let items = self
            .hydrate_team_members(
                dao,
                page.items.iter().map(team_member_from_record).collect(),
            )
            .await?;
        Ok(ListTeamMembersResponse::Members(Json(TeamMemberPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/teams/:team_id/members", method = "post")]
    async fn add_team_members(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        input: Json<AddTeamMembersInput>,
    ) -> Result<AddTeamMembersResponse> {
        info!("Adding {} members to team {team_id}", input.user_ids.len());

        // Team must exist. Keep the meta — adding members doesn't change it,
        // so the same read serves the response too (members are no longer
        // embedded, see `Team`'s doc comment, so there's nothing else to
        // re-fetch).
        let Some(meta) = dao.get_team_meta(&team_id).await.map_err(dao_internal)? else {
            return Ok(AddTeamMembersResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        };

        // Add each user as a Member (no invitation — ad-hoc add).
        let now = now_iso();
        for user_id in &input.0.user_ids {
            let member = dao::records::TeamMemberRecord {
                team_id: team_id.clone(),
                membership_id: new_id(),
                user_id: Some(user_id.clone()),
                display_name: None,
                role: String::from("member"),
                invitation: None,
                created_at: now.clone(),
            };
            dao.put_team_member(&team_id, &member)
                .await
                .map_err(dao_internal)?;
        }

        Ok(AddTeamMembersResponse::Team(Json(team_from_records(
            &meta, false,
        ))))
    }

    #[oai(path = "/teams/:team_id", method = "patch")]
    async fn update_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        input: Json<UpdateTeamInput>,
    ) -> Result<UpdateTeamResponse> {
        info!("Updating team {team_id}");
        let input = input.0;
        let uid = self.require_uid(dao, &jwt_data).await?;

        // Some(Some(url)) = set a new logo; Some(None) is never produced here
        // (there's no "clear logo" input yet — a fresh asset is the only way
        // to change it); None = leave the logo unchanged.
        let resolved_logo: Option<Option<String>> = match &input.logo_asset_id {
            Some(asset_id) => {
                let ids = std::slice::from_ref(asset_id);
                match resolve_asset_urls(dao, &uid, "team_logo", ids).await? {
                    Ok(resolved) => Some(resolved.into_iter().next().map(|(_, url)| url)),
                    Err(msg) => {
                        return Ok(UpdateTeamResponse::ValidationError(PlainText(msg)));
                    }
                }
            }
            None => None,
        };

        match dao
            .update_team(
                &team_id,
                input.name.as_deref(),
                resolved_logo.as_ref().map(|o| o.as_deref()),
            )
            .await
        {
            Ok(()) => {}
            Err(dao::DaoError::NotFound(_)) => {
                return Ok(UpdateTeamResponse::NotFound(PlainText(
                    "team not found".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        }
        match dao.get_team_meta(&team_id).await.map_err(dao_internal)? {
            Some(meta) => Ok(UpdateTeamResponse::Team(Json(team_from_records(
                &meta, false,
            )))),
            None => Ok(UpdateTeamResponse::NotFound(PlainText(
                "team not found".into(),
            ))),
        }
    }

    #[oai(path = "/teams/:team_id/members/:member_id", method = "delete")]
    async fn remove_team_member(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        Path(member_id): Path<String>,
    ) -> Result<RemoveTeamMemberResponse> {
        info!("Removing member {member_id} from team {team_id}");
        // Team must exist. Keep the meta — removing a member doesn't change
        // it, so this same read serves the response too.
        let Some(meta) = dao.get_team_meta(&team_id).await.map_err(dao_internal)? else {
            return Ok(RemoveTeamMemberResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        };
        match dao.remove_team_member(&team_id, &member_id).await {
            Ok(()) => {}
            Err(dao::DaoError::NotFound(_)) => {
                return Ok(RemoveTeamMemberResponse::NotFound(PlainText(
                    "member not found".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        }
        Ok(RemoveTeamMemberResponse::Team(Json(team_from_records(
            &meta, false,
        ))))
    }

    #[oai(path = "/matches/:match_id/invitations", method = "post")]
    async fn add_match_invitations(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<AddInvitationsInput>,
    ) -> Result<AddInvitationsResponse> {
        info!("Inviting to match {match_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        let input = input.0;

        // Match must exist.
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(AddInvitationsResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };

        // The creator (organizer) or a participant may invite others.
        if !caller_can_manage_match(&agg, &uid) {
            return Ok(AddInvitationsResponse::Forbidden(PlainText(
                "only a participant can invite people to this match".into(),
            )));
        }

        // If a side was named, it must be one of this match's sides.
        if let Some(side_id) = &input.side_id
            && !agg.sides.iter().any(|s| &s.side_id == side_id)
        {
            return Ok(AddInvitationsResponse::NotFound(PlainText(
                "side is not part of this match".into(),
            )));
        }

        // Each invitee gets both a roster slot (with an embedded invitation) and
        // a standalone invitation, exactly like a create-time invite — so they
        // show up on the match roster immediately, pre-assigned to `side_id` if
        // given. `build_invited_player` mints the ids and tokens.
        let now = now_iso();
        let invitees = input
            .invited_user_ids
            .iter()
            .map(|u| (Some(u.clone()), None))
            .chain(
                input
                    .invited_external_names
                    .iter()
                    .map(|n| (None, Some(n.clone()))),
            );

        let mut created = Vec::new();
        for (user_id, display_name) in invitees {
            let (player, invitation) = build_invited_player(
                &match_id,
                &agg.match_.name,
                &uid,
                input.side_id.clone(),
                user_id,
                display_name,
                &now,
            );
            dao.put_match_player(&match_id, &player)
                .await
                .map_err(dao_internal)?;
            dao.create_invitation(&invitation)
                .await
                .map_err(dao_internal)?;
            created.push(invitation_from_record(&invitation));
        }
        // New roster slots move players onto sides — refresh each side's
        // cached roster preview (see `PATCH /matches/:match_id`'s roster
        // block for the same call).
        if !created.is_empty() {
            dao.refresh_side_roster_previews(&match_id)
                .await
                .map_err(dao_internal)?;
        }

        // Roster/player writes don't trigger the stream (only `#META` does), so
        // touch the match meta to re-run fan-out / search indexing with the new
        // roster. Re-writing `name` to its current value is a real write to the
        // item (an all-`None` update would no-op), so it emits a stream record.
        if !created.is_empty() {
            dao.update_match_meta(
                &match_id,
                Some(&agg.match_.name),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
            )
            .await
            .map_err(dao_internal)?;
        }

        Ok(AddInvitationsResponse::Invitations(Json(created)))
    }

    #[oai(path = "/teams/:team_id/invitations", method = "post")]
    async fn add_team_invitations(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        input: Json<AddInvitationsInput>,
    ) -> Result<AddInvitationsResponse> {
        info!("Inviting to team {team_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        let team = match dao.get_team_meta(&team_id).await.map_err(dao_internal)? {
            Some(t) => t,
            None => {
                return Ok(AddInvitationsResponse::NotFound(PlainText(
                    "team not found".into(),
                )));
            }
        };
        let created = self
            .invite_to_team(
                dao,
                &team_id,
                &team.name,
                &uid,
                &input.invited_user_ids,
                &input.invited_external_names,
            )
            .await?;
        Ok(AddInvitationsResponse::Invitations(Json(created)))
    }

    /// Invite people to a team: each invitee gets both a pending roster slot
    /// (with an embedded invitation, via `build_invited_team_member`) and a
    /// standalone invitation — exactly like a match invite, so they show up
    /// on the team's member list immediately and `Dao::accept_invitation_tx`
    /// has a roster entry to link on acceptance. Shared by
    /// `add_team_invitations` and `create_team`'s bundled initial invites.
    async fn invite_to_team(
        &self,
        dao: &dao::Dao,
        team_id: &str,
        team_name: &str,
        inviter_id: &str,
        invited_user_ids: &[String],
        invited_external_names: &[String],
    ) -> Result<Vec<Invitation>> {
        let now = now_iso();
        let invitees = invited_user_ids
            .iter()
            .map(|u| (Some(u.clone()), None))
            .chain(
                invited_external_names
                    .iter()
                    .map(|n| (None, Some(n.clone()))),
            );

        let mut created = Vec::new();
        for (user_id, display_name) in invitees {
            let (member, invitation) = build_invited_team_member(
                team_id,
                team_name,
                inviter_id,
                user_id,
                display_name,
                &now,
            );
            dao.put_team_member(team_id, &member)
                .await
                .map_err(dao_internal)?;
            dao.create_invitation(&invitation)
                .await
                .map_err(dao_internal)?;
            created.push(invitation_from_record(&invitation));
        }
        Ok(created)
    }

    #[oai(path = "/users/me/invitations", method = "get")]
    async fn list_my_invitations(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        /// Optionally filter to a single status (e.g. only `pending`).
        Query(status): Query<Option<InvitationStatus>>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListInvitationsResponse> {
        info!("Listing current user's invitations");
        let uid = self.require_uid(dao, &jwt_data).await?;
        let status_str = status.as_ref().map(invitation_status_str);
        let page = dao
            .list_user_invitations(&uid, status_str, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let items = page
            .items
            .iter()
            .map(invitation_detail_from_record)
            .collect();
        Ok(ListInvitationsResponse::Invitations(Json(InvitationPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/notifications", method = "get")]
    async fn list_notifications(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListNotificationsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Listing notifications for {uid}");
        let page = dao
            .list_notifications(&uid, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;

        // Hydrate every actor profile (every current kind has one) with a
        // single batched lookup across the page, rather than one `get_user`
        // per notification.
        let actor_ids: Vec<String> = page
            .items
            .iter()
            .map(|rec| notification_actor_id(&rec.kind).to_string())
            .collect();
        let actor_records = dao
            .batch_get_users(&actor_ids)
            .await
            .map_err(dao_internal)?;

        let mut items = Vec::with_capacity(page.items.len());
        for rec in page.items {
            // A notification outlives its actor by design (see
            // `Notification`'s doc comment): the actor's account may since
            // have been deleted, so an id here with no matching profile is an
            // expected case to render, not an error to fail the page over.
            let actor_id = notification_actor_id(&rec.kind);
            let actor = match actor_records.get(actor_id) {
                Some(record) => user_profile_from_record(record, false),
                None => deleted_user_profile(actor_id),
            };
            items.push(notification_from_record(&rec, actor));
        }

        Ok(ListNotificationsResponse::Notifications(Json(
            NotificationPage {
                items,
                next_cursor: page.next_cursor,
            },
        )))
    }

    #[oai(path = "/notifications/unread-count", method = "get")]
    async fn notifications_unread_count(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<UnreadCountResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Getting unread notification count for {uid}");
        let count = dao
            .unread_notification_count(&uid)
            .await
            .map_err(dao_internal)?;
        Ok(UnreadCountResponse::Count(Json(UnreadCount {
            unread_count: count as u32,
        })))
    }

    #[oai(path = "/notifications/read", method = "post")]
    async fn mark_all_notifications_read(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<MarkNotificationReadResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Marking all notifications read for {uid}");
        dao.mark_all_notifications_read(&uid)
            .await
            .map_err(dao_internal)?;
        Ok(MarkNotificationReadResponse::Ok)
    }

    #[oai(path = "/notifications/:notification_id/read", method = "post")]
    async fn mark_notification_read(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(notification_id): Path<String>,
    ) -> Result<MarkNotificationReadResponse> {
        info!("Marking notification {notification_id} read");
        let uid = self.require_uid(dao, &jwt_data).await?;
        dao.mark_notification_read(&uid, &notification_id)
            .await
            .map_err(dao_internal)?;
        Ok(MarkNotificationReadResponse::Ok)
    }

    #[oai(path = "/devices", method = "post")]
    async fn register_device(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<RegisterDeviceInput>,
    ) -> Result<RegisterDeviceResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Registering device for {uid}");
        let now = chrono::Utc::now().to_rfc3339();
        match dao
            .register_device(
                &uid,
                &input.push_token,
                device_platform_to_record(&input.platform),
                &now,
            )
            .await
        {
            Ok(()) => Ok(RegisterDeviceResponse::Ok),
            Err(dao::DaoError::Conflict(msg)) => {
                Ok(RegisterDeviceResponse::ValidationError(PlainText(msg)))
            }
            Err(e) => Err(dao_internal(e)),
        }
    }

    #[oai(path = "/devices/unregister", method = "post")]
    async fn unregister_device(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<UnregisterDeviceInput>,
    ) -> Result<UnregisterDeviceResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Unregistering device for {uid}");
        match dao.delete_device(&uid, &input.push_token).await {
            Ok(()) => Ok(UnregisterDeviceResponse::Ok),
            Err(dao::DaoError::NotFound(_)) => Ok(UnregisterDeviceResponse::NotFound(PlainText(
                "device not registered".into(),
            ))),
            Err(e) => Err(dao_internal(e)),
        }
    }

    #[oai(path = "/invitations/:invitation_id", method = "get")]
    async fn get_invitation(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(invitation_id): Path<String>,
    ) -> Result<GetInvitationResponse> {
        info!("Getting invitation {invitation_id}");
        match dao
            .get_invitation(&invitation_id)
            .await
            .map_err(dao_internal)?
        {
            Some(rec) => Ok(GetInvitationResponse::Invitation(Json(
                invitation_detail_from_record(&rec),
            ))),
            None => Ok(GetInvitationResponse::NotFound(PlainText(
                "invitation not found".into(),
            ))),
        }
    }

    /// Preview an invitation by its bearer token. Public (no auth): the token is
    /// itself the credential, so anyone holding an invite link can see what it is
    /// an invite *to* before signing in. Used by the invite-link landing/accept
    /// flow to render context and to know which match/team to open on acceptance.
    #[oai(path = "/invitations/by-token/:token", method = "get")]
    async fn get_invitation_by_token(
        &self,
        Data(dao): Data<&dao::Dao>,
        Path(token): Path<String>,
    ) -> Result<GetInvitationResponse> {
        info!("Getting invitation by token");
        match dao
            .get_invitation_by_token(&token)
            .await
            .map_err(dao_internal)?
        {
            Some(rec) => Ok(GetInvitationResponse::Invitation(Json(
                invitation_detail_from_record(&rec),
            ))),
            None => Ok(GetInvitationResponse::NotFound(PlainText(
                "invitation not found".into(),
            ))),
        }
    }

    #[oai(path = "/invitations/:invitation_id", method = "delete")]
    async fn revoke_invitation(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(invitation_id): Path<String>,
    ) -> Result<RevokeInvitationResponse> {
        info!("Revoking invitation {invitation_id}");
        let uid = self.require_uid(dao, &jwt_data).await?;
        match dao
            .get_invitation(&invitation_id)
            .await
            .map_err(dao_internal)?
        {
            None => {
                return Ok(RevokeInvitationResponse::NotFound(PlainText(
                    "invitation not found".into(),
                )));
            }
            Some(rec) if rec.invited_by_user_id != uid => {
                return Ok(RevokeInvitationResponse::Forbidden(PlainText(
                    "only the inviter can revoke this invitation".into(),
                )));
            }
            Some(_) => {}
        }
        match dao.delete_invitation(&invitation_id).await {
            Ok(()) => Ok(RevokeInvitationResponse::Ok),
            // Revoked by a concurrent request between the check above and here.
            Err(dao::DaoError::NotFound(_)) => Ok(RevokeInvitationResponse::NotFound(PlainText(
                "invitation not found".into(),
            ))),
            Err(e) => Err(dao_internal(e)),
        }
    }

    #[oai(path = "/invitations/:invitation_id/respond", method = "post")]
    async fn respond_to_invitation(
        &self,
        AuthSchema(jwt_data): AuthSchema,
        Path(invitation_id): Path<String>,
        Data(dao): Data<&dao::Dao>,
        input: Json<RespondToInvitationInput>,
    ) -> Result<RespondToInvitationResponse> {
        info!(
            "Responding to invitation {invitation_id}: {:?}",
            input.response
        );

        let uid = self.require_uid(dao, &jwt_data).await?;
        let rec = match dao
            .get_invitation(&invitation_id)
            .await
            .map_err(dao_internal)?
        {
            Some(r) => r,
            None => {
                return Ok(RespondToInvitationResponse::NotFound(PlainText(
                    "invitation not found".into(),
                )));
            }
        };
        // Only the targeted user may respond (user-kind invitation).
        if rec.invited_user_id.as_deref() != Some(uid.as_str()) {
            return Ok(RespondToInvitationResponse::Forbidden(PlainText(
                "this invitation is not addressed to you".into(),
            )));
        }

        let responded_at = now_iso();
        let status = match input.0.response {
            // Accept synchronously and atomically: bind the accepter to the
            // invitation, link the roster entry, and (match) write the accepter's
            // own feed row so the game is on their feed immediately. Follower
            // fan-out + notification happen async off the resulting stream event.
            membership::InvitationResponse::Accepted => {
                dao.accept_invitation_tx(&invitation_id, &uid, &responded_at, &responded_at)
                    .await
                    .map_err(|e| match e {
                        dao::DaoError::NotFound(_) => {
                            Error::from_string("not found", StatusCode::NOT_FOUND)
                        }
                        other => dao_internal(other),
                    })?;
                "accepted"
            }
            membership::InvitationResponse::Declined => {
                dao.respond_to_invitation(
                    &invitation_id,
                    "declined",
                    &responded_at,
                    &rec.invited_at,
                    true, // has a user inbox (GSI1) to realign
                )
                .await
                .map_err(|e| match e {
                    dao::DaoError::NotFound(_) => {
                        Error::from_string("not found", StatusCode::NOT_FOUND)
                    }
                    other => dao_internal(other),
                })?;
                "declined"
            }
        };

        let mut invitation = invitation_from_record(&rec);
        invitation.status = invitation_status_from_str(status);
        invitation.responded_at = Some(mapping::parse_ts(&responded_at));
        Ok(RespondToInvitationResponse::Invitation(Json(invitation)))
    }

    #[oai(path = "/invitations/respond-by-token", method = "post")]
    async fn respond_to_invitation_by_token(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<RespondByTokenInput>,
    ) -> Result<RespondByTokenResponse> {
        info!("Responding to invitation by token: {:?}", input.response);
        let input = input.0;

        // The caller (holder of the invite link) becomes the accepting account.
        // Resolving their uid is what lets a bare-token invite be linked to a
        // real user — without it the acceptance can't be attributed and never
        // reaches the roster / feed / stats.
        let uid = self.require_uid(dao, &jwt_data).await?;

        let rec = match dao
            .get_invitation_by_token(&input.invite_token)
            .await
            .map_err(dao_internal)?
        {
            Some(r) => r,
            None => {
                return Ok(RespondByTokenResponse::NotFound(PlainText(
                    "no invitation matches that token".into(),
                )));
            }
        };

        let responded_at = now_iso();
        let status = match input.response {
            // Accept synchronously: bind this account onto the (previously
            // userless) token invitation, link the roster entry, and write the
            // accepter's own feed row. Follower fan-out follows async.
            membership::InvitationResponse::Accepted => {
                dao.accept_invitation_tx(&rec.id, &uid, &responded_at, &responded_at)
                    .await
                    .map_err(|e| match e {
                        dao::DaoError::NotFound(_) => {
                            Error::from_string("not found", StatusCode::NOT_FOUND)
                        }
                        other => dao_internal(other),
                    })?;
                "accepted"
            }
            membership::InvitationResponse::Declined => {
                dao.respond_to_invitation(
                    &rec.id,
                    "declined",
                    &responded_at,
                    &rec.invited_at,
                    false,
                )
                .await
                .map_err(|e| match e {
                    dao::DaoError::NotFound(_) => {
                        Error::from_string("not found", StatusCode::NOT_FOUND)
                    }
                    other => dao_internal(other),
                })?;
                "declined"
            }
        };

        let mut invitation = invitation_from_record(&rec);
        invitation.status = invitation_status_from_str(status);
        invitation.responded_at = Some(mapping::parse_ts(&responded_at));
        Ok(RespondByTokenResponse::Invitation(Json(invitation)))
    }

    #[oai(path = "/users/:user_id/follow", method = "post")]
    async fn follow_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
    ) -> Result<FollowResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("User {uid} following user {user_id}");
        match dao.follow_user(&uid, &user_id, &now_iso()).await {
            Ok(()) => Ok(FollowResponse::Ok),
            Err(dao::DaoError::Conflict(msg)) => Ok(FollowResponse::NotFound(PlainText(msg))),
            Err(e) => Err(dao_internal(e)),
        }
    }

    #[oai(path = "/users/:user_id/follow", method = "delete")]
    async fn unfollow_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
    ) -> Result<FollowResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("User {uid} unfollowing user {user_id}");
        dao.unfollow_user(&uid, &user_id)
            .await
            .map_err(dao_internal)?;
        Ok(FollowResponse::Ok)
    }

    #[oai(path = "/users/:user_id/followers", method = "get")]
    async fn list_user_followers(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Listing followers of user {user_id}");
        let page = dao
            .list_user_followers(&user_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.follower_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids, Some(&uid)).await?;
        Ok(ListFollowsResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/users/:user_id/following", method = "get")]
    async fn list_user_following(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Listing users that user {user_id} follows");
        let page = dao
            .list_user_following(&user_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.followee_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids, Some(&uid)).await?;
        Ok(ListFollowsResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/teams/:team_id/follow", method = "post")]
    async fn follow_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
    ) -> Result<FollowResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("User {uid} following team {team_id}");
        dao.follow_team(&uid, &team_id, &now_iso())
            .await
            .map_err(dao_internal)?;
        Ok(FollowResponse::Ok)
    }

    #[oai(path = "/teams/:team_id/follow", method = "delete")]
    async fn unfollow_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
    ) -> Result<FollowResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("User {uid} unfollowing team {team_id}");
        dao.unfollow_team(&uid, &team_id)
            .await
            .map_err(dao_internal)?;
        Ok(FollowResponse::Ok)
    }

    #[oai(path = "/teams/:team_id/followers", method = "get")]
    async fn list_team_followers(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        let uid = self.require_uid(dao, &jwt_data).await?;
        info!("Listing followers of team {team_id}");
        let page = dao
            .list_team_followers(&team_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.follower_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids, Some(&uid)).await?;
        Ok(ListFollowsResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    /// Hydrate a list of user ids into `UserProfile`s via two batched
    /// exact-key reads across the whole page: the profile items, and — for a
    /// signed-in viewer — which of them the viewer follows. Missing users are
    /// skipped. `is_followed_by_me` is left false when there's no viewer (a
    /// follow-list view rarely needs it per-row); compute it if a screen
    /// requires it.
    async fn hydrate_user_profiles(
        &self,
        dao: &dao::Dao,
        ids: &[String],
        viewer_uid: Option<&str>,
    ) -> Result<Vec<UserProfile>> {
        let records = dao.batch_get_users(ids).await.map_err(dao_internal)?;
        let followed = match viewer_uid {
            Some(viewer) => dao
                .batch_is_following_users(viewer, ids)
                .await
                .map_err(dao_internal)?,
            None => std::collections::HashSet::new(),
        };

        // Preserve the caller's ordering; skip ids with no profile (matches the
        // old per-id `get_user` returning `None`).
        let mut profiles = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(record) = records.get(id) {
                profiles.push(user_profile_from_record(record, followed.contains(id)));
            }
        }
        Ok(profiles)
    }

    /// Hydrate a single match — see `hydrate_matches`, which does the actual
    /// work; this just wraps/unwraps the one-match case.
    async fn hydrate_match(&self, dao: &dao::Dao, m: Match, viewer_uid: &str) -> Result<Match> {
        Ok(self
            .hydrate_matches(dao, vec![m], viewer_uid)
            .await?
            .remove(0))
    }

    /// Hydrate every match in `matches` for one response: fill in each `User`
    /// player's `name`/`avatar_url` from their account, and resolve every
    /// side's display `name` — a custom name the creator gave the side (an
    /// ad-hoc side, or one of two sides sharing a team), else the sole
    /// player's name if there's exactly one, else the assigned team's name,
    /// else a fallback relative to `viewer_uid` — "Your side"/"Opposition" if
    /// they're actually playing in the match, else a neutral "Team A"/
    /// "Team B" by side order — plus, whenever that name is the team's,
    /// `team_logo` alongside it. Resolved per-request rather than stored, so
    /// a side's name/logo can't go stale and "your side" always reflects
    /// whoever is asking. Player and team lookups are each batched exactly
    /// once across every match passed in, however many that is — a feed/list
    /// page hands over the whole page at once rather than calling this per
    /// match.
    async fn hydrate_matches(
        &self,
        dao: &dao::Dao,
        mut matches: Vec<Match>,
        viewer_uid: &str,
    ) -> Result<Vec<Match>> {
        let user_ids: Vec<String> = matches.iter().flat_map(Self::player_user_ids).collect();
        let user_records = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;

        let team_ids: Vec<String> = matches
            .iter()
            .flat_map(|m| m.sides.iter().filter_map(|s| s.team_id.clone()))
            .collect();
        let team_metas = self.batch_team_metas(dao, &team_ids).await?;

        for m in &mut matches {
            Self::apply_player_profiles(m, &user_records);
            Self::resolve_side_names(m, viewer_uid, &team_metas);
        }
        Ok(matches)
    }

    /// Every `User` match player's account id (external players carry their
    /// name inline already, so aren't included).
    fn player_user_ids(m: &Match) -> Vec<String> {
        m.players
            .iter()
            .filter_map(|p| match &p.member {
                Member::User(u) => Some(u.user_id.clone()),
                Member::External(_) => None,
            })
            .collect()
    }

    /// Fill in each `User` team member's `name`/`avatar_url` from their
    /// account — the team-side counterpart of `hydrate_matches`/
    /// `apply_player_profiles`. `team_member_from_record` (a pure DAO->API
    /// mapping, no DB access) can't do this itself, so `list_team_members`
    /// routes its page through here first.
    async fn hydrate_team_members(
        &self,
        dao: &dao::Dao,
        mut members: Vec<TeamMember>,
    ) -> Result<Vec<TeamMember>> {
        let user_ids: Vec<String> = members
            .iter()
            .filter_map(|m| match &m.member {
                Member::User(u) => Some(u.user_id.clone()),
                Member::External(_) => None,
            })
            .collect();
        let records = dao.batch_get_users(&user_ids).await.map_err(dao_internal)?;
        for member in &mut members {
            if let Member::User(u) = &mut member.member
                && let Some(record) = records.get(&u.user_id)
            {
                u.name = record.name.clone();
                u.avatar_url = record.profile_image_url.clone();
            }
        }
        Ok(members)
    }

    /// Fill in each `User` match player's `name`/`avatar_url` from `records`
    /// (keyed by user id, as returned by `batch_get_users`). Pure/sync.
    fn apply_player_profiles(
        m: &mut Match,
        records: &std::collections::HashMap<String, dao::records::UserRecord>,
    ) {
        for player in &mut m.players {
            if let Member::User(u) = &mut player.member
                && let Some(record) = records.get(&u.user_id)
            {
                u.name = record.name.clone();
                u.avatar_url = record.profile_image_url.clone();
            }
        }
    }

    /// Batch-fetch team meta (name + logo) for the given ids (deduped,
    /// missing ids simply absent from the result), keyed by team id — the
    /// side-name *and* side-logo fallback chains both read off this one
    /// batch.
    async fn batch_team_metas(
        &self,
        dao: &dao::Dao,
        team_ids: &[String],
    ) -> Result<std::collections::HashMap<String, dao::records::TeamRecord>> {
        if team_ids.is_empty() {
            return Ok(Default::default());
        }
        dao.batch_get_team_metas(team_ids).await.map_err(dao_internal)
    }

    /// Resolve one match's side names (and, wherever the resolved name is
    /// actually the team's, `team_logo` alongside it), given `team_metas`
    /// already resolved by the caller. Pure/sync — no DAO calls — so it's
    /// cheap to run per match after a shared batch team-meta lookup (see
    /// `hydrate_matches`).
    fn resolve_side_names(
        m: &mut Match,
        viewer_uid: &str,
        team_metas: &std::collections::HashMap<String, dao::records::TeamRecord>,
    ) {
        // The side the viewer is actually on (by an invite they were placed
        // on, accepted or not) — used for the "Your side"/"Opposition"
        // fallback below.
        let viewer_side_id = m.players.iter().find_map(|p| match &p.member {
            Member::User(u) if u.user_id == viewer_uid => p.side_id.clone(),
            _ => None,
        });

        for (i, side) in m.sides.iter_mut().enumerate() {
            let on_side: Vec<&MatchPlayer> = m
                .players
                .iter()
                .filter(|p| p.side_id.as_deref() == Some(side.id.as_str()))
                .collect();
            let sole_player_name = match on_side.as_slice() {
                [p] => Some(match &p.member {
                    Member::User(u) => u.name.clone(),
                    Member::External(e) => e.display_name.clone(),
                }),
                _ => None,
            };
            // Same "small enough to show players directly" call the feed
            // makes from its denormalized cache (`ROSTER_PREVIEW_CAP`) — here
            // computed live, since the full roster is already in memory.
            side.roster_preview = (on_side.len() <= agon_core::dao::match_ops::ROSTER_PREVIEW_CAP
                && !on_side.is_empty())
            .then(|| {
                on_side
                    .iter()
                    .map(|p| match &p.member {
                        Member::User(u) => RosterPreviewPlayer {
                            user_id: Some(u.user_id.clone()),
                            name: u.name.clone(),
                            avatar_url: u.avatar_url.clone(),
                        },
                        Member::External(e) => RosterPreviewPlayer {
                            user_id: None,
                            name: e.display_name.clone(),
                            avatar_url: None,
                        },
                    })
                    .collect()
            });

            let custom_name = side
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty());

            // Resolves alongside `name` below: `team_logo` is only ever set
            // when the side falls all the way through to the team-name
            // branch — a custom name or a sole player's name means the team
            // isn't actually what's being shown, so its logo shouldn't show
            // either (see doc comment on `MatchSide::team_logo`).
            side.team_logo = None;

            side.name = Some(match custom_name {
                // An explicit name always wins, over both the sole player's
                // name and the team's own name — it's there specifically
                // because the creator wanted something other than either
                // default (e.g. to tell two sides sharing one team apart).
                Some(name) => name.to_string(),
                None => match sole_player_name {
                    Some(name) => name,
                    None => match &side.team_id {
                        Some(team_id) => match team_metas.get(team_id) {
                            Some(team) => {
                                side.team_logo = team.logo_url.as_ref().map(|url| Photo {
                                    image_url: url.clone(),
                                    asset_id: None,
                                });
                                team.name.clone()
                            }
                            None => "Team".to_string(),
                        },
                        None => match &viewer_side_id {
                            Some(vs) if vs == &side.id => "Your side".to_string(),
                            Some(_) => "Opposition".to_string(),
                            None if i == 0 => "Team A".to_string(),
                            None => "Team B".to_string(),
                        },
                    },
                },
            });
        }
    }

    /// The same side-name priority chain as [`Self::resolve_side_names`]
    /// (custom name → sole player's name → team name → "Your
    /// side"/"Opposition" → neutral "Team A"/"Team B"), plus the same
    /// `team_logo` resolution, for a `FeedMatch` or `SearchMatch` — which
    /// never have the full player list to scan.
    ///
    /// `roster_preview` already gives the *complete* roster whenever a side
    /// has one player, so "the sole player's name" is recovered from it
    /// (`Some([p])`) rather than the live roster — same fact, cheaper source.
    /// `viewer_side_id` is `None` for a search hit (not derived from a
    /// per-viewer fan-out, so there's no "Your side" to resolve) and
    /// `Some`/`None` per feed entry for a feed card.
    fn resolve_side_names_from_cache(
        sides: &mut [MatchSide],
        viewer_side_id: Option<&str>,
        team_metas: &std::collections::HashMap<String, dao::records::TeamRecord>,
    ) {
        for (i, side) in sides.iter_mut().enumerate() {
            let sole_player_name = match side.roster_preview.as_deref() {
                Some([p]) => Some(p.name.clone()),
                _ => None,
            };
            let custom_name = side
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty());

            side.team_logo = None;

            side.name = Some(match custom_name {
                // Same priority as `resolve_side_names`: an explicit name
                // always wins over the sole player's name.
                Some(name) => name.to_string(),
                None => match sole_player_name {
                    Some(name) => name,
                    None => match &side.team_id {
                        Some(team_id) => match team_metas.get(team_id) {
                            Some(team) => {
                                side.team_logo = team.logo_url.as_ref().map(|url| Photo {
                                    image_url: url.clone(),
                                    asset_id: None,
                                });
                                team.name.clone()
                            }
                            None => "Team".to_string(),
                        },
                        None => match viewer_side_id {
                            Some(vs) if vs == side.id => "Your side".to_string(),
                            Some(_) => "Opposition".to_string(),
                            None if i == 0 => "Team A".to_string(),
                            None => "Team B".to_string(),
                        },
                    },
                },
            });
        }
    }

    /// Fetch a single user's public profile, or `None` if absent. Used to embed
    /// an author/actor inline. (N+1 in list contexts — batch later.)
    async fn try_user_profile(&self, dao: &dao::Dao, user_id: &str) -> Result<Option<UserProfile>> {
        match dao.get_user(user_id).await.map_err(dao_internal)? {
            Some(record) => Ok(Some(user_profile_from_record(&record, false))),
            None => Ok(None),
        }
    }

    /// Map comment records to API `Comment`s, hydrating each author profile
    /// (tombstoned comments have no author). N+1; batch later.
    async fn hydrate_comments(
        &self,
        dao: &dao::Dao,
        records: Vec<dao::records::CommentRecord>,
    ) -> Result<Vec<Comment>> {
        let mut out = Vec::with_capacity(records.len());
        for rec in records {
            let author = match &rec.author_user_id {
                Some(uid) => self.try_user_profile(dao, uid).await?,
                None => None,
            };
            out.push(comment_from_record(&rec, author));
        }
        Ok(out)
    }
}

/// Re-point a `Score`'s request-scoped client ids to the real side/player ids
/// assigned at creation — side ids via `CreateMatchSideInput.client_id`,
/// player ids via a real user's own id or a guest's `CreateMatchExternalInviteInput.client_id`
/// (see `create_match`). Returns `None` if any referenced side or player is
/// unknown.
fn resolve_score_ids(
    score: &Score,
    side_ids: &std::collections::HashMap<String, String>,
    player_ids: &std::collections::HashMap<String, String>,
) -> Option<Score> {
    let map = |client_id: &str| side_ids.get(client_id).cloned();
    let pmap = |client_id: &str| player_ids.get(client_id).cloned();
    let pmap_opt = |id: &Option<String>| -> Option<Option<String>> {
        match id {
            Some(id) => pmap(id).map(Some),
            None => Some(None),
        }
    };
    match score {
        Score::Simple(s) => {
            let mut entries = HashMap::with_capacity(s.entries.len());
            for (side_id, points) in &s.entries {
                entries.insert(map(side_id)?, *points);
            }
            Some(Score::Simple(SimpleScore { entries }))
        }
        Score::Sets(s) => {
            let mut entries = HashMap::with_capacity(s.entries.len());
            for (side_id, sets) in &s.entries {
                entries.insert(map(side_id)?, sets.clone());
            }
            Some(Score::Sets(SetsScore { entries }))
        }
        Score::Cricket(s) => {
            let mut innings = Vec::with_capacity(s.innings.len());
            for i in &s.innings {
                let batting = match &i.batting {
                    Some(bs) => {
                        let mut out = Vec::with_capacity(bs.len());
                        for b in bs {
                            let dismissal = match &b.dismissal {
                                Some(d) => Some(CricketDismissal {
                                    kind: d.kind.clone(),
                                    bowler_player_id: pmap_opt(&d.bowler_player_id)?,
                                    fielder_player_id: pmap_opt(&d.fielder_player_id)?,
                                }),
                                None => None,
                            };
                            out.push(CricketBattingEntry {
                                player_id: pmap(&b.player_id)?,
                                runs: b.runs,
                                balls_faced: b.balls_faced,
                                fours: b.fours,
                                sixes: b.sixes,
                                dismissal,
                                batting_position: b.batting_position,
                            });
                        }
                        Some(out)
                    }
                    None => None,
                };
                let bowling = match &i.bowling {
                    Some(bs) => {
                        let mut out = Vec::with_capacity(bs.len());
                        for b in bs {
                            out.push(CricketBowlingEntry {
                                player_id: pmap(&b.player_id)?,
                                overs: b.overs,
                                maidens: b.maidens,
                                runs_conceded: b.runs_conceded,
                                wickets: b.wickets,
                                wides: b.wides,
                                no_balls: b.no_balls,
                            });
                        }
                        Some(out)
                    }
                    None => None,
                };
                let fall_of_wickets = match &i.fall_of_wickets {
                    Some(fs) => {
                        let mut out = Vec::with_capacity(fs.len());
                        for f in fs {
                            out.push(CricketFallOfWicket {
                                wicket: f.wicket,
                                runs: f.runs,
                                player_id: pmap(&f.player_id)?,
                                overs: f.overs,
                            });
                        }
                        Some(out)
                    }
                    None => None,
                };
                innings.push(CricketScoreInnings {
                    batting_side_id: map(&i.batting_side_id)?,
                    bowling_side_id: map(&i.bowling_side_id)?,
                    runs: i.runs,
                    wickets: i.wickets,
                    overs: i.overs,
                    declared: i.declared,
                    batting,
                    bowling,
                    fall_of_wickets,
                    extras: i.extras.clone(),
                });
            }
            let recent_deliveries = match &s.recent_deliveries {
                Some(ds) => {
                    let mut out = Vec::with_capacity(ds.len());
                    for d in ds {
                        out.push(CricketDelivery {
                            over: d.over,
                            ball: d.ball,
                            bowler_player_id: pmap(&d.bowler_player_id)?,
                            striker_player_id: pmap(&d.striker_player_id)?,
                            non_striker_player_id: pmap(&d.non_striker_player_id)?,
                            runs_off_bat: d.runs_off_bat,
                            extra: d.extra.clone(),
                            wicket: match &d.wicket {
                                Some(w) => Some(CricketDeliveryWicket {
                                    kind: w.kind.clone(),
                                    dismissed_player_id: pmap(&w.dismissed_player_id)?,
                                    bowler_player_id: pmap_opt(&w.bowler_player_id)?,
                                    fielder_player_id: pmap_opt(&w.fielder_player_id)?,
                                }),
                                None => None,
                            },
                            occurred_at: d.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let next_ball_context = match &s.next_ball_context {
                Some(ctx) => Some(NextBallContext {
                    striker_player_id: pmap_opt(&ctx.striker_player_id)?,
                    non_striker_player_id: pmap_opt(&ctx.non_striker_player_id)?,
                    bowler_player_id: pmap_opt(&ctx.bowler_player_id)?,
                    over: ctx.over,
                    ball: ctx.ball,
                    previous_over_bowler_player_id: pmap_opt(&ctx.previous_over_bowler_player_id)?,
                    runs_conceded_this_over: ctx.runs_conceded_this_over,
                }),
                None => None,
            };
            Some(Score::Cricket(CricketScore {
                innings,
                recent_deliveries,
                next_ball_context,
                awaiting_next_innings: s.awaiting_next_innings,
                players: HashMap::new(),
            }))
        }
        Score::Football(s) => {
            let mut score = HashMap::with_capacity(s.score.len());
            for (side_id, goals) in &s.score {
                score.insert(map(side_id)?, *goals);
            }
            let goals = match &s.goals {
                Some(gs) => {
                    let mut out = Vec::with_capacity(gs.len());
                    for g in gs {
                        out.push(FootballGoalEvent {
                            side_id: map(&g.side_id)?,
                            scorer_player_id: pmap_opt(&g.scorer_player_id)?,
                            assist_player_id: pmap_opt(&g.assist_player_id)?,
                            own_goal: g.own_goal,
                            penalty: g.penalty,
                            minute: g.minute,
                            occurred_at: g.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let cards = match &s.cards {
                Some(cs) => {
                    let mut out = Vec::with_capacity(cs.len());
                    for c in cs {
                        out.push(FootballCardEvent {
                            side_id: map(&c.side_id)?,
                            player_id: pmap(&c.player_id)?,
                            color: c.color.clone(),
                            minute: c.minute,
                            occurred_at: c.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let substitutions = match &s.substitutions {
                Some(subs) => {
                    let mut out = Vec::with_capacity(subs.len());
                    for sub in subs {
                        out.push(FootballSubstitutionEvent {
                            side_id: map(&sub.side_id)?,
                            player_in_id: pmap(&sub.player_in_id)?,
                            player_out_id: pmap(&sub.player_out_id)?,
                            minute: sub.minute,
                            occurred_at: sub.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let penalty_shootout = match &s.penalty_shootout {
                Some(ks) => {
                    let mut out = Vec::with_capacity(ks.len());
                    for k in ks {
                        out.push(FootballPenaltyShootoutKick {
                            side_id: map(&k.side_id)?,
                            scored: k.scored,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let penalty_shootout_score = match &s.penalty_shootout_score {
                Some(pss) => {
                    let mut out = HashMap::with_capacity(pss.len());
                    for (side_id, kicks) in pss {
                        out.insert(map(side_id)?, *kicks);
                    }
                    Some(out)
                }
                None => None,
            };
            Some(Score::Football(FootballScore {
                score,
                goals,
                cards,
                substitutions,
                period: s.period,
                period_times: s.period_times.clone(),
                penalty_shootout,
                penalty_shootout_score,
                players: HashMap::new(),
            }))
        }
        Score::Netball(s) => {
            let mut score = HashMap::with_capacity(s.score.len());
            for (side_id, goals) in &s.score {
                score.insert(map(side_id)?, *goals);
            }
            let goals = match &s.goals {
                Some(gs) => {
                    let mut out = Vec::with_capacity(gs.len());
                    for g in gs {
                        out.push(NetballGoalEvent {
                            side_id: map(&g.side_id)?,
                            scorer_player_id: pmap_opt(&g.scorer_player_id)?,
                            scorer_position: g.scorer_position,
                            two_points: g.two_points,
                            minute: g.minute,
                            occurred_at: g.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let fouls = match &s.fouls {
                Some(fs) => {
                    let mut out = Vec::with_capacity(fs.len());
                    for fo in fs {
                        out.push(NetballFoulEvent {
                            side_id: map(&fo.side_id)?,
                            player_id: pmap_opt(&fo.player_id)?,
                            foul_kind: fo.foul_kind,
                            minute: fo.minute,
                            occurred_at: fo.occurred_at,
                        });
                    }
                    Some(out)
                }
                None => None,
            };
            let period_scores = match &s.period_scores {
                Some(pss) => {
                    let mut out = HashMap::with_capacity(pss.len());
                    for (period, entries) in pss {
                        let mut mapped = HashMap::with_capacity(entries.len());
                        for (side_id, goals) in entries {
                            mapped.insert(map(side_id)?, *goals);
                        }
                        out.insert(*period, mapped);
                    }
                    Some(out)
                }
                None => None,
            };
            Some(Score::Netball(NetballScore {
                score,
                goals,
                fouls,
                period: s.period,
                period_times: s.period_times.clone(),
                period_scores,
                players: HashMap::new(),
            }))
        }
    }
}

/// Set a score's resolved-players map (`CricketScore.players`/
/// `FootballScore.players`), whichever variant it is. No-op for
/// `Score::Simple`/`Score::Sets`, which have no such field.
fn set_score_players(score: &mut Score, resolved: HashMap<String, RosterPreviewPlayer>) {
    match score {
        Score::Cricket(s) => s.players = resolved,
        Score::Football(s) => s.players = resolved,
        Score::Netball(s) => s.players = resolved,
        Score::Simple(_) | Score::Sets(_) => {}
    }
}

/// Look up `score`'s own referenced player ids (`score_player_ids`) in a
/// cross-match batch result (keyed by `(match_id, player_id)`) and set
/// `score`'s resolved-players map from whichever of them belong to
/// `match_id`. Shared by [`Api::hydrate_confirmed_pending_score_players`]'s
/// `confirmed_score`/`pending_score` handling.
fn resolve_score_players_for_match(
    score: &mut Score,
    match_id: &str,
    match_players: &HashMap<(String, String), dao::records::MatchPlayerRecord>,
    users: &HashMap<String, dao::records::UserRecord>,
) {
    let ids = score_player_ids(score);
    if ids.is_empty() {
        return;
    }
    let resolved: HashMap<String, RosterPreviewPlayer> = ids
        .into_iter()
        .filter_map(|pid| {
            let p = match_players.get(&(match_id.to_string(), pid.clone()))?;
            Some((
                pid,
                roster_preview_player(p.user_id.as_deref(), p.display_name.as_deref(), users),
            ))
        })
        .collect();
    set_score_players(score, resolved);
}

/// Every player id referenced anywhere in a score — the set
/// `Api::hydrate_score_players` resolves names for. Empty (nothing to
/// resolve) for `Score::Simple`/`Score::Sets`, which don't reference
/// players by id at all.
fn score_player_ids(score: &Score) -> Vec<String> {
    match score {
        Score::Cricket(s) => cricket_score_player_ids(s),
        Score::Football(s) => football_score_player_ids(s),
        Score::Netball(s) => netball_score_player_ids(s),
        Score::Simple(_) | Score::Sets(_) => Vec::new(),
    }
}

/// Every player id referenced in a `CricketScore`: `next_ball_context`'s
/// striker/non-striker/bowler/previous-over-bowler, each innings' batting/
/// bowling/fall-of-wicket entries (plus a batting entry's dismissal bowler/
/// fielder), and `recent_deliveries` (plus each delivery's wicket). May
/// repeat the same id many times over (e.g. a bowler across several
/// deliveries) — `Dao::batch_get_match_players` dedupes before querying.
fn cricket_score_player_ids(score: &CricketScore) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(ctx) = &score.next_ball_context {
        ids.extend(ctx.striker_player_id.clone());
        ids.extend(ctx.non_striker_player_id.clone());
        ids.extend(ctx.bowler_player_id.clone());
        ids.extend(ctx.previous_over_bowler_player_id.clone());
    }
    for innings in &score.innings {
        for entry in innings.batting.iter().flatten() {
            ids.push(entry.player_id.clone());
            if let Some(d) = &entry.dismissal {
                ids.extend(d.bowler_player_id.clone());
                ids.extend(d.fielder_player_id.clone());
            }
        }
        for entry in innings.bowling.iter().flatten() {
            ids.push(entry.player_id.clone());
        }
        for fow in innings.fall_of_wickets.iter().flatten() {
            ids.push(fow.player_id.clone());
        }
    }
    for delivery in score.recent_deliveries.iter().flatten() {
        ids.push(delivery.bowler_player_id.clone());
        ids.push(delivery.striker_player_id.clone());
        ids.push(delivery.non_striker_player_id.clone());
        if let Some(w) = &delivery.wicket {
            ids.push(w.dismissed_player_id.clone());
            ids.extend(w.bowler_player_id.clone());
            ids.extend(w.fielder_player_id.clone());
        }
    }
    ids
}

/// Every player id referenced in a `FootballScore`: each goal's scorer/
/// assist, each card's player, each substitution's player in/out. Same
/// "may repeat, deduped downstream" contract as [`cricket_score_player_ids`].
fn football_score_player_ids(score: &FootballScore) -> Vec<String> {
    let mut ids = Vec::new();
    for goal in score.goals.iter().flatten() {
        ids.extend(goal.scorer_player_id.clone());
        ids.extend(goal.assist_player_id.clone());
    }
    for card in score.cards.iter().flatten() {
        ids.push(card.player_id.clone());
    }
    for sub in score.substitutions.iter().flatten() {
        ids.push(sub.player_in_id.clone());
        ids.push(sub.player_out_id.clone());
    }
    ids
}

/// Every player id referenced in a `NetballScore`: each goal's scorer, each
/// foul's player. Same "may repeat, deduped downstream" contract as
/// [`cricket_score_player_ids`].
fn netball_score_player_ids(score: &NetballScore) -> Vec<String> {
    let mut ids = Vec::new();
    for goal in score.goals.iter().flatten() {
        ids.extend(goal.scorer_player_id.clone());
    }
    for foul in score.fouls.iter().flatten() {
        ids.extend(foul.player_id.clone());
    }
    ids
}

/// Derives the winner (when decidable) from a live-scored match's persisted
/// score — used by `update_match` to finish a live-scored match without the
/// client having to work out and resubmit a margin the server already has
/// enough to compute (see `LiveScoringPage`/`CricketLiveScoringPage`'s
/// `finishMatch`). `side_ids` is the match's own two side ids, needed
/// because a tally map only holds entries for sides that are actually on the
/// board — "absence means zero" — so the winner comparison needs to know
/// both ids to look up, not just iterate whatever's present. `None` for
/// `Score::Simple`/`Score::Sets` — winner derivation for those sports isn't
/// this function's job (the client always supplies `winner_side_id` itself
/// for a manual entry/correction on those sports).
fn winner_from_score(score: &Score, side_ids: &[String]) -> Option<String> {
    match score {
        Score::Football(s) => {
            // Still level on goals falls back to the penalty-shootout tally,
            // same as the client-side logic this replaces.
            two_side_winner(side_ids, |sid| *s.score.get(sid).unwrap_or(&0) as i64).or_else(|| {
                two_side_winner(side_ids, |sid| {
                    s.penalty_shootout_score
                        .as_ref()
                        .and_then(|pss| pss.get(sid))
                        .copied()
                        .unwrap_or(0) as i64
                })
            })
        }
        Score::Cricket(s) => {
            // The winner is the summed match totals (two-innings formats add
            // up both) — the same comparison `CricketLiveScoringPage` used to
            // make client-side.
            let mut totals: HashMap<&str, u32> = HashMap::new();
            for i in &s.innings {
                *totals.entry(i.batting_side_id.as_str()).or_insert(0) += i.runs;
            }
            two_side_winner(side_ids, |sid| *totals.get(sid).unwrap_or(&0) as i64)
        }
        Score::Netball(s) => {
            two_side_winner(side_ids, |sid| *s.score.get(sid).unwrap_or(&0) as i64)
        }
        Score::Simple(_) | Score::Sets(_) => None,
    }
}

/// Compares two sides by a score function and returns the higher-scoring
/// side's id — `None` if tied, or if there aren't exactly two sides (every
/// match today is exactly two, but this stays honest rather than guessing
/// for a hypothetical multi-side match).
fn two_side_winner(side_ids: &[String], score_for: impl Fn(&str) -> i64) -> Option<String> {
    let [a_id, b_id] = side_ids else { return None };
    let a = score_for(a_id);
    let b = score_for(b_id);
    if a == b {
        None
    } else if a > b {
        Some(a_id.clone())
    } else {
        Some(b_id.clone())
    }
}

/// Build a match player record plus the standalone invitation entity for one
/// invitee (an Agon user or an external). The player and invitation share the
/// invitation id/status; externals get a minted token.
/// Whether `uid` is a participant in the match — a player linked to this Agon
/// user who has either been added ad-hoc (no invitation) or accepted their
/// invitation. Pending/declined invitees are not participants. Drives who may
/// edit a match (its metadata, roster, and result).
fn caller_is_participant(players: &[dao::records::MatchPlayerRecord], uid: &str) -> bool {
    players.iter().any(|p| {
        p.user_id.as_deref() == Some(uid)
            && match &p.invitation {
                None => true,
                Some(inv) => inv.status == "accepted",
            }
    })
}

/// Whether the caller may manage the match (edit, invite, record the result):
/// the creator who organized it, or any participant. The creator can manage a
/// match they set up between other people without playing in it themselves.
fn caller_can_manage_match(agg: &dao::match_ops::MatchAggregate, uid: &str) -> bool {
    agg.match_.created_by_user_id == uid || caller_is_participant(&agg.players, uid)
}

fn build_invited_player(
    match_id: &str,
    match_name: &str,
    invited_by_user_id: &str,
    side_id: Option<String>,
    user_id: Option<String>,
    display_name: Option<String>,
    now: &str,
) -> (
    dao::records::MatchPlayerRecord,
    dao::records::InvitationRecord,
) {
    let invitation_id = new_id();
    let player_id = new_id();

    let (kind, invited_user_id, invite_token) = match &user_id {
        Some(uid) => (
            dao::records::InvitationKindRecord::User {
                invited_user_id: uid.clone(),
            },
            Some(uid.clone()),
            None,
        ),
        None => {
            let token = new_id();
            (
                dao::records::InvitationKindRecord::Token {
                    invite_token: token.clone(),
                },
                None,
                Some(token),
            )
        }
    };

    let embedded = dao::records::EmbeddedInvitationRecord {
        id: invitation_id.clone(),
        status: String::from("pending"),
        invited_by_user_id: invited_by_user_id.to_string(),
        invited_at: now.to_string(),
        responded_at: None,
        kind: kind.clone(),
    };

    let player = dao::records::MatchPlayerRecord {
        player_id,
        user_id: user_id.clone(),
        display_name: display_name.clone(),
        side_id,
        is_member_of_team: None,
        invitation: Some(embedded),
    };

    let invitation = dao::records::InvitationRecord {
        id: invitation_id,
        status: String::from("pending"),
        invited_by_user_id: invited_by_user_id.to_string(),
        invited_user_id,
        invite_token,
        kind,
        context: dao::records::InvitationContextRecord::Match {
            match_id: match_id.to_string(),
            match_name: match_name.to_string(),
        },
        invited_at: now.to_string(),
        responded_at: None,
    };

    (player, invitation)
}

/// Build a pending team-member roster slot together with its standalone
/// invitation, exactly the team-side counterpart of `build_invited_player`:
/// the member carries the invitation embedded (so it shows up on the team's
/// member list immediately, pre-accepted-looking as pending) while the
/// standalone `InvitationRecord` drives the inbox/notification and is what
/// `Dao::accept_invitation_tx` looks up by id to link the two. Shared by
/// `add_team_invitations` and `create_team`'s bundled initial invites.
fn build_invited_team_member(
    team_id: &str,
    team_name: &str,
    invited_by_user_id: &str,
    user_id: Option<String>,
    display_name: Option<String>,
    now: &str,
) -> (
    dao::records::TeamMemberRecord,
    dao::records::InvitationRecord,
) {
    let invitation_id = new_id();
    let membership_id = new_id();

    let (kind, invited_user_id, invite_token) = match &user_id {
        Some(uid) => (
            dao::records::InvitationKindRecord::User {
                invited_user_id: uid.clone(),
            },
            Some(uid.clone()),
            None,
        ),
        None => {
            let token = new_id();
            (
                dao::records::InvitationKindRecord::Token {
                    invite_token: token.clone(),
                },
                None,
                Some(token),
            )
        }
    };

    let embedded = dao::records::EmbeddedInvitationRecord {
        id: invitation_id.clone(),
        status: String::from("pending"),
        invited_by_user_id: invited_by_user_id.to_string(),
        invited_at: now.to_string(),
        responded_at: None,
        kind: kind.clone(),
    };

    let member = dao::records::TeamMemberRecord {
        team_id: team_id.to_string(),
        membership_id,
        user_id: user_id.clone(),
        display_name: display_name.clone(),
        role: String::from("member"),
        invitation: Some(embedded),
        created_at: now.to_string(),
    };

    let invitation = dao::records::InvitationRecord {
        id: invitation_id,
        status: String::from("pending"),
        invited_by_user_id: invited_by_user_id.to_string(),
        invited_user_id,
        invite_token,
        kind,
        context: dao::records::InvitationContextRecord::Team {
            team_id: team_id.to_string(),
            team_name: team_name.to_string(),
        },
        invited_at: now.to_string(),
        responded_at: None,
    };

    (member, invitation)
}

/// The stored string tag for an upload purpose.
fn upload_purpose_str(p: &UploadPurpose) -> &'static str {
    match p {
        UploadPurpose::ProfileImage => "profile_image",
        UploadPurpose::TeamLogo => "team_logo",
        UploadPurpose::MatchHeader => "match_header",
    }
}

/// Map the API `AssetStatus` string tag stored on a record.
fn asset_status_from_str(s: &str) -> AssetStatus {
    match s {
        "uploaded" => AssetStatus::Uploaded,
        "failed" => AssetStatus::Failed,
        _ => AssetStatus::Pending,
    }
}

/// Build the API `Asset` from a stored record. Pending assets get a freshly
/// generated S3 presigned PUT (short-lived, so regenerated on each read — that is
/// the upload-retry mechanism); uploaded assets carry their serving `url` and no
/// target.
///
/// The stored `url` on an uploaded asset is the canonical CloudFront URL. For
/// public purposes (profile/team) it is returned as-is; a match-header asset's
/// url is signed here so the `Asset` view is directly usable. (Match headers are
/// also signed when attached to a `Match`; signing both keeps `GET /assets/:id`
/// consistent.)
async fn asset_from_record(assets: &Assets, record: &dao::records::AssetRecord) -> Asset {
    let status = asset_status_from_str(&record.status);
    let upload = match status {
        AssetStatus::Pending => {
            match assets
                .presign_put(
                    &record.storage_key,
                    &record.content_type,
                    record.content_length,
                )
                .await
            {
                Ok(target) => Some(target),
                Err(e) => {
                    // Don't fail the whole response: return the pending asset with
                    // no upload target. The client can re-read to retry once the
                    // transient issue clears.
                    error!(error = %e, asset = %record.id, "failed to presign upload; returning asset without target");
                    None
                }
            }
        }
        _ => None,
    };
    let url = record.url.as_ref().map(|url| {
        if record.purpose == "match_header" {
            assets.sign_get(url)
        } else {
            url.clone()
        }
    });
    Asset {
        id: record.id.clone(),
        status,
        content_type: record.content_type.clone(),
        upload,
        url,
    }
}

/// Resolve a list of asset ids to their own id plus stored (canonical) URL, in
/// order, enforcing that each is `Uploaded`, owned by `uid`, and of the
/// expected `purpose`. Returns `Err` with a client-facing message on the first
/// asset that fails a check, so the caller can surface a 400. An empty input
/// yields an empty list.
async fn resolve_asset_urls(
    dao: &dao::Dao,
    uid: &str,
    purpose: &str,
    asset_ids: &[String],
) -> Result<std::result::Result<Vec<(String, String)>, String>> {
    let mut resolved = Vec::with_capacity(asset_ids.len());
    for asset_id in asset_ids {
        let asset = dao.get_asset(asset_id).await.map_err(dao_internal)?;
        match asset {
            Some(a) if a.status == "uploaded" && a.owner_user_id == uid && a.purpose == purpose => {
                match a.url {
                    Some(url) => resolved.push((asset_id.clone(), url)),
                    // Uploaded but no URL recorded — treat as not-yet-usable.
                    None => {
                        return Ok(Err(format!(
                            "asset {asset_id} has no url yet; try again shortly"
                        )));
                    }
                }
            }
            _ => {
                return Ok(Err(format!(
                    "asset {asset_id} not found, not uploaded, wrong type, or not owned by you"
                )));
            }
        }
    }
    Ok(Ok(resolved))
}

/// Sign a `Match`'s header-photo URLs for private serving. The mapping layer
/// stores each photo's canonical (unsigned) CloudFront URL; match headers are a
/// signed CloudFront behaviour, so we mint a short-lived signed URL at read time
/// (and, in future, only after a per-match visibility check). A no-op in dev,
/// where signing isn't configured. Call this on every `Match` returned to a
/// client.
fn sign_match_headers(assets: &Assets, m: &mut Match) {
    for photo in &mut m.header_photos {
        photo.image_url = assets.sign_get(&photo.image_url);
    }
}

fn sign_feed_match_headers(assets: &Assets, m: &mut FeedMatch) {
    for photo in &mut m.header_photos {
        photo.image_url = assets.sign_get(&photo.image_url);
    }
}

fn sign_search_match_headers(assets: &Assets, m: &mut SearchMatch) {
    for photo in &mut m.header_photos {
        photo.image_url = assets.sign_get(&photo.image_url);
    }
}

/// Builds a mock `Pending` asset with a presigned upload target.
fn mock_pending_asset(id: String, content_type: String) -> Asset {
    Asset {
        id,
        status: AssetStatus::Pending,
        upload: Some(UploadTarget {
            upload_url: String::from("https://storage.example.com/uploads/asset_123?signature=abc"),
            method: String::from("PUT"),
            headers: vec![UploadHeader {
                name: String::from("Content-Type"),
                value: content_type.clone(),
            }],
        }),
        content_type,
        url: None,
    }
}

/// Builds the mock current user (the caller's own view: profile + private email).
fn mock_user() -> User {
    User {
        email: String::from("jamesnelgar@gmail.com"),
        profile: mock_user_profile(String::from("123"), String::from("James Elgar")),
    }
}

/// Builds a mock match comment.
fn mock_comment() -> Comment {
    Comment {
        id: String::from("comment_1"),
        author: Some(mock_user_profile(
            String::from("user_2"),
            String::from("Raj Patel"),
        )),
        text: Some(String::from("Tough match — you fought hard!")),
        created_at: mock_timestamp(),
        edited_at: None,
        parent_id: None,
        reply_count: 2,
        deleted_at: None,
    }
}

/// Builds a mock user profile. Shared across the user/search/follow endpoints
/// until a real DAO is wired in.
fn mock_user_profile(id: String, name: String) -> UserProfile {
    UserProfile {
        id,
        name,
        profile_image: Some(Photo {
            image_url: String::from("https://cdn.example.com/users/avatar.jpg"),
            asset_id: None,
        }),
        stats: UserStats {
            cricket: None,
            football: None,
            tennis: Some(GenericPlayerStats {
                matches_played: 12,
                wins: 7,
                draws: 0,
                losses: 5,
                win_percentage: Some(58.3),
            }),
            badminton: None,
            squash: None,
            table_tennis: None,
            netball: None,
            other: None,
        },
        follower_count: 42,
        following_count: 17,
        is_followed_by_me: false,
    }
}

/// Builds a mock match for the given id. Shared by the feed and get-match
/// endpoints until a real DAO lookup is wired in.
fn mock_match(id: String) -> Match {
    Match {
        id,
        name: String::from("Sunday League 5-a-side"),
        description: String::from("Match at the local astro pitch"),
        match_type: MatchType::Football,
        status: MatchStatus::Completed,
        starts_at: mock_timestamp(),
        location: Some(Location {
            latitude: 51.5074,
            longitude: -0.1278,
        }),
        header_photos: vec![Photo {
            image_url: String::from("https://cdn.example.com/matches/match_123/header.jpg"),
            asset_id: Some(String::from("asset_123")),
        }],
        sides: vec![
            MatchSide {
                id: String::from("side_red"),
                team_id: Some(String::from("team_red")),
                name: Some(String::from("Red Team")),
                team_logo: None,
                roster_preview: None,
            },
            MatchSide {
                id: String::from("side_blue"),
                team_id: Some(String::from("team_blue")),
                name: Some(String::from("Blue Team")),
                team_logo: None,
                roster_preview: None,
            },
        ],
        players: vec![
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_red_1"),
                    user_id: String::from("user_1"),
                    invitation: None,
                    name: String::from("Alex Kim"),
                    avatar_url: None,
                }),
                side_id: Some(String::from("side_red")),
                is_member_of_team: Some(true),
            },
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_red_2"),
                    user_id: String::from("user_2"),
                    invitation: None,
                    name: String::from("Jordan Lee"),
                    avatar_url: None,
                }),
                side_id: Some(String::from("side_red")),
                is_member_of_team: Some(true),
            },
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_blue_1"),
                    user_id: String::from("user_3"),
                    invitation: None,
                    name: String::from("Sam Rivera"),
                    avatar_url: None,
                }),
                side_id: Some(String::from("side_blue")),
                is_member_of_team: Some(true),
            },
        ],
        confirmed_score: Some(ConfirmedScore {
            score: Score::Simple(SimpleScore {
                entries: HashMap::from([
                    (String::from("side_red"), 3),
                    (String::from("side_blue"), 1),
                ]),
            }),
            winner_side_id: Some(String::from("side_red")),
        }),
        pending_score: None,
        social: MatchSocial {
            like_count: 3,
            comment_count: 2,
            i_liked: false,
        },
        format: None,
    }
}

/// A fixed timestamp for mock data (Date::now is unavailable in this context
/// and mocks don't need a real clock).
fn mock_timestamp() -> chrono::DateTime<chrono::Utc> {
    "2026-06-01T10:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap()
}

/// Current time as an RFC-3339 / ISO-8601 UTC string (sortable; used in keys).
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Generate a new opaque id (base64url of random bytes).
fn new_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    // NO_PAD: the URL-safe alphabet (`-`/`_`) is fine, but trailing `=` padding is
    // not — these ids are used verbatim as Meilisearch document ids, and Meili
    // rejects `=` (only alphanumerics, `-`, `_` are allowed). See agon_worker
    // search indexing.
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

/// A mock confirmed score submission with one confirm response.
fn mock_score_submission() -> ScoreSubmission {
    ScoreSubmission {
        id: String::from("submission_1"),
        score: Score::Simple(SimpleScore {
            entries: HashMap::from([
                (String::from("side_red"), 3),
                (String::from("side_blue"), 1),
            ]),
        }),
        winner_side_id: Some(String::from("side_red")),
        status: ScoreSubmissionStatus::Confirmed,
        submitted_by_player_id: String::from("player_red_1"),
        submitted_at: mock_timestamp(),
        responses: vec![ScoreSubmissionResponse {
            side_id: String::from("side_blue"),
            responded_by_player_id: String::from("player_blue_1"),
            response: ScoreResponseKind::Confirm,
            responded_at: mock_timestamp(),
        }],
    }
}

/// Builds one pending invitation per invitee in an AddInvitationsInput: a
/// user-invitation per user id, a token-invitation per external name. All are
/// attributed to `invited_by_user_id` (the caller who sent them).
fn mock_invitations_for_input(
    input: &AddInvitationsInput,
    invited_by_user_id: &str,
) -> Vec<Invitation> {
    let mut invitations = Vec::new();
    for (i, user_id) in input.invited_user_ids.iter().enumerate() {
        invitations.push(Invitation {
            id: format!("inv_user_{i}"),
            status: InvitationStatus::Pending,
            invited_by_user_id: invited_by_user_id.to_string(),
            invited_at: mock_timestamp(),
            responded_at: None,
            kind: InvitationKind::User(UserInvitation {
                invited_user_id: user_id.clone(),
            }),
        });
    }
    for (i, _name) in input.invited_external_names.iter().enumerate() {
        invitations.push(Invitation {
            id: format!("inv_external_{i}"),
            status: InvitationStatus::Pending,
            invited_by_user_id: invited_by_user_id.to_string(),
            invited_at: mock_timestamp(),
            responded_at: None,
            kind: InvitationKind::Token(TokenInvitation {
                invite_token: format!("token_{i}"),
            }),
        });
    }
    invitations
}

/// A mock pending user-invitation with the given id.
fn mock_user_invitation(id: String) -> Invitation {
    Invitation {
        id,
        status: InvitationStatus::Pending,
        invited_by_user_id: String::from("user_1"),
        invited_at: mock_timestamp(),
        responded_at: None,
        kind: InvitationKind::User(UserInvitation {
            invited_user_id: String::from("user_2"),
        }),
    }
}

/// A mock standalone invitation with its context, for the inbox / fetch-by-id.
fn mock_invitation_detail() -> InvitationDetail {
    InvitationDetail {
        invitation: mock_user_invitation(String::from("inv_user_0")),
        context: InvitationContext::Match(InvitationMatchContext {
            match_id: String::from("match_123"),
            match_name: String::from("Sunday League 5-a-side"),
        }),
    }
}

/// Builds a mock notification of each kind.
fn mock_notifications() -> Vec<Notification> {
    let actor = |id: &str, name: &str| mock_user_profile(String::from(id), String::from(name));
    vec![
        Notification {
            id: String::from("notif_1"),
            is_read: false,
            created_at: mock_timestamp(),
            kind: NotificationKind::MatchInvitation(MatchInvitationNotification {
                inviter: actor("user_2", "Raj Patel"),
                invitation_id: String::from("inv_abc"),
                match_id: String::from("match_123"),
                match_name: String::from("Tennis vs Raj"),
            }),
        },
        Notification {
            id: String::from("notif_team_inv"),
            is_read: false,
            created_at: mock_timestamp(),
            kind: NotificationKind::TeamInvitation(TeamInvitationNotification {
                inviter: actor("user_5", "Tom Brennan"),
                invitation_id: String::from("inv_team_xyz"),
                team_id: String::from("team_kent"),
                team_name: String::from("Kent"),
            }),
        },
        Notification {
            id: String::from("notif_accepted"),
            is_read: false,
            created_at: mock_timestamp(),
            kind: NotificationKind::InvitationAccepted(InvitationAcceptedNotification {
                accepted_by: actor("user_3", "Alex Morgan"),
                invitation_id: String::from("inv_abc"),
                context: InvitationContext::Match(InvitationMatchContext {
                    match_id: String::from("match_123"),
                    match_name: String::from("Tennis vs Raj"),
                }),
            }),
        },
        Notification {
            id: String::from("notif_2"),
            is_read: false,
            created_at: mock_timestamp(),
            kind: NotificationKind::Follow(FollowNotification {
                follower: actor("user_1", "Sofia Lindqvist"),
            }),
        },
        Notification {
            id: String::from("notif_3"),
            is_read: true,
            created_at: mock_timestamp(),
            kind: NotificationKind::Like(LikeNotification {
                liked_by: actor("user_3", "Alex Morgan"),
                match_id: String::from("match_123"),
                match_name: String::from("Tennis vs Raj"),
            }),
        },
        Notification {
            id: String::from("notif_4"),
            is_read: true,
            created_at: mock_timestamp(),
            kind: NotificationKind::Comment(CommentNotification {
                commenter: actor("user_4", "Priya Shah"),
                match_id: String::from("match_123"),
                comment_id: String::from("comment_9"),
                preview: String::from("Good game, rematch soon?"),
            }),
        },
        Notification {
            id: String::from("notif_reply"),
            is_read: true,
            created_at: mock_timestamp(),
            kind: NotificationKind::Reply(ReplyNotification {
                replier: actor("user_2", "Raj Patel"),
                match_id: String::from("match_123"),
                comment_id: String::from("comment_10"),
                parent_comment_id: String::from("comment_9"),
                preview: String::from("Definitely, next week?"),
            }),
        },
        Notification {
            id: String::from("notif_score_submitted"),
            is_read: false,
            created_at: mock_timestamp(),
            kind: NotificationKind::ScoreSubmitted(ScoreSubmittedNotification {
                submitted_by: actor("user_2", "Raj Patel"),
                match_id: String::from("match_123"),
                match_name: String::from("Tennis vs Raj"),
                submission_id: String::from("sub_abc"),
                needs_confirmation: true,
            }),
        },
        Notification {
            id: String::from("notif_score_confirmed"),
            is_read: true,
            created_at: mock_timestamp(),
            kind: NotificationKind::ScoreConfirmed(ScoreConfirmedNotification {
                confirmed_by: actor("user_3", "Alex Morgan"),
                match_id: String::from("match_123"),
                match_name: String::from("Tennis vs Raj"),
                submission_id: String::from("sub_abc"),
            }),
        },
    ]
}

/// Builds a mock lightweight team list item.
fn mock_team_list_item(id: String, name: String) -> TeamListItem {
    TeamListItem {
        id,
        name,
        logo: None,
        follower_count: 128,
        is_followed_by_me: false,
    }
}

/// Builds a mock team. Shared by the team endpoints until a real DAO is wired in.
fn mock_team(id: String, name: String) -> Team {
    Team {
        id,
        name,
        logo: None,
        invite_token: Some(String::from("team_invite_abc123")),
        follower_count: 128,
        is_followed_by_me: false,
    }
}

/// Builds a mock page of team members. Shared by the team endpoints until a
/// real DAO is wired in.
fn mock_team_member_page() -> TeamMemberPage {
    let invited_at = mock_timestamp();

    TeamMemberPage {
        items: vec![
            // An accepted Agon user (the team admin).
            TeamMember {
                member: Member::User(UserMember {
                    id: String::from("membership_1"),
                    user_id: String::from("user_1"),
                    invitation: Some(Invitation {
                        id: String::from("team_inv_1"),
                        status: InvitationStatus::Accepted,
                        invited_by_user_id: String::from("user_1"),
                        invited_at,
                        responded_at: Some(invited_at),
                        kind: InvitationKind::User(UserInvitation {
                            invited_user_id: String::from("user_1"),
                        }),
                    }),
                    name: String::from("Alex Kim"),
                    avatar_url: None,
                }),
                role: TeamRole::Admin,
            },
            // An external invitee who has not yet accepted / linked an account.
            TeamMember {
                member: Member::External(membership::ExternalMember {
                    id: String::from("membership_2"),
                    display_name: String::from("Dave (ringer)"),
                    invitation: Some(Invitation {
                        id: String::from("team_inv_2"),
                        status: InvitationStatus::Pending,
                        invited_by_user_id: String::from("user_1"),
                        invited_at,
                        responded_at: None,
                        kind: InvitationKind::Token(TokenInvitation {
                            invite_token: String::from("team_invite_abc123"),
                        }),
                    }),
                }),
                role: TeamRole::Member,
            },
        ],
        next_cursor: None,
    }
}

/// Whether an ISO-8601 `starts_at` falls within an optional `[from, to]` range
/// (inclusive). A missing bound is unbounded on that side; an unparseable
/// timestamp is treated as in-range (don't silently drop a real feed item over
/// a formatting quirk).
fn within_range(
    starts_at: &str,
    from: Option<chrono::DateTime<chrono::Utc>>,
    to: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    if from.is_none() && to.is_none() {
        return true;
    }
    let Ok(ts) = starts_at.parse::<chrono::DateTime<chrono::Utc>>() else {
        return true;
    };
    if let Some(from) = from
        && ts < from
    {
        return false;
    }
    if let Some(to) = to
        && ts > to
    {
        return false;
    }
    true
}

/// Default page size when the client does not specify a limit.
const DEFAULT_PAGE_LIMIT: u32 = 20;
/// Hard cap so a client cannot request an unbounded page.
const MAX_PAGE_LIMIT: u32 = 50;
/// Hard cap on a feed page, tighter than `MAX_PAGE_LIMIT`: at
/// `agon_core::dao::audience::MAX_KNOWN_PLAYERS` (5) known participants per
/// match, 20 matches is exactly `BATCH_GET_MAX` (100) — the largest page that
/// still hydrates `known_participants` in a single `BatchGetItem`, regardless
/// of how full every match's list happens to be.
const FEED_MAX_PAGE_LIMIT: u32 = 20;

/// Maximum size of an uploaded asset, in bytes (10 MB). Enforced at asset
/// creation and baked into the presigned PUT so S3 rejects a mismatch too.
const MAX_UPLOAD_BYTES: i64 = 10 * 1024 * 1024;

/// Clamps a client-supplied limit to `[_, MAX_PAGE_LIMIT]`, defaulting when absent.
fn page_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT)
}

/// Clamps a client-supplied feed-page limit to `[_, FEED_MAX_PAGE_LIMIT]`.
fn feed_page_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(FEED_MAX_PAGE_LIMIT)
}

/// Decode a search-endpoint cursor into a zero-based offset. Search pagination
/// is offset-based (Meilisearch), so unlike the DynamoDB `LastEvaluatedKey`
/// cursors elsewhere, the cursor is simply the stringified next offset. An
/// absent cursor means the first page; a malformed one is an error the caller
/// surfaces as a 400.
fn search_offset(cursor: Option<&str>) -> Result<u32, ()> {
    match cursor {
        Some(raw) => raw.parse::<u32>().map_err(|_| ()),
        None => Ok(0),
    }
}

/// Turn a search index's `next_offset` into the opaque cursor string returned to
/// clients (`None` => no more pages).
fn search_cursor(next_offset: Option<u32>) -> Option<String> {
    next_offset.map(|o| o.to_string())
}

/// Map an `agon_core` search error to a 500 (an index outage is our problem, not
/// the client's).
fn search_internal(err: agon_core::error::SearchError) -> poem::Error {
    error!("search error: {err}");
    poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "git")]
#[command(about = "Agon Service CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Starts the service
    #[command(arg_required_else_help = true)]
    RunServer {
        /// The url of the service
        url: String,
    },

    /// Generates service open api schema
    GenerateSchema,

    /// Generates a signed JWT for local testing. Signs ES256 with the test key
    /// in AGON_TEST_JWT_PRIVATE_KEY (kid from AGON_TEST_JWT_KID, default
    /// `agon-test`); the service must trust the matching public JWK via
    /// AGON_STATIC_JWKS.
    GenerateToken {
        /// The subject (user id) to embed in the `sub` claim
        #[arg(default_value = "test-user")]
        sub: String,
        /// The email to embed in the `email` claim (used by signup, which reads
        /// the email from the token rather than the request body).
        #[arg(long)]
        email: Option<String>,
    },
}

fn log_request(uri: &Uri, status: StatusCode) {
    info!(
        path = uri.path(),
        status = status.as_u16(),
        "Request complete"
    );
}

/// Request metrics recorded on the global OTel meter (exported via OTLP; see
/// agon_core::telemetry). Built once and reused so the instruments are stable.
///
/// Metric attributes are deliberately restricted to `http.request.method` and
/// `http.response.status_code` — low, bounded cardinality. The concrete request
/// path (which embeds IDs) is attached to the span instead, not the metrics, so
/// we don't blow up the Prometheus series count.
struct RequestMetrics {
    /// Total requests handled, by method + status.
    count: opentelemetry::metrics::Counter<u64>,
    /// Request duration in seconds, by method + status.
    duration: opentelemetry::metrics::Histogram<f64>,
}

impl RequestMetrics {
    fn global() -> &'static RequestMetrics {
        use std::sync::OnceLock;
        static METRICS: OnceLock<RequestMetrics> = OnceLock::new();
        METRICS.get_or_init(|| {
            let meter = opentelemetry::global::meter("agon-service");
            RequestMetrics {
                count: meter
                    .u64_counter("http.server.request.count")
                    .with_description("Total HTTP requests handled")
                    .build(),
                duration: meter
                    .f64_histogram("http.server.request.duration")
                    .with_description("HTTP request duration")
                    .with_unit("s")
                    .build(),
            }
        })
    }
}

async fn log_middleware<E: Endpoint>(next: E, req: Request) -> Result<Response> {
    use opentelemetry::KeyValue;
    use std::time::Instant;

    let uri = req.uri().clone();
    let method = req.method().to_string();
    let start = Instant::now();

    let (status, result) = match next.call(req).await {
        Ok(resp) => {
            let resp = resp.into_response();
            (resp.status(), Ok(resp))
        }
        Err(err) => (err.status(), Err(err)),
    };

    log_request(&uri, status);

    // Record request count + latency on the global meter. Attributes are kept to
    // bounded-cardinality dimensions only (see RequestMetrics).
    let attrs = [
        KeyValue::new("http.request.method", method),
        KeyValue::new("http.response.status_code", status.as_u16() as i64),
    ];
    let metrics = RequestMetrics::global();
    metrics.count.add(1, &attrs);
    metrics
        .duration
        .record(start.elapsed().as_secs_f64(), &attrs);

    result
}

#[tokio::main]
async fn main() {
    // Held for the process lifetime; dropping it flushes the OTLP batch
    // exporters on shutdown. See agon_core::telemetry.
    let _telemetry = agon_core::telemetry::init("agon-service");

    let args = Cli::parse();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:7000");

    match args.command {
        Commands::RunServer { url: _ } => {
            info!("Starting up server");

            let ui = api_service.scalar();

            let table = std::env::var("AGON_TABLE_NAME").unwrap_or_else(|_| "agon".to_string());
            let dao = dao::Dao::from_env(table).await;

            // Object storage: presigned S3 uploads + CloudFront serving URLs.
            let assets = Assets::from_env().await;

            // Meilisearch client for discovery endpoints (users/teams/matches
            // search). Indexes are kept in sync by the async worker; the API only
            // queries them and hydrates results from DynamoDB.
            let meili_url =
                std::env::var("MEILI_URL").unwrap_or_else(|_| "http://localhost:7700".to_string());
            let meili_key = std::env::var("MEILI_MASTER_KEY").unwrap_or_default();
            let search = agon_core::search::SearchClient::new(meili_url, meili_key);

            // Explicit origin allowlist — NOT `*`. A wildcard origin is invalid
            // combined with `allow_credentials(true)` (the CORS spec forbids it and
            // browsers reject the preflight), and mixing `*` with specific origins
            // in poem's Cors doesn't act as a catch-all anyway. Local dev is the
            // Vite server, pinned to 5173; deployed origins match the get-agon.com
            // regex.
            let cors = Cors::new()
                .allow_origin("http://localhost:5173")
                .allow_origin_regex("https://*.get-agon.com")
                .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allow_headers(vec!["content-type", "authorization"])
                .allow_credentials(true);

            // JWT verifier: trusts the Supabase JWKS (real users) and/or the
            // static test key (integration tests / local). Built once, shared.
            let verifier = auth::JwtVerifier::from_env();

            let app = Route::new()
                .nest("/", api_service)
                .nest("/docs", ui)
                .with(cors)
                .data(dao)
                .data(search)
                .data(verifier)
                .data(assets)
                .around(log_middleware)
                // Outermost: opens a tracing span per request, so it covers
                // log_middleware's logging/metrics and everything downstream.
                // Without it, `agon_core::telemetry`'s OTLP tracer never has a
                // span to export — requests were logged and measured but no
                // trace ever left the process.
                .with(Tracing);

            Server::new(TcpListener::bind("0.0.0.0:7000"))
                .run(app)
                .await
                .expect("Failed to start server");
        }

        Commands::GenerateSchema => {
            let mut file = File::create("schema.json").expect("Cannot create schema/schmea.json");
            file.write_all(api_service.spec().as_bytes())
                .expect("Failed to write to file");
        }

        Commands::GenerateToken { sub, email } => {
            // Sign with the ES256 test private key (PEM in AGON_TEST_JWT_PRIVATE_KEY).
            // The `kid` must match the public JWK the service trusts via
            // AGON_STATIC_JWKS. This is the test/local counterpart to Supabase's
            // asymmetric signing — there is no shared-secret path.
            let private_key_pem = std::env::var("AGON_TEST_JWT_PRIVATE_KEY")
                .expect("AGON_TEST_JWT_PRIVATE_KEY not set");
            let kid =
                std::env::var("AGON_TEST_JWT_KID").unwrap_or_else(|_| "agon-test".to_string());

            let audience =
                std::env::var("AGON_JWT_AUDIENCE").unwrap_or_else(|_| "authenticated".to_string());
            let claims = JwtClaims {
                sub,
                // far-future expiry so local/dev tokens don't need refreshing.
                exp: 9_999_999_999,
                iss: None,
                aud: Some(audience),
                role: None,
                email,
            };

            let mut header = Header::new(Algorithm::ES256);
            header.kid = Some(kid);

            let token = encode(
                &header,
                &claims,
                &EncodingKey::from_ec_pem(private_key_pem.as_bytes())
                    .expect("AGON_TEST_JWT_PRIVATE_KEY is not a valid EC PEM"),
            )
            .expect("Failed to encode JWT");

            println!("{token}");
        }
    }
}
