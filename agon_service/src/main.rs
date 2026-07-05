// The service is currently mock-backed: error response variants and the cricket
// score aggregator are part of the intended API surface but aren't constructed
// until the real DAO is wired in, so dead_code is expected for now. The enum/arg
// size lints are not worth restructuring generated-API response types over.
#![allow(dead_code)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::result_large_err)]
#![allow(clippy::too_many_arguments)]

use std::{fs::File, io::Write};

use base64::{Engine, prelude::BASE64_URL_SAFE};
use clap::{Parser, Subcommand};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use poem::http::Uri;
use poem::{Endpoint, IntoResponse, Response};
use poem::{
    EndpointExt, Error, Request, Result, Route, Server, http::StatusCode, listener::TcpListener,
    middleware::Cors, web::Data,
};
use poem_openapi::auth::Bearer;
use poem_openapi::param::Query;
use poem_openapi::{
    ApiResponse, Enum, Object, OpenApi, OpenApiService, SecurityScheme, Union,
    param::Path,
    payload::{Json, PlainText},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

// Data access layer for DynamoDB — now the shared `agon_core` crate. Aliased as
// `dao` so existing `dao::…` paths in handlers keep working.
use agon_core::dao;
// Boundary mapping between API models and DAO records.
mod mapping;
use mapping::{
    comment_from_record, dao_internal, detailed_score_from_record, detailed_score_to_record,
    invitation_detail_from_record, invitation_from_record, invitation_status_from_str,
    invitation_status_str, match_from_records, match_status_str, match_type_tag,
    notification_actor_id, notification_from_record, score_submission_from_record, score_to_record,
    team_from_records, team_list_item_from_record, user_profile_from_record,
};

mod detailed_score;
use detailed_score::{
    DetailedScore,
    football::{FootballDetail, FootballEvent, FootballEventKind},
};

mod membership;
use membership::{
    AddInvitationsInput, Invitation, InvitationContext, InvitationDetail, InvitationKind,
    InvitationMatchContext, InvitationStatus, Member, RespondByTokenInput,
    RespondToInvitationInput, TokenInvitation, UserInvitation, UserMember,
};

mod team;
use team::{
    AddTeamMembersInput, CreateTeamInput, Team, TeamListItem, TeamMember, TeamRole, UpdateTeamInput,
};

mod notification;
use notification::{
    CommentNotification, FollowNotification, InvitationAcceptedNotification, LikeNotification,
    MatchInvitationNotification, Notification, NotificationKind, NotificationPage,
    TeamInvitationNotification, UnreadCount,
};

#[derive(Debug, Deserialize, Serialize)]
struct JwtClaims {
    sub: String,
    exp: usize,
    iss: Option<String>,
    aud: Option<String>,
    role: Option<String>,
}

#[derive(SecurityScheme)]
#[oai(
    ty = "bearer",
    key_name = "authorization",
    key_in = "header",
    checker = "jwt_checker"
)]
struct AuthSchema(JwtClaims);

async fn jwt_checker(_req: &Request, bearer: Bearer) -> Result<JwtClaims, poem::error::Error> {
    info!("Attempting to validate JWT token");
    info!(
        "Token prefix: {}",
        &bearer.token[..std::cmp::min(20, bearer.token.len())]
    );

    // Change to change the validity of the token (set to false to fail the validation)
    let secret_key = std::env::var("JWT_SECRET").expect("JWT Secret not found");
    let decoding_key = DecodingKey::from_secret(secret_key.as_bytes());

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.validate_nbf = false;

    let token_data =
        decode::<JwtClaims>(&bearer.token, &decoding_key, &validation).map_err(|err| {
            info!("JWT invalid {:?}", err);
            Error::from_string("Invalid JWT", StatusCode::UNAUTHORIZED)
        })?;

    Ok(token_data.claims)
}

struct Api;

#[derive(Object)]
pub struct UserSportStats {
    pub match_type: MatchType,
    pub matches_played: i32,
    pub win_percentage: f32,
    // TODO Elo
}

#[derive(Object)]
pub struct UserProfile {
    pub id: String,
    pub name: String,
    /// Profile image. Uploaded by the client directly to object storage
    /// (Supabase Storage); the API only stores/returns the resulting URL.
    pub profile_image: Option<Photo>,
    pub stats: Vec<UserSportStats>,
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
    email: String,
    name: String,
}

/// Editable fields on the current user's own profile. All optional — only
/// supplied fields change. `profile_image_asset_id` references an `Asset` the
/// client created via `POST /assets`; the server rejects it unless the asset
/// is `Uploaded`, and resolves it to the stored image URL. None leaves the image
/// unchanged.
#[derive(Object)]
struct UpdateUserInput {
    email: Option<String>,
    name: Option<String>,
    profile_image_asset_id: Option<String>,
}

#[derive(Object)]
pub struct Photo {
    pub image_url: String,
}

/// What an upload is for. Drives the storage bucket/path and the size/type
/// constraints the server applies when issuing the presigned URL.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum UploadPurpose {
    ProfileImage,
    TeamImage,
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
    /// Display name — usually the team name, but works for ad-hoc sides too.
    name: Option<String>,
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
/// (e.g. cricket, golf) without breaking existing clients.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
enum Score {
    /// Single number per side: football, basketball, rugby.
    Simple(SimpleScore),
    /// Set-based: tennis, volleyball, badminton.
    Sets(SetsScore),
}

#[derive(Object)]
struct SimpleScore {
    entries: Vec<SimpleScoreEntry>,
}

#[derive(Object)]
struct SimpleScoreEntry {
    side_id: String,
    points: u32,
}

#[derive(Object)]
struct SetsScore {
    entries: Vec<SetsScoreEntry>,
}

#[derive(Object)]
struct SetsScoreEntry {
    side_id: String,
    /// Games won per set, e.g. [6, 4, 7]. Index-aligned across entries so the
    /// same index is the same set for every side.
    sets: Vec<u32>,
}

/// The sport a match was played in. Determines the expected `Score`/
/// `DetailedScore` shape (e.g. racket sports use `Score::Sets`, football/cricket
/// use `Score::Simple`). Extend as more sports are supported.
#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum MatchType {
    Tennis,
    Badminton,
    Squash,
    TableTennis,
    Football,
    Cricket,
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

/// A side to create as part of a new match. The server assigns the real side id;
/// `client_id` lets the request reference this side from `invites` and `score`.
#[derive(Object)]
struct CreateMatchSideInput {
    /// Caller-chosen temporary id, unique within the request, used to wire up
    /// invites and score entries to this side before real ids exist.
    client_id: String,
    team_id: Option<String>,
    name: Option<String>,
}

/// An invitation to create with the match. `side_client_id` references a
/// `CreateMatchSideInput.client_id` (None = invite without assigning a side).
#[derive(Object)]
struct CreateMatchInviteInput {
    side_client_id: Option<String>,
    invited_user_ids: Vec<String>,
    invited_external_names: Vec<String>,
}

#[derive(Object)]
struct CreateMatchInput {
    name: String,
    description: String,
    match_type: MatchType,
    /// When the match starts / started.
    starts_at: chrono::DateTime<chrono::Utc>,
    location: Option<Location>,
    /// The opposing sides. At least two are required.
    sides: Vec<CreateMatchSideInput>,
    /// Players to invite up front. Optional — more can be added later.
    invites: Vec<CreateMatchInviteInput>,
    /// If present, the match is created already played (status `Completed`) and
    /// the score enters the confirmation flow. `side_id`s reference the created
    /// sides' `client_id`s. Absent => an upcoming match.
    score: Option<Score>,
    winner_side_id: Option<String>,
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
    /// The result. Creates a score submission when changed; for a not-yet-played
    /// match this also completes it. `side_id`s reference the match's sides.
    score: Option<Score>,
    winner_side_id: Option<String>,
    /// Optional sport-specific breakdown (goals/assists, full scorecard),
    /// captured alongside the result.
    detailed_score: Option<DetailedScore>,
}

/// A single entry in the feed. Modelled as a union so new item types
/// (member joined, achievements, etc.) can be added without breaking clients.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
enum FeedItem {
    Match(Match),
}

/// One page of the feed. `next_cursor` is an opaque token; when it is
/// absent/null the client has reached the end of the feed.
#[derive(Object)]
struct FeedPage {
    items: Vec<FeedItem>,
    next_cursor: Option<String>,
}

/// One page of matches from discovery/search. `next_cursor` absent => end.
#[derive(Object)]
struct MatchPage {
    items: Vec<Match>,
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

    #[oai(status = 404)]
    NotFound(PlainText<String>),
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

#[derive(ApiResponse)]
enum GetMatchDetailedScoreResponse {
    #[oai(status = 200)]
    DetailedScore(Json<DetailedScore>),

    /// The match exists but has no detailed score recorded.
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

    #[oai(path = "/users/me", method = "get")]
    async fn get_current_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<GetUserResponse> {
        info!("Getting current user");
        let record = match dao.get_user(&jwt_data.sub).await.map_err(dao_internal)? {
            Some(r) => r,
            None => {
                return Ok(GetUserResponse::NotFound(PlainText(
                    "user not found".into(),
                )));
            }
        };
        let stats = dao
            .list_user_stats(&jwt_data.sub)
            .await
            .map_err(dao_internal)?;
        // Own profile: not "followed by me".
        let profile = user_profile_from_record(&record, &stats, false);
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

        // Resolve an attached asset id to its stored URL (must be Uploaded and
        // owned by the caller). Some(Some(url)) = set image; None = leave as-is.
        let resolved_image: Option<Option<String>> = match &input.profile_image_asset_id {
            Some(asset_id) => {
                let asset = dao.get_asset(asset_id).await.map_err(dao_internal)?;
                match asset {
                    Some(a) if a.status == "uploaded" && a.owner_user_id == jwt_data.sub => {
                        Some(a.url)
                    }
                    _ => {
                        return Ok(UpdateUserResponse::ValidationError(PlainText(
                            "asset not found, not uploaded, or not owned by you".into(),
                        )));
                    }
                }
            }
            None => None,
        };

        dao.update_user_profile(
            &jwt_data.sub,
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
            .get_user(&jwt_data.sub)
            .await
            .map_err(dao_internal)?
            .ok_or_else(|| Error::from_string("user not found", StatusCode::NOT_FOUND))?;
        let stats = dao
            .list_user_stats(&jwt_data.sub)
            .await
            .map_err(dao_internal)?;
        let profile = user_profile_from_record(&record, &stats, false);
        Ok(UpdateUserResponse::User(Json(User {
            email: record.email,
            profile,
        })))
    }

    #[oai(path = "/assets", method = "post")]
    async fn create_asset(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateAssetInput>,
    ) -> Result<CreateAssetResponse> {
        info!("Creating asset for content type {}", input.content_type);
        let input = input.0;

        // Validate the content type against the purpose (images only for now).
        if !input.content_type.starts_with("image/") {
            return Ok(CreateAssetResponse::ValidationError(PlainText(
                "content_type not allowed for this purpose".into(),
            )));
        }

        let id = new_id();
        // Provider-agnostic object key. A storage event later flips the asset to
        // Uploaded and sets the URL — none of the provider details leak here.
        let storage_key = format!("{}/{}", upload_purpose_str(&input.purpose), id);
        let record = dao::records::AssetRecord {
            id: id.clone(),
            owner_user_id: jwt_data.sub,
            purpose: upload_purpose_str(&input.purpose).to_string(),
            content_type: input.content_type,
            status: String::from("pending"),
            storage_key,
            url: None,
            created_at: now_iso(),
        };
        dao.create_asset(&record).await.map_err(dao_internal)?;
        Ok(CreateAssetResponse::Asset(Json(asset_from_record(&record))))
    }

    #[oai(path = "/assets/:asset_id", method = "get")]
    async fn get_asset(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(asset_id): Path<String>,
    ) -> Result<GetAssetResponse> {
        info!("Getting asset {asset_id}");
        match dao.get_asset(&asset_id).await.map_err(dao_internal)? {
            // Pending assets get a fresh presigned upload target on each read
            // (the previous one may have expired) — that is the retry mechanism.
            Some(record) => Ok(GetAssetResponse::Asset(Json(asset_from_record(&record)))),
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
        let stats = dao.list_user_stats(&user_id).await.map_err(dao_internal)?;
        let is_followed = if jwt_data.sub == user_id {
            false
        } else {
            dao.is_following_user(&jwt_data.sub, &user_id)
                .await
                .map_err(dao_internal)?
        };
        Ok(GetUserProfileResponse::User(Json(
            user_profile_from_record(&record, &stats, is_followed),
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
        // The user's id is their JWT subject (Supabase user id).
        let record = dao::records::UserRecord {
            id: jwt_data.sub.clone(),
            email: input.email,
            name: input.name,
            profile_image_url: None,
            follower_count: 0,
            following_count: 0,
            unread_count: 0,
            created_at: now_iso(),
        };
        match dao.create_user(&record).await {
            Ok(()) => {}
            Err(dao::DaoError::Conflict(_)) => {
                return Ok(CreateUserResponse::ValidationError(PlainText(
                    "a user with that email or id already exists".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        }
        let profile = user_profile_from_record(&record, &[], false);
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
        AuthSchema(_jwt_data): AuthSchema,
        #[oai(name = "q")] Query(query): Query<String>,
    ) -> Result<SearchUsersResponse> {
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

        let profiles = self.hydrate_user_profiles(dao, &hits.ids).await?;
        Ok(SearchUsersResponse::Users(Json(profiles)))
    }

    #[oai(path = "/feed", method = "get")]
    async fn get_user_feed(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
        /// Only include items at or after this time (inclusive).
        Query(from): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Only include items at or before this time (inclusive).
        Query(to): Query<Option<chrono::DateTime<chrono::Utc>>>,
    ) -> Result<GetFeedResponse> {
        info!("Getting caller's social feed");

        // The feed is always the authenticated caller's own social feed (matches
        // from people/teams they follow). No user_id / sport filtering here —
        // that is the match-discovery endpoint (GET /matches), served by search.

        let limit = page_limit(limit);

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
        let page = match dao.list_feed(&jwt_data.sub, cursor.as_deref(), limit).await {
            Ok(p) => p,
            Err(dao::DaoError::Malformed(_)) => {
                return Ok(GetFeedResponse::ValidationError(PlainText(
                    "Invalid cursor".to_string(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        };

        let mut items = Vec::with_capacity(page.items.len());
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
            if let Some(agg) = dao.get_match(&entry.ref_id).await.map_err(dao_internal)? {
                let i_liked = dao
                    .has_liked_match(&entry.ref_id, &jwt_data.sub)
                    .await
                    .map_err(dao_internal)?;
                items.push(FeedItem::Match(match_from_records(
                    &agg.match_,
                    &agg.sides,
                    &agg.players,
                    i_liked,
                )));
            }
        }

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
        AuthSchema(_jwt_data): AuthSchema,
        /// Free-text query over match name / participants.
        #[oai(name = "q")]
        Query(query): Query<Option<String>>,
        /// Only matches this user played in (member id or user id).
        Query(participant): Query<Option<String>>,
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

        // Match discovery is served by the search index (Meilisearch), NOT
        // DynamoDB — it supports arbitrary combinations of text / participant /
        // sport / date-range with date sorting. This is distinct from GET /feed
        // (the caller's social feed). Powers the profile "recent activity" view
        // (participant = the profile's user) and general match search.
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

        // Build the Meilisearch filter from the supplied facets. `starts_at` is
        // stored as an ISO-8601 string, which compares correctly lexically, so
        // the date range uses plain string comparisons.
        let mut clauses: Vec<String> = Vec::new();
        if let Some(mt) = &match_type {
            clauses.push(format!("sport = \"{}\"", match_type_tag(mt)));
        }
        if let Some(p) = &participant {
            clauses.push(format!("participant_ids = \"{p}\""));
        }
        if let Some(from) = from {
            clauses.push(format!("starts_at >= \"{}\"", from.to_rfc3339()));
        }
        if let Some(to) = to {
            clauses.push(format!("starts_at <= \"{}\"", to.to_rfc3339()));
        }
        let filter = (!clauses.is_empty()).then(|| clauses.join(" AND "));

        let q = agon_core::search::SearchQuery {
            q: query.unwrap_or_default(),
            filter,
            sort: vec!["starts_at:desc".to_string()],
            offset,
            limit: page_limit(limit),
        };
        let hits = search
            .search(agon_core::search::Index::Matches, &q)
            .await
            .map_err(search_internal)?;

        // Hydrate each match from DynamoDB (the index carries only filter/sort
        // fields). `i_liked` needs the caller's like state; resolve per match.
        // TODO: BatchGet + batch the like checks (N+1 for now).
        let mut items = Vec::with_capacity(hits.ids.len());
        for id in &hits.ids {
            if let Some(agg) = dao.get_match(id).await.map_err(dao_internal)? {
                let i_liked = dao
                    .has_liked_match(id, &_jwt_data.sub)
                    .await
                    .map_err(dao_internal)?;
                items.push(match_from_records(
                    &agg.match_,
                    &agg.sides,
                    &agg.players,
                    i_liked,
                ));
            }
        }
        Ok(ListMatchesResponse::Matches(Json(MatchPage {
            items,
            next_cursor: search_cursor(hits.next_offset),
        })))
    }

    #[oai(path = "/matches", method = "post")]
    async fn create_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateMatchInput>,
    ) -> Result<CreateMatchResponse> {
        info!("Creating match {}", input.name);
        let input = input.0;

        // A match needs at least two sides for a score to be meaningful.
        if input.sides.len() < 2 {
            return Ok(CreateMatchResponse::ValidationError(PlainText(
                "a match must have at least two sides".to_string(),
            )));
        }

        let now = now_iso();
        let match_id = new_id();

        // Assign a real side id per input side and remember the client_id -> id
        // mapping so invites and the score can be re-pointed at real ids.
        let mut side_ids: std::collections::HashMap<String, String> = Default::default();
        let mut side_records: Vec<dao::records::MatchSideRecord> = Vec::new();
        for side in &input.sides {
            let side_id = new_id();
            side_ids.insert(side.client_id.clone(), side_id.clone());
            side_records.push(dao::records::MatchSideRecord {
                side_id,
                team_id: side.team_id.clone(),
                name: side.name.clone(),
            });
        }

        // Resolve a score's client-side ids to the real side ids. Reject a score
        // referencing an unknown side.
        let confirmed_score = match &input.score {
            Some(score) => match resolve_score_side_ids(score, &side_ids) {
                Some(resolved) => Some(dao::records::ConfirmedScoreRecord {
                    score: score_to_record(&resolved),
                    winner_side_id: input
                        .winner_side_id
                        .as_ref()
                        .and_then(|c| side_ids.get(c).cloned()),
                }),
                None => {
                    return Ok(CreateMatchResponse::ValidationError(PlainText(
                        "score references an unknown side".into(),
                    )));
                }
            },
            None => None,
        };

        // A supplied score means the match is already played (Completed);
        // otherwise it is Scheduled.
        let status = if confirmed_score.is_some() {
            "completed"
        } else {
            "scheduled"
        };

        // Build a player + invitation per invitee. Externals get a minted token
        // and a standalone invitation record; users get a user-kind invitation.
        let mut player_records: Vec<dao::records::MatchPlayerRecord> = Vec::new();
        let mut invitation_records: Vec<dao::records::InvitationRecord> = Vec::new();
        for invite in &input.invites {
            let side_id = invite
                .side_client_id
                .as_ref()
                .and_then(|c| side_ids.get(c).cloned());
            for user_id in &invite.invited_user_ids {
                let (player, inv) = build_invited_player(
                    &match_id,
                    &input.name,
                    &jwt_data.sub,
                    side_id.clone(),
                    Some(user_id.clone()),
                    None,
                    &now,
                );
                player_records.push(player);
                invitation_records.push(inv);
            }
            for name in &invite.invited_external_names {
                let (player, inv) = build_invited_player(
                    &match_id,
                    &input.name,
                    &jwt_data.sub,
                    side_id.clone(),
                    None,
                    Some(name.clone()),
                    &now,
                );
                player_records.push(player);
                invitation_records.push(inv);
            }
        }

        let match_record = dao::records::MatchRecord {
            id: match_id.clone(),
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
            header_photo_urls: Vec::new(),
            confirmed_score,
            pending_score: None,
            like_count: 0,
            comment_count: 0,
            created_at: now.clone(),
        };

        match dao
            .create_match(&match_record, &side_records, &player_records)
            .await
        {
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

        Ok(CreateMatchResponse::Match(Json(match_from_records(
            &match_record,
            &side_records,
            &player_records,
            false,
        ))))
    }

    #[oai(path = "/matches/:match_id", method = "get")]
    async fn get_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<GetMatchResponse> {
        info!("Getting match {match_id}");
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(GetMatchResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        let i_liked = dao
            .has_liked_match(&match_id, &jwt_data.sub)
            .await
            .map_err(dao_internal)?;
        Ok(GetMatchResponse::Match(Json(match_from_records(
            &agg.match_,
            &agg.sides,
            &agg.players,
            i_liked,
        ))))
    }

    #[oai(path = "/matches/:match_id", method = "patch")]
    async fn update_match(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(match_id): Path<String>,
        input: Json<UpdateMatchInput>,
    ) -> Result<UpdateMatchResponse> {
        info!("Updating match {match_id}");
        let input = input.0;

        // Load the current state (404 if missing).
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(UpdateMatchResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
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

        // A supplied score creates a new submission only when it differs from
        // the current confirmed score. For a not-yet-played match, a new score
        // also completes it.
        let mut confirmed_score: Option<dao::records::ConfirmedScoreRecord> = None;
        let mut status_override: Option<&str> = input.status.as_ref().map(match_status_str);

        if let Some(score) = &input.score {
            // Validate every scored side exists on the match.
            let score_sides: Vec<&str> = match score {
                Score::Simple(s) => s.entries.iter().map(|e| e.side_id.as_str()).collect(),
                Score::Sets(s) => s.entries.iter().map(|e| e.side_id.as_str()).collect(),
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
                // Record a score submission (history) attributed to the caller.
                let submitted_at = now_iso();
                let submission = dao::records::ScoreSubmissionRecord {
                    submission_id: new_id(),
                    score: new_record.clone(),
                    winner_side_id: input.winner_side_id.clone(),
                    status: String::from("confirmed"),
                    submitted_by_player_id: jwt_data.sub.clone(),
                    submitted_at,
                    responses: Vec::new(),
                };
                dao.put_score_submission(&match_id, &submission)
                    .await
                    .map_err(dao_internal)?;

                confirmed_score = Some(dao::records::ConfirmedScoreRecord {
                    score: new_record,
                    winner_side_id: input.winner_side_id.clone(),
                });

                // A not-yet-played match becomes Completed when first scored.
                if status_override.is_none() && agg.match_.status != "completed" {
                    status_override = Some("completed");
                }
            }
        }

        // Persist a supplied detailed score.
        if let Some(ds) = &input.detailed_score {
            let record = detailed_score_to_record(ds);
            dao.put_match_detailed_score(&match_id, &record)
                .await
                .map_err(dao_internal)?;
        }

        // Apply metadata + resolved score in one update.
        dao.update_match_meta(
            &match_id,
            input.name.as_deref(),
            input.description.as_deref(),
            status_override,
            input
                .starts_at
                .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                .as_deref(),
            confirmed_score,
            None,
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
        if let Some(assignments) = &input.side_assignments {
            // Reassign an existing player's side. Fetch current roster to
            // preserve the player's other fields.
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
            .has_liked_match(&match_id, &jwt_data.sub)
            .await
            .map_err(dao_internal)?;
        Ok(UpdateMatchResponse::Match(Json(match_from_records(
            &agg.match_,
            &agg.sides,
            &agg.players,
            i_liked,
        ))))
    }

    #[oai(path = "/matches/:match_id/detailed-score", method = "get")]
    async fn get_match_detailed_score(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(match_id): Path<String>,
    ) -> Result<GetMatchDetailedScoreResponse> {
        info!("Getting detailed score for match {match_id}");

        // The detailed score is stored under `DETAIL#<sport>`, so we need the
        // match's sport tag to address it. Fetch the match aggregate (meta) for
        // the sport; 404 if the match itself is missing.
        let agg = match dao.get_match(&match_id).await.map_err(dao_internal)? {
            Some(a) => a,
            None => {
                return Ok(GetMatchDetailedScoreResponse::NotFound(PlainText(
                    "match not found".into(),
                )));
            }
        };
        let record = dao
            .get_match_detailed_score(&match_id, &agg.match_.match_type)
            .await
            .map_err(dao_internal)?;
        match record.as_ref().and_then(detailed_score_from_record) {
            Some(ds) => Ok(GetMatchDetailedScoreResponse::DetailedScore(Json(ds))),
            None => Ok(GetMatchDetailedScoreResponse::NotFound(PlainText(
                "match has no detailed score".into(),
            ))),
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
            .find(|p| p.user_id.as_deref() == Some(jwt_data.sub.as_str()));
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
                dao.update_match_meta(&match_id, None, None, None, None, None, Some(None))
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
        // Idempotent create of the caller -> match like edge (bumps like_count).
        dao.like_match(&match_id, &jwt_data.sub, &now_iso())
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
        // Idempotent removal of the caller -> match like edge.
        dao.unlike_match(&match_id, &jwt_data.sub)
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
        let items = self.hydrate_user_profiles(dao, &ids).await?;
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
        if input.text.trim().is_empty() {
            return Ok(CreateCommentResponse::ValidationError(PlainText(
                "comment text must not be empty".into(),
            )));
        }

        let record = dao::records::CommentRecord {
            comment_id: new_id(),
            match_id: match_id.clone(),
            parent_id: input.parent_id.clone(),
            author_user_id: Some(jwt_data.sub.clone()),
            text: Some(input.text),
            created_at: now_iso(),
            edited_at: None,
            deleted_at: None,
            reply_count: 0,
        };

        // A reply targets a top-level comment: validate the parent exists and is
        // itself top-level (no replying to a reply).
        if let Some(parent_id) = &input.parent_id {
            match dao
                .get_comment(&match_id, parent_id)
                .await
                .map_err(dao_internal)?
            {
                Some(p) if p.parent_id.is_none() => {}
                Some(_) => {
                    return Ok(CreateCommentResponse::ValidationError(PlainText(
                        "cannot reply to a reply".into(),
                    )));
                }
                None => {
                    return Ok(CreateCommentResponse::NotFound(PlainText(
                        "parent comment not found".into(),
                    )));
                }
            }
            dao.create_reply(&record).await.map_err(dao_internal)?;
        } else {
            dao.create_comment(&record).await.map_err(dao_internal)?;
        }

        // Author is the caller.
        let author = self.try_user_profile(dao, &jwt_data.sub).await?;
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
        if existing.author_user_id.as_deref() != Some(jwt_data.sub.as_str()) {
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
        let author = self.try_user_profile(dao, &jwt_data.sub).await?;
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
        if existing.author_user_id.as_deref() != Some(jwt_data.sub.as_str()) {
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
            dao.delete_comment_hard(&match_id, &comment_id)
                .await
                .map_err(dao_internal)?;
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
        info!("Listing teams for {}", jwt_data.sub);
        let page = dao
            .list_user_teams(&jwt_data.sub, cursor.as_deref(), page_limit(limit))
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
        let now = now_iso();
        let team = dao::records::TeamRecord {
            id: new_id(),
            name: input.0.name,
            invite_token: Some(new_id()),
            follower_count: 0,
            created_at: now.clone(),
        };
        // The creator becomes the first member with the Admin role (already an
        // Agon user, so no invitation to accept).
        let creator = dao::records::TeamMemberRecord {
            team_id: team.id.clone(),
            membership_id: new_id(),
            user_id: Some(jwt_data.sub.clone()),
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
        Ok(CreateTeamResponse::Team(Json(team_from_records(
            &team,
            &[creator],
            false,
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
        match dao.get_team(&team_id).await.map_err(dao_internal)? {
            Some(agg) => {
                let is_followed_by_me = dao
                    .is_following_team(&jwt_data.sub, &team_id)
                    .await
                    .map_err(dao_internal)?;
                Ok(GetTeamResponse::Team(Json(team_from_records(
                    &agg.team,
                    &agg.members,
                    is_followed_by_me,
                ))))
            }
            None => Ok(GetTeamResponse::NotFound(PlainText(
                "team not found".into(),
            ))),
        }
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

        // Team must exist.
        if dao
            .get_team_meta(&team_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(AddTeamMembersResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        }

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

        // Return the updated team aggregate.
        match dao.get_team(&team_id).await.map_err(dao_internal)? {
            Some(agg) => Ok(AddTeamMembersResponse::Team(Json(team_from_records(
                &agg.team,
                &agg.members,
                false,
            )))),
            None => Ok(AddTeamMembersResponse::NotFound(PlainText(
                "team not found".into(),
            ))),
        }
    }

    #[oai(path = "/teams/:team_id", method = "patch")]
    async fn update_team(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        input: Json<UpdateTeamInput>,
    ) -> Result<UpdateTeamResponse> {
        info!("Updating team {team_id}");
        match dao.update_team(&team_id, input.0.name.as_deref()).await {
            Ok(()) => {}
            Err(dao::DaoError::NotFound(_)) => {
                return Ok(UpdateTeamResponse::NotFound(PlainText(
                    "team not found".into(),
                )));
            }
            Err(e) => return Err(dao_internal(e)),
        }
        match dao.get_team(&team_id).await.map_err(dao_internal)? {
            Some(agg) => Ok(UpdateTeamResponse::Team(Json(team_from_records(
                &agg.team,
                &agg.members,
                false,
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
        if dao
            .get_team_meta(&team_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(RemoveTeamMemberResponse::NotFound(PlainText(
                "team not found".into(),
            )));
        }
        dao.remove_team_member(&team_id, &member_id)
            .await
            .map_err(dao_internal)?;
        match dao.get_team(&team_id).await.map_err(dao_internal)? {
            Some(agg) => Ok(RemoveTeamMemberResponse::Team(Json(team_from_records(
                &agg.team,
                &agg.members,
                false,
            )))),
            None => Ok(RemoveTeamMemberResponse::NotFound(PlainText(
                "team not found".into(),
            ))),
        }
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
        // Match must exist.
        if dao
            .get_match(&match_id)
            .await
            .map_err(dao_internal)?
            .is_none()
        {
            return Ok(AddInvitationsResponse::NotFound(PlainText(
                "match not found".into(),
            )));
        }
        let ctx = dao::records::InvitationContextRecord::Match {
            match_id: match_id.clone(),
            match_name: String::new(), // snapshot label; TODO: read match name
        };
        let created = self
            .create_invitations(dao, &jwt_data.sub, ctx, &input.0)
            .await?;
        // TODO: also create the MatchPlayer roster slot per invitee (side_id from
        // input) — deferred with the roster-reconciliation work.
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
        let team = match dao.get_team_meta(&team_id).await.map_err(dao_internal)? {
            Some(t) => t,
            None => {
                return Ok(AddInvitationsResponse::NotFound(PlainText(
                    "team not found".into(),
                )));
            }
        };
        let ctx = dao::records::InvitationContextRecord::Team {
            team_id: team_id.clone(),
            team_name: team.name,
        };
        let created = self
            .create_invitations(dao, &jwt_data.sub, ctx, &input.0)
            .await?;
        // TODO: also create the TeamMember slot per invitee — deferred.
        Ok(AddInvitationsResponse::Invitations(Json(created)))
    }

    /// Create one invitation per invitee (users by id → user-kind; external
    /// names → token-kind), persist them, and return the API models. Shared by
    /// the match and team invite endpoints.
    async fn create_invitations(
        &self,
        dao: &dao::Dao,
        inviter_id: &str,
        context: dao::records::InvitationContextRecord,
        input: &AddInvitationsInput,
    ) -> Result<Vec<Invitation>> {
        let now = now_iso();
        let mut created = Vec::new();

        for user_id in &input.invited_user_ids {
            let rec = dao::records::InvitationRecord {
                id: new_id(),
                status: String::from("pending"),
                invited_by_user_id: inviter_id.to_string(),
                invited_user_id: Some(user_id.clone()),
                invite_token: None,
                kind: dao::records::InvitationKindRecord::User {
                    invited_user_id: user_id.clone(),
                },
                context: context.clone(),
                invited_at: now.clone(),
                responded_at: None,
            };
            dao.create_invitation(&rec).await.map_err(dao_internal)?;
            created.push(invitation_from_record(&rec));
        }

        for _name in &input.invited_external_names {
            let token = new_id();
            let rec = dao::records::InvitationRecord {
                id: new_id(),
                status: String::from("pending"),
                invited_by_user_id: inviter_id.to_string(),
                invited_user_id: None,
                invite_token: Some(token.clone()),
                kind: dao::records::InvitationKindRecord::Token {
                    invite_token: token,
                },
                context: context.clone(),
                invited_at: now.clone(),
                responded_at: None,
            };
            dao.create_invitation(&rec).await.map_err(dao_internal)?;
            created.push(invitation_from_record(&rec));
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
        let status_str = status.as_ref().map(invitation_status_str);
        let page = dao
            .list_user_invitations(
                &jwt_data.sub,
                status_str,
                cursor.as_deref(),
                page_limit(limit),
            )
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
        info!("Listing notifications for {}", jwt_data.sub);
        let page = dao
            .list_notifications(&jwt_data.sub, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;

        let mut items = Vec::with_capacity(page.items.len());
        for rec in page.items {
            // Hydrate the actor profile (every current kind has one).
            let actor = self
                .try_user_profile(dao, notification_actor_id(&rec.kind))
                .await?
                .unwrap_or_else(|| {
                    user_profile_from_record(
                        &dao::records::UserRecord {
                            id: notification_actor_id(&rec.kind).to_string(),
                            email: String::new(),
                            name: String::new(),
                            profile_image_url: None,
                            follower_count: 0,
                            following_count: 0,
                            unread_count: 0,
                            created_at: String::new(),
                        },
                        &[],
                        false,
                    )
                });
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
        info!("Getting unread notification count for {}", jwt_data.sub);
        let count = dao
            .unread_notification_count(&jwt_data.sub)
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
        info!("Marking all notifications read for {}", jwt_data.sub);
        dao.mark_all_notifications_read(&jwt_data.sub)
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
        dao.mark_notification_read(&jwt_data.sub, &notification_id)
            .await
            .map_err(dao_internal)?;
        Ok(MarkNotificationReadResponse::Ok)
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

    #[oai(path = "/invitations/:invitation_id", method = "delete")]
    async fn revoke_invitation(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(invitation_id): Path<String>,
    ) -> Result<RevokeInvitationResponse> {
        info!("Revoking invitation {invitation_id}");
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
            Some(rec) if rec.invited_by_user_id != jwt_data.sub => {
                return Ok(RevokeInvitationResponse::Forbidden(PlainText(
                    "only the inviter can revoke this invitation".into(),
                )));
            }
            Some(_) => {}
        }
        dao.delete_invitation(&invitation_id)
            .await
            .map_err(dao_internal)?;
        Ok(RevokeInvitationResponse::Ok)
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
        if rec.invited_user_id.as_deref() != Some(jwt_data.sub.as_str()) {
            return Ok(RespondToInvitationResponse::Forbidden(PlainText(
                "this invitation is not addressed to you".into(),
            )));
        }

        let status = match input.0.response {
            membership::InvitationResponse::Accepted => "accepted",
            membership::InvitationResponse::Declined => "declined",
        };
        let responded_at = now_iso();
        dao.respond_to_invitation(
            &invitation_id,
            status,
            &responded_at,
            &rec.invited_at,
            true, // has a user inbox (GSI1) to realign
        )
        .await
        .map_err(|e| match e {
            dao::DaoError::NotFound(_) => Error::from_string("not found", StatusCode::NOT_FOUND),
            other => dao_internal(other),
        })?;

        let mut invitation = invitation_from_record(&rec);
        invitation.status = invitation_status_from_str(status);
        invitation.responded_at = Some(mapping::parse_ts(&responded_at));
        // TODO: on accept, the async worker links/rosters the member + fans out.
        Ok(RespondToInvitationResponse::Invitation(Json(invitation)))
    }

    #[oai(path = "/invitations/respond-by-token", method = "post")]
    async fn respond_to_invitation_by_token(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        input: Json<RespondByTokenInput>,
    ) -> Result<RespondByTokenResponse> {
        info!("Responding to invitation by token: {:?}", input.response);
        let input = input.0;

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

        let status = match input.response {
            membership::InvitationResponse::Accepted => "accepted",
            membership::InvitationResponse::Declined => "declined",
        };
        let responded_at = now_iso();
        dao.respond_to_invitation(&rec.id, status, &responded_at, &rec.invited_at, false)
            .await
            .map_err(|e| match e {
                dao::DaoError::NotFound(_) => {
                    Error::from_string("not found", StatusCode::NOT_FOUND)
                }
                other => dao_internal(other),
            })?;

        let mut invitation = invitation_from_record(&rec);
        invitation.status = invitation_status_from_str(status);
        invitation.responded_at = Some(mapping::parse_ts(&responded_at));
        // TODO: on accept, the async worker links the external member to the
        // accepting account (external→user) and performs roster/fan-out.
        Ok(RespondByTokenResponse::Invitation(Json(invitation)))
    }

    #[oai(path = "/users/:user_id/follow", method = "post")]
    async fn follow_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(user_id): Path<String>,
    ) -> Result<FollowResponse> {
        info!("User {} following user {user_id}", jwt_data.sub);
        match dao.follow_user(&jwt_data.sub, &user_id, &now_iso()).await {
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
        info!("User {} unfollowing user {user_id}", jwt_data.sub);
        dao.unfollow_user(&jwt_data.sub, &user_id)
            .await
            .map_err(dao_internal)?;
        Ok(FollowResponse::Ok)
    }

    #[oai(path = "/users/:user_id/followers", method = "get")]
    async fn list_user_followers(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(user_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        info!("Listing followers of user {user_id}");
        let page = dao
            .list_user_followers(&user_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.follower_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids).await?;
        Ok(ListFollowsResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    #[oai(path = "/users/:user_id/following", method = "get")]
    async fn list_user_following(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(user_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        info!("Listing users that user {user_id} follows");
        let page = dao
            .list_user_following(&user_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.followee_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids).await?;
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
        info!("User {} following team {team_id}", jwt_data.sub);
        dao.follow_team(&jwt_data.sub, &team_id, &now_iso())
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
        info!("User {} unfollowing team {team_id}", jwt_data.sub);
        dao.unfollow_team(&jwt_data.sub, &team_id)
            .await
            .map_err(dao_internal)?;
        Ok(FollowResponse::Ok)
    }

    #[oai(path = "/teams/:team_id/followers", method = "get")]
    async fn list_team_followers(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        /// Opaque cursor from the previous page's `next_cursor`.
        Query(cursor): Query<Option<String>>,
        /// Maximum number of items to return (defaults to 20, capped at 50).
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListFollowsResponse> {
        info!("Listing followers of team {team_id}");
        let page = dao
            .list_team_followers(&team_id, cursor.as_deref(), page_limit(limit))
            .await
            .map_err(dao_internal)?;
        let ids: Vec<String> = page.items.into_iter().map(|e| e.follower_id).collect();
        let items = self.hydrate_user_profiles(dao, &ids).await?;
        Ok(ListFollowsResponse::Users(Json(UserPage {
            items,
            next_cursor: page.next_cursor,
        })))
    }

    /// Hydrate a list of user ids into `UserProfile`s. TODO: batch these with
    /// BatchGetItem; currently a per-id fetch (N+1). Missing users are skipped.
    /// `is_followed_by_me` is left false here (a follow-list view rarely needs
    /// it per-row); compute it if a screen requires it.
    async fn hydrate_user_profiles(
        &self,
        dao: &dao::Dao,
        ids: &[String],
    ) -> Result<Vec<UserProfile>> {
        let mut profiles = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(record) = dao.get_user(id).await.map_err(dao_internal)? {
                let stats = dao.list_user_stats(id).await.map_err(dao_internal)?;
                profiles.push(user_profile_from_record(&record, &stats, false));
            }
        }
        Ok(profiles)
    }

    /// Fetch a single user's public profile, or `None` if absent. Used to embed
    /// an author/actor inline. (N+1 in list contexts — batch later.)
    async fn try_user_profile(&self, dao: &dao::Dao, user_id: &str) -> Result<Option<UserProfile>> {
        match dao.get_user(user_id).await.map_err(dao_internal)? {
            Some(record) => {
                let stats = dao.list_user_stats(user_id).await.map_err(dao_internal)?;
                Ok(Some(user_profile_from_record(&record, &stats, false)))
            }
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

/// Re-point a `Score`'s side ids from request-scoped client ids to the real
/// side ids assigned at creation. Returns None if any referenced side id is
/// unknown.
fn resolve_score_side_ids(
    score: &Score,
    side_ids: &std::collections::HashMap<String, String>,
) -> Option<Score> {
    let map = |client_id: &str| side_ids.get(client_id).cloned();
    match score {
        Score::Simple(s) => {
            let mut entries = Vec::with_capacity(s.entries.len());
            for e in &s.entries {
                entries.push(SimpleScoreEntry {
                    side_id: map(&e.side_id)?,
                    points: e.points,
                });
            }
            Some(Score::Simple(SimpleScore { entries }))
        }
        Score::Sets(s) => {
            let mut entries = Vec::with_capacity(s.entries.len());
            for e in &s.entries {
                entries.push(SetsScoreEntry {
                    side_id: map(&e.side_id)?,
                    sets: e.sets.clone(),
                });
            }
            Some(Score::Sets(SetsScore { entries }))
        }
    }
}

/// Build a match player record plus the standalone invitation entity for one
/// invitee (an Agon user or an external). The player and invitation share the
/// invitation id/status; externals get a minted token.
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

/// The stored string tag for an upload purpose.
fn upload_purpose_str(p: &UploadPurpose) -> &'static str {
    match p {
        UploadPurpose::ProfileImage => "profile_image",
        UploadPurpose::TeamImage => "team_image",
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
/// generated presigned upload target (short-lived, so regenerated on each read);
/// uploaded assets carry their public `url` and no target.
fn asset_from_record(record: &dao::records::AssetRecord) -> Asset {
    let status = asset_status_from_str(&record.status);
    let upload = match status {
        AssetStatus::Pending => Some(UploadTarget {
            // TODO: generate a real provider-specific presigned PUT for
            // `record.storage_key` (S3/R2/GCS/Supabase). The contract stays
            // provider-agnostic: the client just replays method + headers.
            upload_url: format!(
                "https://storage.example.com/uploads/{}?signature=placeholder",
                record.storage_key
            ),
            method: String::from("PUT"),
            headers: vec![UploadHeader {
                name: String::from("Content-Type"),
                value: record.content_type.clone(),
            }],
        }),
        _ => None,
    };
    Asset {
        id: record.id.clone(),
        status,
        content_type: record.content_type.clone(),
        upload,
        url: record.url.clone(),
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
        }),
        stats: vec![UserSportStats {
            match_type: MatchType::Tennis,
            matches_played: 12,
            win_percentage: 58.3,
        }],
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
        }],
        sides: vec![
            MatchSide {
                id: String::from("side_red"),
                team_id: Some(String::from("team_red")),
                name: Some(String::from("Red Team")),
            },
            MatchSide {
                id: String::from("side_blue"),
                team_id: Some(String::from("team_blue")),
                name: Some(String::from("Blue Team")),
            },
        ],
        players: vec![
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_red_1"),
                    user_id: String::from("user_1"),
                    invitation: None,
                }),
                side_id: Some(String::from("side_red")),
                is_member_of_team: Some(true),
            },
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_red_2"),
                    user_id: String::from("user_2"),
                    invitation: None,
                }),
                side_id: Some(String::from("side_red")),
                is_member_of_team: Some(true),
            },
            MatchPlayer {
                member: Member::User(UserMember {
                    id: String::from("player_blue_1"),
                    user_id: String::from("user_3"),
                    invitation: None,
                }),
                side_id: Some(String::from("side_blue")),
                is_member_of_team: Some(true),
            },
        ],
        confirmed_score: Some(ConfirmedScore {
            score: Score::Simple(SimpleScore {
                entries: vec![
                    SimpleScoreEntry {
                        side_id: String::from("side_red"),
                        points: 3,
                    },
                    SimpleScoreEntry {
                        side_id: String::from("side_blue"),
                        points: 1,
                    },
                ],
            }),
            winner_side_id: Some(String::from("side_red")),
        }),
        pending_score: None,
        social: MatchSocial {
            like_count: 3,
            comment_count: 2,
            i_liked: false,
        },
    }
}

/// Builds a mock football detailed score matching the mock match's 3-1 result.
fn mock_football_detailed_score() -> DetailedScore {
    DetailedScore::Football(FootballDetail {
        events: vec![
            FootballEvent {
                kind: FootballEventKind::Goal,
                side_id: String::from("side_red"),
                minute: Some(12),
                player_id: Some(String::from("player_red_1")),
                assist_player_id: Some(String::from("player_red_2")),
                substituted_player_id: None,
            },
            FootballEvent {
                kind: FootballEventKind::Goal,
                side_id: String::from("side_blue"),
                minute: Some(34),
                player_id: Some(String::from("player_blue_1")),
                assist_player_id: None,
                substituted_player_id: None,
            },
            FootballEvent {
                kind: FootballEventKind::Goal,
                side_id: String::from("side_red"),
                minute: Some(58),
                player_id: Some(String::from("player_red_2")),
                assist_player_id: Some(String::from("player_red_1")),
                substituted_player_id: None,
            },
            FootballEvent {
                kind: FootballEventKind::Penalty,
                side_id: String::from("side_red"),
                minute: Some(81),
                player_id: Some(String::from("player_red_1")),
                assist_player_id: None,
                substituted_player_id: None,
            },
        ],
    })
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
    BASE64_URL_SAFE.encode(bytes)
}

/// A mock confirmed score submission with one confirm response.
fn mock_score_submission() -> ScoreSubmission {
    ScoreSubmission {
        id: String::from("submission_1"),
        score: Score::Simple(SimpleScore {
            entries: vec![
                SimpleScoreEntry {
                    side_id: String::from("side_red"),
                    points: 3,
                },
                SimpleScoreEntry {
                    side_id: String::from("side_blue"),
                    points: 1,
                },
            ],
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
    ]
}

/// Builds a mock lightweight team list item.
fn mock_team_list_item(id: String, name: String) -> TeamListItem {
    TeamListItem {
        id,
        name,
        follower_count: 128,
        is_followed_by_me: false,
    }
}

/// Builds a mock team. Shared by the team endpoints until a real DAO is wired in.
fn mock_team(id: String, name: String) -> Team {
    let invited_at = mock_timestamp();

    Team {
        id,
        name,
        members: vec![
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
        invite_token: Some(String::from("team_invite_abc123")),
        follower_count: 128,
        is_followed_by_me: false,
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

/// Clamps a client-supplied limit to `[_, MAX_PAGE_LIMIT]`, defaulting when absent.
fn page_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT)
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

    /// Generates a signed JWT for local testing (signed with JWT_SECRET)
    GenerateToken {
        /// The subject (user id) to embed in the `sub` claim
        #[arg(default_value = "test-user")]
        sub: String,
    },
}

fn log_request(uri: Uri, status: StatusCode) {
    info!(
        path = uri.path(),
        status = status.as_u16(),
        "Request complete"
    );
}

async fn log_middleware<E: Endpoint>(next: E, req: Request) -> Result<Response> {
    let uri = req.uri().clone();
    let res = next.call(req).await;

    match res {
        Ok(resp) => {
            let resp = resp.into_response();
            log_request(uri, resp.status());
            Ok(resp)
        }
        Err(err) => {
            log_request(uri, err.status());
            Err(err)
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    let args = Cli::parse();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:7000");

    match args.command {
        Commands::RunServer { url: _ } => {
            info!("Starting up server");

            let ui = api_service.scalar();

            let table = std::env::var("AGON_TABLE_NAME").unwrap_or_else(|_| "agon".to_string());
            let dao = dao::Dao::from_env(table).await;

            // Meilisearch client for discovery endpoints (users/teams/matches
            // search). Indexes are kept in sync by the async worker; the API only
            // queries them and hydrates results from DynamoDB.
            let meili_url =
                std::env::var("MEILI_URL").unwrap_or_else(|_| "http://localhost:7700".to_string());
            let meili_key = std::env::var("MEILI_MASTER_KEY").unwrap_or_default();
            let search = agon_core::search::SearchClient::new(meili_url, meili_key);

            let cors = Cors::new()
                .allow_origin("*")
                .allow_origin("http://localhost:5174")
                .allow_origin("http://localhost:5175")
                .allow_origin("http://localhost:7003")
                .allow_origin_regex("https://*.get-agon.com")
                .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allow_headers(vec!["content-type", "authorization"])
                .allow_credentials(true);

            let app = Route::new()
                .nest("/", api_service)
                .nest("/docs", ui)
                .with(cors)
                .data(dao)
                .data(search)
                .around(log_middleware);

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

        Commands::GenerateToken { sub } => {
            let secret_key = std::env::var("JWT_SECRET").expect("JWT Secret not found");

            let claims = JwtClaims {
                sub,
                // far-future expiry; `jwt_checker` disables exp validation anyway
                exp: 9_999_999_999,
                iss: None,
                aud: None,
                role: None,
            };

            let token = encode(
                &Header::new(Algorithm::HS256),
                &claims,
                &EncodingKey::from_secret(secret_key.as_bytes()),
            )
            .expect("Failed to encode JWT");

            println!("{token}");
        }
    }
}
