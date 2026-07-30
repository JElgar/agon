//! Boundary mapping between API models (poem-openapi) and DAO records.
//!
//! The DAO owns its own record types and shares nothing with the API. This
//! module is the single place that translates between the two, so handlers stay
//! thin and the coupling lives in one file.

use poem::error::InternalServerError;

use crate::detailed_score::DetailedScore;
use crate::detailed_score::cricket::{
    CricketDelivery, CricketDeliveryExtra, CricketDeliveryWicket, CricketDismissalKind,
    CricketExtraKind, Overs,
};
use crate::detailed_score::football::{
    FootballCardColor, FootballCardEvent, FootballGoalEvent, FootballPeriod,
    FootballSubstitutionEvent,
};
use crate::live_score::{
    LiveEvent, LiveEventInput, NewLiveEventInput,
    cricket::{
        CricketInningsEndEvent, CricketInningsStartEvent, CricketLiveEvent, CricketRetireEvent,
        InningsEndReason,
    },
    football::{FootballLiveEvent, FootballPeriodEvent},
};
use crate::match_format::{CricketFormat, FootballFormat, MatchFormat};
use crate::membership::{
    ExternalMember, Invitation, InvitationContext, InvitationKind, InvitationMatchContext,
    InvitationStatus, InvitationTeamContext, Member, TokenInvitation, UserInvitation, UserMember,
};
use crate::notification::{
    CommentNotification, FollowNotification, InvitationAcceptedNotification, LikeNotification,
    MatchInvitationNotification, Notification, NotificationKind, ReplyNotification,
    ScoreConfirmedNotification, ScoreSubmittedNotification, TeamInvitationNotification,
};
use crate::team::{Team, TeamListItem, TeamMember, TeamRole};
use crate::{
    Comment, ConfirmedScore, CricketScore, CricketScoreInnings, Location, Match, MatchPlayer,
    MatchSide, MatchSocial, MatchStatus, MatchType, PendingScore, Photo, Score, ScoreConfirmation,
    ScoreResponseKind, ScoreSubmission, ScoreSubmissionResponse, ScoreSubmissionStatus, SetsScore,
    SetsScoreEntry, SimpleScore, SimpleScoreEntry, UserProfile, UserSportStats,
};
use agon_core::dao::error::DaoError;
use agon_core::dao::live_score_ops::NewLiveEvent;
use agon_core::dao::records::{
    CommentRecord, ConfirmedScoreRecord, CricketDeliveryExtraRecord, CricketDeliveryRecord,
    CricketDeliveryWicketRecord, CricketDismissalKindRecord, CricketExtraKindRecord,
    CricketFormatRecord, CricketInningsEndEventRecord, CricketInningsStartEventRecord,
    CricketLiveEventRecord, CricketRetireEventRecord, CricketScoreInningsRecord,
    EmbeddedInvitationRecord, FootballCardColorRecord, FootballCardEventRecord,
    FootballFormatRecord, FootballGoalEventRecord, FootballLiveEventRecord,
    FootballPeriodEventRecord, FootballPeriodRecord, FootballSubstitutionEventRecord,
    InningsEndReasonRecord, InvitationContextRecord, InvitationKindRecord, InvitationRecord,
    LiveEventPayloadRecord, LiveEventRecord, MatchDetailedScoreRecord, MatchFormatRecord,
    MatchLikeRecord, MatchPlayerRecord, MatchRecord, MatchSideRecord, NotificationKindRecord,
    NotificationRecord, OversRecord, PendingScoreRecord, ScoreConfirmationRecord, ScoreRecord,
    ScoreResponseRecord, ScoreSubmissionRecord, SetsScoreEntryRecord, SimpleScoreEntryRecord,
    TeamMemberRecord, TeamRecord, UserRecord, UserSportStatsRecord,
};
use poem_openapi::types::{ParseFromJSON, ToJSON};

/// Parse an RFC-3339 timestamp string stored by the DAO into a UTC datetime,
/// defaulting to the epoch on a malformed value (reads never fail on bad data).
pub fn parse_ts(raw: &str) -> chrono::DateTime<chrono::Utc> {
    raw.parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
}

/// Parse an optional stored timestamp.
pub fn parse_ts_opt(raw: &Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    raw.as_deref().map(parse_ts)
}

/// Map an unexpected `DaoError` to a 500. Handlers deal with the *expected*
/// variants (Conflict/NotFound) explicitly by matching before calling this.
pub fn dao_internal(err: DaoError) -> poem::Error {
    InternalServerError(err)
}

/// Parse a stored sport tag into the API enum, defaulting unknown values rather
/// than failing a read.
pub fn match_type_from_tag(tag: &str) -> MatchType {
    match tag {
        "tennis" => MatchType::Tennis,
        "badminton" => MatchType::Badminton,
        "squash" => MatchType::Squash,
        "table_tennis" => MatchType::TableTennis,
        "football" => MatchType::Football,
        "cricket" => MatchType::Cricket,
        _ => MatchType::Other,
    }
}

/// The stored string tag for an API sport enum.
pub fn match_type_tag(mt: &MatchType) -> &'static str {
    match mt {
        MatchType::Tennis => "tennis",
        MatchType::Badminton => "badminton",
        MatchType::Squash => "squash",
        MatchType::TableTennis => "table_tennis",
        MatchType::Football => "football",
        MatchType::Cricket => "cricket",
        MatchType::Other => "other",
    }
}

/// Build the public `UserProfile` from a stored user record (its inline
/// per-sport `stats` map) and the viewer-relative follow flag.
pub fn user_profile_from_record(user: &UserRecord, is_followed_by_me: bool) -> UserProfile {
    // Sort by sport tag for a deterministic response order (a `HashMap`'s
    // iteration order isn't).
    let mut entries: Vec<(&String, &UserSportStatsRecord)> = user.stats.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    UserProfile {
        id: user.id.clone(),
        name: user.name.clone(),
        profile_image: user.profile_image_url.as_ref().map(|url| Photo {
            image_url: url.clone(),
        }),
        stats: entries
            .into_iter()
            .map(|(sport, rec)| sport_stats_from_record(sport, rec))
            .collect(),
        follower_count: user.follower_count as u32,
        following_count: user.following_count as u32,
        is_followed_by_me,
    }
}

/// Map a stored per-sport stats record to the API model, deriving win %.
pub fn sport_stats_from_record(sport: &str, rec: &UserSportStatsRecord) -> UserSportStats {
    let win_percentage = if rec.matches_played == 0 {
        0.0
    } else {
        (rec.wins as f32 / rec.matches_played as f32) * 100.0
    };
    UserSportStats {
        match_type: match_type_from_tag(sport),
        matches_played: rec.matches_played as i32,
        win_percentage,
    }
}

// ===========================================================================
// Match status
// ===========================================================================

pub fn match_status_from_str(s: &str) -> MatchStatus {
    match s {
        "scheduled" => MatchStatus::Scheduled,
        "in_progress" => MatchStatus::InProgress,
        "completed" => MatchStatus::Completed,
        "cancelled" => MatchStatus::Cancelled,
        _ => MatchStatus::Scheduled,
    }
}

pub fn match_status_str(s: &MatchStatus) -> &'static str {
    match s {
        MatchStatus::Scheduled => "scheduled",
        MatchStatus::InProgress => "in_progress",
        MatchStatus::Completed => "completed",
        MatchStatus::Cancelled => "cancelled",
    }
}

// ===========================================================================
// Score (union) <-> ScoreRecord
// ===========================================================================

pub fn score_from_record(rec: &ScoreRecord) -> Score {
    match rec {
        ScoreRecord::Simple { entries } => Score::Simple(SimpleScore {
            entries: entries
                .iter()
                .map(|e| SimpleScoreEntry {
                    side_id: e.side_id.clone(),
                    points: e.points,
                })
                .collect(),
        }),
        ScoreRecord::Sets { entries } => Score::Sets(SetsScore {
            entries: entries
                .iter()
                .map(|e| SetsScoreEntry {
                    side_id: e.side_id.clone(),
                    sets: e.sets.clone(),
                })
                .collect(),
        }),
        ScoreRecord::Cricket { innings } => Score::Cricket(CricketScore {
            innings: innings
                .iter()
                .map(|i| CricketScoreInnings {
                    batting_side_id: i.batting_side_id.clone(),
                    bowling_side_id: i.bowling_side_id.clone(),
                    runs: i.runs,
                    wickets: i.wickets,
                    overs: overs_from_record(&i.overs),
                    declared: i.declared,
                })
                .collect(),
        }),
    }
}

fn overs_from_record(rec: &OversRecord) -> Overs {
    Overs {
        overs: rec.overs,
        balls: rec.balls,
    }
}

fn overs_to_record(overs: &Overs) -> OversRecord {
    OversRecord {
        overs: overs.overs,
        balls: overs.balls,
    }
}

pub fn score_to_record(score: &Score) -> ScoreRecord {
    match score {
        Score::Simple(s) => ScoreRecord::Simple {
            entries: s
                .entries
                .iter()
                .map(|e| SimpleScoreEntryRecord {
                    side_id: e.side_id.clone(),
                    points: e.points,
                })
                .collect(),
        },
        Score::Sets(s) => ScoreRecord::Sets {
            entries: s
                .entries
                .iter()
                .map(|e| SetsScoreEntryRecord {
                    side_id: e.side_id.clone(),
                    sets: e.sets.clone(),
                })
                .collect(),
        },
        Score::Cricket(s) => ScoreRecord::Cricket {
            innings: s
                .innings
                .iter()
                .map(|i| CricketScoreInningsRecord {
                    batting_side_id: i.batting_side_id.clone(),
                    bowling_side_id: i.bowling_side_id.clone(),
                    runs: i.runs,
                    wickets: i.wickets,
                    overs: overs_to_record(&i.overs),
                    declared: i.declared,
                })
                .collect(),
        },
    }
}

pub fn confirmed_score_from_record(rec: &ConfirmedScoreRecord) -> ConfirmedScore {
    ConfirmedScore {
        score: score_from_record(&rec.score),
        winner_side_id: rec.winner_side_id.clone(),
    }
}

pub fn pending_score_from_record(rec: &PendingScoreRecord) -> PendingScore {
    PendingScore {
        submission_id: rec.submission_id.clone(),
        score: score_from_record(&rec.score),
        winner_side_id: rec.winner_side_id.clone(),
        confirmations: rec
            .confirmations
            .iter()
            .map(score_confirmation_from_record)
            .collect(),
    }
}

pub fn score_confirmation_from_record(rec: &ScoreConfirmationRecord) -> ScoreConfirmation {
    ScoreConfirmation {
        side_id: rec.side_id.clone(),
        confirmed_by_player_id: rec.confirmed_by_player_id.clone(),
        confirmed_at: parse_ts(&rec.confirmed_at),
    }
}

// ===========================================================================
// Score submissions
// ===========================================================================

pub fn score_submission_status_from_str(s: &str) -> ScoreSubmissionStatus {
    match s {
        "pending" => ScoreSubmissionStatus::Pending,
        "confirmed" => ScoreSubmissionStatus::Confirmed,
        "disputed" => ScoreSubmissionStatus::Disputed,
        "superseded" => ScoreSubmissionStatus::Superseded,
        _ => ScoreSubmissionStatus::Pending,
    }
}

pub fn score_submission_status_str(s: &ScoreSubmissionStatus) -> &'static str {
    match s {
        ScoreSubmissionStatus::Pending => "pending",
        ScoreSubmissionStatus::Confirmed => "confirmed",
        ScoreSubmissionStatus::Disputed => "disputed",
        ScoreSubmissionStatus::Superseded => "superseded",
    }
}

pub fn score_response_kind_from_str(s: &str) -> ScoreResponseKind {
    match s {
        "dispute" => ScoreResponseKind::Dispute,
        _ => ScoreResponseKind::Confirm,
    }
}

pub fn score_response_kind_str(k: &ScoreResponseKind) -> &'static str {
    match k {
        ScoreResponseKind::Confirm => "confirm",
        ScoreResponseKind::Dispute => "dispute",
    }
}

pub fn score_response_from_record(rec: &ScoreResponseRecord) -> ScoreSubmissionResponse {
    ScoreSubmissionResponse {
        side_id: rec.side_id.clone(),
        responded_by_player_id: rec.responded_by_player_id.clone(),
        response: score_response_kind_from_str(&rec.response),
        responded_at: parse_ts(&rec.responded_at),
    }
}

pub fn score_submission_from_record(rec: &ScoreSubmissionRecord) -> ScoreSubmission {
    ScoreSubmission {
        id: rec.submission_id.clone(),
        score: score_from_record(&rec.score),
        winner_side_id: rec.winner_side_id.clone(),
        status: score_submission_status_from_str(&rec.status),
        submitted_by_player_id: rec.submitted_by_player_id.clone(),
        submitted_at: parse_ts(&rec.submitted_at),
        responses: rec
            .responses
            .iter()
            .map(score_response_from_record)
            .collect(),
    }
}

// ===========================================================================
// Invitations
// ===========================================================================

pub fn invitation_status_from_str(s: &str) -> InvitationStatus {
    match s {
        "accepted" => InvitationStatus::Accepted,
        "declined" => InvitationStatus::Declined,
        _ => InvitationStatus::Pending,
    }
}

pub fn invitation_status_str(s: &InvitationStatus) -> &'static str {
    match s {
        InvitationStatus::Pending => "pending",
        InvitationStatus::Accepted => "accepted",
        InvitationStatus::Declined => "declined",
    }
}

pub fn invitation_kind_from_record(rec: &InvitationKindRecord) -> InvitationKind {
    match rec {
        InvitationKindRecord::User { invited_user_id } => InvitationKind::User(UserInvitation {
            invited_user_id: invited_user_id.clone(),
        }),
        InvitationKindRecord::Token { invite_token } => InvitationKind::Token(TokenInvitation {
            invite_token: invite_token.clone(),
        }),
    }
}

pub fn invitation_context_from_record(rec: &InvitationContextRecord) -> InvitationContext {
    match rec {
        InvitationContextRecord::Match {
            match_id,
            match_name,
        } => InvitationContext::Match(InvitationMatchContext {
            match_id: match_id.clone(),
            match_name: match_name.clone(),
        }),
        InvitationContextRecord::Team { team_id, team_name } => {
            InvitationContext::Team(InvitationTeamContext {
                team_id: team_id.clone(),
                team_name: team_name.clone(),
            })
        }
    }
}

/// Build the API `Invitation` from a standalone invitation record.
pub fn invitation_from_record(rec: &InvitationRecord) -> Invitation {
    Invitation {
        id: rec.id.clone(),
        status: invitation_status_from_str(&rec.status),
        invited_by_user_id: rec.invited_by_user_id.clone(),
        invited_at: parse_ts(&rec.invited_at),
        responded_at: parse_ts_opt(&rec.responded_at),
        kind: invitation_kind_from_record(&rec.kind),
    }
}

/// Build the standalone `InvitationDetail` (invitation + its context) from a
/// stored invitation record.
pub fn invitation_detail_from_record(
    rec: &InvitationRecord,
) -> crate::membership::InvitationDetail {
    crate::membership::InvitationDetail {
        invitation: invitation_from_record(rec),
        context: invitation_context_from_record(&rec.context),
    }
}

/// Build the API `Invitation` from an invitation snapshot embedded on a member.
pub fn invitation_from_embedded(rec: &EmbeddedInvitationRecord) -> Invitation {
    Invitation {
        id: rec.id.clone(),
        status: invitation_status_from_str(&rec.status),
        invited_by_user_id: rec.invited_by_user_id.clone(),
        invited_at: parse_ts(&rec.invited_at),
        responded_at: parse_ts_opt(&rec.responded_at),
        kind: invitation_kind_from_record(&rec.kind),
    }
}

// ===========================================================================
// Members (team member / match player)
// ===========================================================================

/// Build the shared `Member` union from the fields common to a team member /
/// match player record: a linked user id or an external display name, plus an
/// optional embedded invitation. A linked user's `name`/`avatar_url` are left
/// blank here — callers that need them hydrated (e.g. a match roster) fill
/// them in afterwards with a batch profile lookup.
pub fn member_from_parts(
    membership_id: &str,
    user_id: Option<&str>,
    display_name: Option<&str>,
    invitation: Option<&EmbeddedInvitationRecord>,
) -> Member {
    let invitation = invitation.map(invitation_from_embedded);
    match user_id {
        Some(uid) => Member::User(UserMember {
            id: membership_id.to_string(),
            user_id: uid.to_string(),
            invitation,
            name: String::new(),
            avatar_url: None,
        }),
        None => Member::External(ExternalMember {
            id: membership_id.to_string(),
            display_name: display_name.unwrap_or_default().to_string(),
            invitation,
        }),
    }
}

pub fn team_role_from_str(s: &str) -> TeamRole {
    match s {
        "admin" => TeamRole::Admin,
        _ => TeamRole::Member,
    }
}

pub fn team_role_str(r: &TeamRole) -> &'static str {
    match r {
        TeamRole::Admin => "admin",
        TeamRole::Member => "member",
    }
}

pub fn team_member_from_record(rec: &TeamMemberRecord) -> TeamMember {
    TeamMember {
        member: member_from_parts(
            &rec.membership_id,
            rec.user_id.as_deref(),
            rec.display_name.as_deref(),
            rec.invitation.as_ref(),
        ),
        role: team_role_from_str(&rec.role),
    }
}

pub fn team_from_records(
    team: &TeamRecord,
    members: &[TeamMemberRecord],
    is_followed_by_me: bool,
) -> Team {
    Team {
        id: team.id.clone(),
        name: team.name.clone(),
        members: members.iter().map(team_member_from_record).collect(),
        invite_token: team.invite_token.clone(),
        follower_count: team.follower_count as u32,
        is_followed_by_me,
    }
}

pub fn team_list_item_from_record(team: &TeamRecord, is_followed_by_me: bool) -> TeamListItem {
    TeamListItem {
        id: team.id.clone(),
        name: team.name.clone(),
        follower_count: team.follower_count as u32,
        is_followed_by_me,
    }
}

pub fn match_player_from_record(rec: &MatchPlayerRecord) -> MatchPlayer {
    MatchPlayer {
        member: member_from_parts(
            &rec.player_id,
            rec.user_id.as_deref(),
            rec.display_name.as_deref(),
            rec.invitation.as_ref(),
        ),
        side_id: rec.side_id.clone(),
        is_member_of_team: rec.is_member_of_team,
    }
}

pub fn match_side_from_record(rec: &MatchSideRecord) -> MatchSide {
    MatchSide {
        id: rec.side_id.clone(),
        team_id: rec.team_id.clone(),
        name: rec.name.clone(),
    }
}

// ===========================================================================
// Match aggregate
// ===========================================================================

/// Build the API `Match` from a match record plus its sides and players.
/// `i_liked` is a viewer-relative flag the caller resolves separately.
pub fn match_from_records(
    rec: &MatchRecord,
    sides: &[MatchSideRecord],
    players: &[MatchPlayerRecord],
    i_liked: bool,
) -> Match {
    Match {
        id: rec.id.clone(),
        name: rec.name.clone(),
        description: rec.description.clone(),
        match_type: match_type_from_tag(&rec.match_type),
        status: match_status_from_str(&rec.status),
        starts_at: parse_ts(&rec.starts_at),
        location: rec.location.as_ref().map(|l| Location {
            latitude: l.latitude,
            longitude: l.longitude,
        }),
        header_photos: rec
            .header_photo_urls
            .iter()
            .map(|url| Photo {
                image_url: url.clone(),
            })
            .collect(),
        sides: sides.iter().map(match_side_from_record).collect(),
        players: players.iter().map(match_player_from_record).collect(),
        confirmed_score: rec
            .confirmed_score
            .as_ref()
            .map(confirmed_score_from_record),
        pending_score: rec.pending_score.as_ref().map(pending_score_from_record),
        social: MatchSocial {
            like_count: rec.like_count as u32,
            comment_count: rec.comment_count as u32,
            i_liked,
        },
        format: rec.format.as_ref().map(match_format_from_record),
    }
}

// ===========================================================================
// Comments
// ===========================================================================

/// Build the API `Comment` from a comment record. `author` is resolved by the
/// caller (None on a tombstone) and passed in.
pub fn comment_from_record(rec: &CommentRecord, author: Option<UserProfile>) -> Comment {
    Comment {
        id: rec.comment_id.clone(),
        parent_id: rec.parent_id.clone(),
        author,
        text: rec.text.clone(),
        created_at: parse_ts(&rec.created_at),
        edited_at: parse_ts_opt(&rec.edited_at),
        reply_count: rec.reply_count as u32,
        deleted_at: parse_ts_opt(&rec.deleted_at),
    }
}

/// Whether a comment record is a tombstone (deleted but kept for its replies).
pub fn comment_is_tombstone(rec: &CommentRecord) -> bool {
    rec.deleted_at.is_some()
}

// ===========================================================================
// Likes
// ===========================================================================

/// The liking user's id from a like record.
pub fn like_user_id(rec: &MatchLikeRecord) -> String {
    rec.user_id.clone()
}

// ===========================================================================
// Detailed score <-> serde_json::Value (via poem-openapi's JSON traits)
// ===========================================================================

/// Serialize a `DetailedScore` union into the `(sport, detail)` record shape.
/// The sport tag mirrors the union variant so a read can pick the right variant.
pub fn detailed_score_to_record(
    ds: &DetailedScore,
    last_seq: Option<u32>,
) -> MatchDetailedScoreRecord {
    let sport = match ds {
        DetailedScore::Football(_) => "football",
        DetailedScore::Cricket(_) => "cricket",
    }
    .to_string();
    let detail = ds.to_json().unwrap_or(serde_json::Value::Null);
    MatchDetailedScoreRecord {
        sport,
        detail,
        last_seq,
    }
}

/// Parse a stored detailed-score record back into the `DetailedScore` union.
/// Returns None if the stored blob can't be parsed (treated as "no detail").
pub fn detailed_score_from_record(rec: &MatchDetailedScoreRecord) -> Option<DetailedScore> {
    DetailedScore::parse_from_json(Some(rec.detail.clone())).ok()
}

// ===========================================================================
// Match format: MatchFormat (API, poem-openapi) <-> MatchFormatRecord (DAO,
// plain serde). A hand-mirrored DAO enum rather than opaque JSON, same
// convention as live scoring (see LiveEventPayloadRecord's doc comment) and
// for the same reason: a variant added on the API side and forgotten here is
// a compile error, not a silently-dropped setting on read.
// ===========================================================================

pub fn match_format_to_record(fmt: &MatchFormat) -> MatchFormatRecord {
    match fmt {
        MatchFormat::Football(f) => MatchFormatRecord::Football(FootballFormatRecord {
            half_length_minutes: f.half_length_minutes,
            num_halves: f.num_halves,
            extra_time: f.extra_time,
            extra_time_half_length_minutes: f.extra_time_half_length_minutes,
            penalties: f.penalties,
        }),
        MatchFormat::Cricket(f) => MatchFormatRecord::Cricket(CricketFormatRecord {
            overs_per_innings: f.overs_per_innings,
            innings_per_side: f.innings_per_side,
            balls_per_over: f.balls_per_over,
            no_ball_penalty_runs: f.no_ball_penalty_runs,
            wide_penalty_runs: f.wide_penalty_runs,
            wide_is_extra_ball: f.wide_is_extra_ball,
            no_ball_is_extra_ball: f.no_ball_is_extra_ball,
            free_hit_after_no_ball: f.free_hit_after_no_ball,
        }),
    }
}

/// Build the API `MatchFormat` from a stored record. Infallible — `format`
/// is a typed DAO enum, not JSON, so there's no parse step that can fail.
pub fn match_format_from_record(rec: &MatchFormatRecord) -> MatchFormat {
    match rec {
        MatchFormatRecord::Football(f) => MatchFormat::Football(FootballFormat {
            half_length_minutes: f.half_length_minutes,
            num_halves: f.num_halves,
            extra_time: f.extra_time,
            extra_time_half_length_minutes: f.extra_time_half_length_minutes,
            penalties: f.penalties,
        }),
        MatchFormatRecord::Cricket(f) => MatchFormat::Cricket(CricketFormat {
            overs_per_innings: f.overs_per_innings,
            innings_per_side: f.innings_per_side,
            balls_per_over: f.balls_per_over,
            no_ball_penalty_runs: f.no_ball_penalty_runs,
            wide_penalty_runs: f.wide_penalty_runs,
            wide_is_extra_ball: f.wide_is_extra_ball,
            no_ball_is_extra_ball: f.no_ball_is_extra_ball,
            free_hit_after_no_ball: f.free_hit_after_no_ball,
        }),
    }
}

/// The sport tag a `MatchFormat` is for, mirroring the union variant (same
/// convention as `live_event_sport_tag`) — used to reject a format that
/// doesn't match the match's own sport.
pub fn match_format_sport_tag(fmt: &MatchFormat) -> &'static str {
    match fmt {
        MatchFormat::Football(_) => "football",
        MatchFormat::Cricket(_) => "cricket",
    }
}

// ===========================================================================
// Live scoring: LiveEventInput (API, poem-openapi) <-> LiveEventPayloadRecord
// (DAO, plain serde). Two hand-synced enum trees rather than an opaque JSON
// blob — see LiveEventRecord's doc comment for why. The compiler catches a
// variant added on one side and forgotten on the other; a missing match arm
// below is a build failure, not a silent runtime drop.
// ===========================================================================

/// The stored sport tag for a live event, mirroring the union variant so a
/// read can pick the right variant back out (same convention as
/// `detailed_score_to_record`).
pub fn live_event_sport_tag(event: &LiveEventInput) -> &'static str {
    match event {
        LiveEventInput::Football(_) => "football",
        LiveEventInput::Cricket(_) => "cricket",
    }
}

pub fn live_event_input_to_record(event: &LiveEventInput) -> LiveEventPayloadRecord {
    match event {
        LiveEventInput::Football(f) => {
            LiveEventPayloadRecord::Football(football_live_event_to_record(f))
        }
        LiveEventInput::Cricket(c) => {
            LiveEventPayloadRecord::Cricket(cricket_live_event_to_record(c))
        }
    }
}

pub fn live_event_payload_from_record(rec: &LiveEventPayloadRecord) -> LiveEventInput {
    match rec {
        LiveEventPayloadRecord::Football(f) => {
            LiveEventInput::Football(football_live_event_from_record(f))
        }
        LiveEventPayloadRecord::Cricket(c) => {
            LiveEventInput::Cricket(cricket_live_event_from_record(c))
        }
    }
}

// ---- Football ---------------------------------------------------------

fn football_live_event_to_record(event: &FootballLiveEvent) -> FootballLiveEventRecord {
    match event {
        FootballLiveEvent::Goal(g) => FootballLiveEventRecord::Goal(FootballGoalEventRecord {
            side_id: g.side_id.clone(),
            scorer_player_id: g.scorer_player_id.clone(),
            assist_player_id: g.assist_player_id.clone(),
            own_goal: g.own_goal,
            penalty: g.penalty,
            minute: g.minute,
        }),
        FootballLiveEvent::Card(c) => FootballLiveEventRecord::Card(FootballCardEventRecord {
            side_id: c.side_id.clone(),
            player_id: c.player_id.clone(),
            color: match c.color {
                FootballCardColor::Yellow => FootballCardColorRecord::Yellow,
                FootballCardColor::Red => FootballCardColorRecord::Red,
            },
            minute: c.minute,
        }),
        FootballLiveEvent::Substitution(s) => {
            FootballLiveEventRecord::Substitution(FootballSubstitutionEventRecord {
                side_id: s.side_id.clone(),
                player_in_id: s.player_in_id.clone(),
                player_out_id: s.player_out_id.clone(),
                minute: s.minute,
            })
        }
        FootballLiveEvent::Period(p) => {
            FootballLiveEventRecord::Period(FootballPeriodEventRecord {
                period: match p.period {
                    FootballPeriod::KickOff => FootballPeriodRecord::KickOff,
                    FootballPeriod::HalfTime => FootballPeriodRecord::HalfTime,
                    FootballPeriod::SecondHalfKickOff => FootballPeriodRecord::SecondHalfKickOff,
                    FootballPeriod::FullTime => FootballPeriodRecord::FullTime,
                    FootballPeriod::ExtraTimeHalfTime => FootballPeriodRecord::ExtraTimeHalfTime,
                    FootballPeriod::ExtraTimeFullTime => FootballPeriodRecord::ExtraTimeFullTime,
                    FootballPeriod::PenaltiesComplete => FootballPeriodRecord::PenaltiesComplete,
                },
            })
        }
    }
}

fn football_live_event_from_record(rec: &FootballLiveEventRecord) -> FootballLiveEvent {
    match rec {
        FootballLiveEventRecord::Goal(g) => FootballLiveEvent::Goal(FootballGoalEvent {
            side_id: g.side_id.clone(),
            scorer_player_id: g.scorer_player_id.clone(),
            assist_player_id: g.assist_player_id.clone(),
            own_goal: g.own_goal,
            penalty: g.penalty,
            minute: g.minute,
        }),
        FootballLiveEventRecord::Card(c) => FootballLiveEvent::Card(FootballCardEvent {
            side_id: c.side_id.clone(),
            player_id: c.player_id.clone(),
            color: match c.color {
                FootballCardColorRecord::Yellow => FootballCardColor::Yellow,
                FootballCardColorRecord::Red => FootballCardColor::Red,
            },
            minute: c.minute,
        }),
        FootballLiveEventRecord::Substitution(s) => {
            FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                side_id: s.side_id.clone(),
                player_in_id: s.player_in_id.clone(),
                player_out_id: s.player_out_id.clone(),
                minute: s.minute,
            })
        }
        FootballLiveEventRecord::Period(p) => FootballLiveEvent::Period(FootballPeriodEvent {
            period: match p.period {
                FootballPeriodRecord::KickOff => FootballPeriod::KickOff,
                FootballPeriodRecord::HalfTime => FootballPeriod::HalfTime,
                FootballPeriodRecord::SecondHalfKickOff => FootballPeriod::SecondHalfKickOff,
                FootballPeriodRecord::FullTime => FootballPeriod::FullTime,
                FootballPeriodRecord::ExtraTimeHalfTime => FootballPeriod::ExtraTimeHalfTime,
                FootballPeriodRecord::ExtraTimeFullTime => FootballPeriod::ExtraTimeFullTime,
                FootballPeriodRecord::PenaltiesComplete => FootballPeriod::PenaltiesComplete,
            },
        }),
    }
}

// ---- Cricket ------------------------------------------------------------

fn cricket_live_event_to_record(event: &CricketLiveEvent) -> CricketLiveEventRecord {
    match event {
        CricketLiveEvent::Delivery(d) => {
            CricketLiveEventRecord::Delivery(cricket_delivery_to_record(d))
        }
        CricketLiveEvent::Retire(r) => CricketLiveEventRecord::Retire(CricketRetireEventRecord {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEvent::InningsStart(s) => {
            CricketLiveEventRecord::InningsStart(CricketInningsStartEventRecord {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEvent::InningsEnd(e) => {
            CricketLiveEventRecord::InningsEnd(CricketInningsEndEventRecord {
                reason: innings_end_reason_to_record(&e.reason),
            })
        }
    }
}

pub fn cricket_live_event_from_record(rec: &CricketLiveEventRecord) -> CricketLiveEvent {
    match rec {
        CricketLiveEventRecord::Delivery(d) => {
            CricketLiveEvent::Delivery(cricket_delivery_from_record(d))
        }
        CricketLiveEventRecord::Retire(r) => CricketLiveEvent::Retire(CricketRetireEvent {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEventRecord::InningsStart(s) => {
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEventRecord::InningsEnd(e) => {
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: innings_end_reason_from_record(&e.reason),
            })
        }
    }
}

fn innings_end_reason_to_record(reason: &InningsEndReason) -> InningsEndReasonRecord {
    match reason {
        InningsEndReason::AllOut => InningsEndReasonRecord::AllOut,
        InningsEndReason::OversComplete => InningsEndReasonRecord::OversComplete,
        InningsEndReason::Declared => InningsEndReasonRecord::Declared,
        InningsEndReason::TargetReached => InningsEndReasonRecord::TargetReached,
    }
}

fn innings_end_reason_from_record(rec: &InningsEndReasonRecord) -> InningsEndReason {
    match rec {
        InningsEndReasonRecord::AllOut => InningsEndReason::AllOut,
        InningsEndReasonRecord::OversComplete => InningsEndReason::OversComplete,
        InningsEndReasonRecord::Declared => InningsEndReason::Declared,
        InningsEndReasonRecord::TargetReached => InningsEndReason::TargetReached,
    }
}

fn cricket_delivery_to_record(d: &CricketDelivery) -> CricketDeliveryRecord {
    CricketDeliveryRecord {
        over: d.over,
        ball: d.ball,
        bowler_player_id: d.bowler_player_id.clone(),
        striker_player_id: d.striker_player_id.clone(),
        non_striker_player_id: d.non_striker_player_id.clone(),
        runs_off_bat: d.runs_off_bat,
        extra: d.extra.as_ref().map(cricket_delivery_extra_to_record),
        wicket: d.wicket.as_ref().map(cricket_delivery_wicket_to_record),
    }
}

fn cricket_delivery_from_record(rec: &CricketDeliveryRecord) -> CricketDelivery {
    CricketDelivery {
        over: rec.over,
        ball: rec.ball,
        bowler_player_id: rec.bowler_player_id.clone(),
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        runs_off_bat: rec.runs_off_bat,
        extra: rec.extra.as_ref().map(cricket_delivery_extra_from_record),
        wicket: rec.wicket.as_ref().map(cricket_delivery_wicket_from_record),
    }
}

fn cricket_delivery_extra_to_record(e: &CricketDeliveryExtra) -> CricketDeliveryExtraRecord {
    CricketDeliveryExtraRecord {
        kind: cricket_extra_kind_to_record(&e.kind),
        runs: e.runs,
    }
}

fn cricket_delivery_extra_from_record(rec: &CricketDeliveryExtraRecord) -> CricketDeliveryExtra {
    CricketDeliveryExtra {
        kind: cricket_extra_kind_from_record(&rec.kind),
        runs: rec.runs,
    }
}

fn cricket_extra_kind_to_record(kind: &CricketExtraKind) -> CricketExtraKindRecord {
    match kind {
        CricketExtraKind::Wide => CricketExtraKindRecord::Wide,
        CricketExtraKind::NoBall => CricketExtraKindRecord::NoBall,
        CricketExtraKind::Bye => CricketExtraKindRecord::Bye,
        CricketExtraKind::LegBye => CricketExtraKindRecord::LegBye,
        CricketExtraKind::Penalty => CricketExtraKindRecord::Penalty,
    }
}

fn cricket_extra_kind_from_record(rec: &CricketExtraKindRecord) -> CricketExtraKind {
    match rec {
        CricketExtraKindRecord::Wide => CricketExtraKind::Wide,
        CricketExtraKindRecord::NoBall => CricketExtraKind::NoBall,
        CricketExtraKindRecord::Bye => CricketExtraKind::Bye,
        CricketExtraKindRecord::LegBye => CricketExtraKind::LegBye,
        CricketExtraKindRecord::Penalty => CricketExtraKind::Penalty,
    }
}

fn cricket_delivery_wicket_to_record(w: &CricketDeliveryWicket) -> CricketDeliveryWicketRecord {
    CricketDeliveryWicketRecord {
        kind: cricket_dismissal_kind_to_record(&w.kind),
        dismissed_player_id: w.dismissed_player_id.clone(),
        bowler_player_id: w.bowler_player_id.clone(),
        fielder_player_id: w.fielder_player_id.clone(),
    }
}

fn cricket_delivery_wicket_from_record(rec: &CricketDeliveryWicketRecord) -> CricketDeliveryWicket {
    CricketDeliveryWicket {
        kind: cricket_dismissal_kind_from_record(&rec.kind),
        dismissed_player_id: rec.dismissed_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn cricket_dismissal_kind_to_record(kind: &CricketDismissalKind) -> CricketDismissalKindRecord {
    match kind {
        CricketDismissalKind::Bowled => CricketDismissalKindRecord::Bowled,
        CricketDismissalKind::Caught => CricketDismissalKindRecord::Caught,
        CricketDismissalKind::LegBeforeWicket => CricketDismissalKindRecord::LegBeforeWicket,
        CricketDismissalKind::RunOut => CricketDismissalKindRecord::RunOut,
        CricketDismissalKind::Stumped => CricketDismissalKindRecord::Stumped,
        CricketDismissalKind::HitWicket => CricketDismissalKindRecord::HitWicket,
        CricketDismissalKind::RetiredOut => CricketDismissalKindRecord::RetiredOut,
        CricketDismissalKind::RetiredHurt => CricketDismissalKindRecord::RetiredHurt,
    }
}

fn cricket_dismissal_kind_from_record(rec: &CricketDismissalKindRecord) -> CricketDismissalKind {
    match rec {
        CricketDismissalKindRecord::Bowled => CricketDismissalKind::Bowled,
        CricketDismissalKindRecord::Caught => CricketDismissalKind::Caught,
        CricketDismissalKindRecord::LegBeforeWicket => CricketDismissalKind::LegBeforeWicket,
        CricketDismissalKindRecord::RunOut => CricketDismissalKind::RunOut,
        CricketDismissalKindRecord::Stumped => CricketDismissalKind::Stumped,
        CricketDismissalKindRecord::HitWicket => CricketDismissalKind::HitWicket,
        CricketDismissalKindRecord::RetiredOut => CricketDismissalKind::RetiredOut,
        CricketDismissalKindRecord::RetiredHurt => CricketDismissalKind::RetiredHurt,
    }
}

// ---- Batch append / read / derive ---------------------------------------

/// Build the DAO-layer append input from one API-level new-event input.
/// `recorded_at` is stamped once per batch by the caller so every event in a
/// batch shares the same server-receipt time (only `occurred_at`, the
/// device's own clock, varies per event).
pub fn new_live_event_to_dao(
    input: &NewLiveEventInput,
    recorded_by_user_id: &str,
    recorded_at: &str,
) -> NewLiveEvent {
    NewLiveEvent {
        payload: live_event_input_to_record(&input.event),
        recorded_by_user_id: recorded_by_user_id.to_string(),
        occurred_at: input
            .occurred_at
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        recorded_at: recorded_at.to_string(),
    }
}

/// Build the API `LiveEvent` from a stored record. Infallible — `payload` is
/// a typed DAO enum, not JSON, so there's no parse step that can fail.
pub fn live_event_from_record(rec: &LiveEventRecord) -> LiveEvent {
    LiveEvent {
        seq: rec.seq,
        recorded_by_user_id: rec.recorded_by_user_id.clone(),
        occurred_at: parse_ts(&rec.occurred_at),
        recorded_at: parse_ts(&rec.recorded_at),
        event: live_event_payload_from_record(&rec.payload),
    }
}

/// Folds a match's live event log into its full detail. `None` if
/// `match_type` isn't a sport live scoring supports yet (only football and
/// cricket so far — see `LiveEventInput`).
///
/// `records` is whatever the DAO currently has on record, in seq order — a
/// deleted event is already absent from it and an amended one already shows
/// its corrected content, so there's no filtering pass needed here.
pub fn derive_live_detail(
    match_type: &str,
    records: &[LiveEventRecord],
    format: Option<&MatchFormatRecord>,
) -> Option<DetailedScore> {
    match match_type {
        "football" => {
            let events: Vec<(chrono::DateTime<chrono::Utc>, FootballLiveEvent)> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Football(f) => {
                        Some((parse_ts(&r.occurred_at), football_live_event_from_record(f)))
                    }
                    LiveEventPayloadRecord::Cricket(_) => None,
                })
                .collect();
            Some(DetailedScore::Football(
                crate::live_score::football::derive_detail(&events),
            ))
        }
        "cricket" => {
            let events: Vec<CricketLiveEvent> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Cricket(c) => Some(cricket_live_event_from_record(c)),
                    LiveEventPayloadRecord::Football(_) => None,
                })
                .collect();
            let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) =
                cricket_format_args(format);
            Some(DetailedScore::Cricket(
                crate::detailed_score::cricket::CricketDetail::from_events(
                    &events,
                    balls_per_over,
                    wide_is_extra_ball,
                    no_ball_is_extra_ball,
                ),
            ))
        }
        _ => None,
    }
}

/// A cricket match's configured over length and extra-ball rules — standard
/// rules (a 6-ball over, wides/no-balls re-bowled as extras) unless the match
/// configured something else (e.g. The Hundred's 5-ball over). The only
/// pieces of match format the DAO-agnostic scoring math actually needs, so
/// callers thread these three through as plain arguments rather than the
/// whole format.
pub fn cricket_format_args(format: Option<&MatchFormatRecord>) -> (u32, bool, bool) {
    match format {
        Some(MatchFormatRecord::Cricket(f)) => (
            f.balls_per_over,
            f.wide_is_extra_ball,
            f.no_ball_is_extra_ball,
        ),
        _ => (6, true, true),
    }
}

// ===========================================================================
// Notifications
// ===========================================================================

/// Collect every actor user id referenced by a notification kind, so the caller
/// can hydrate the actor `UserProfile`s in one pass.
pub fn notification_actor_id(kind: &NotificationKindRecord) -> &str {
    match kind {
        NotificationKindRecord::MatchInvitation { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::TeamInvitation { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::InvitationAccepted { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::Follow { actor_user_id } => actor_user_id,
        NotificationKindRecord::Like { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::Comment { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::Reply { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::ScoreSubmitted { actor_user_id, .. } => actor_user_id,
        NotificationKindRecord::ScoreConfirmed { actor_user_id, .. } => actor_user_id,
    }
}

/// Build the API `Notification` from a record, given the resolved actor profile
/// (already hydrated by the caller).
pub fn notification_from_record(rec: &NotificationRecord, actor: UserProfile) -> Notification {
    let kind = match &rec.kind {
        NotificationKindRecord::MatchInvitation {
            invitation_id,
            match_id,
            match_name,
            ..
        } => NotificationKind::MatchInvitation(MatchInvitationNotification {
            inviter: actor,
            invitation_id: invitation_id.clone(),
            match_id: match_id.clone(),
            match_name: match_name.clone(),
        }),
        NotificationKindRecord::TeamInvitation {
            invitation_id,
            team_id,
            team_name,
            ..
        } => NotificationKind::TeamInvitation(TeamInvitationNotification {
            inviter: actor,
            invitation_id: invitation_id.clone(),
            team_id: team_id.clone(),
            team_name: team_name.clone(),
        }),
        NotificationKindRecord::InvitationAccepted {
            invitation_id,
            context,
            ..
        } => NotificationKind::InvitationAccepted(InvitationAcceptedNotification {
            accepted_by: actor,
            invitation_id: invitation_id.clone(),
            context: invitation_context_from_record(context),
        }),
        NotificationKindRecord::Follow { .. } => {
            NotificationKind::Follow(FollowNotification { follower: actor })
        }
        NotificationKindRecord::Like {
            match_id,
            match_name,
            ..
        } => NotificationKind::Like(LikeNotification {
            liked_by: actor,
            match_id: match_id.clone(),
            match_name: match_name.clone(),
        }),
        NotificationKindRecord::Comment {
            match_id,
            comment_id,
            preview,
            ..
        } => NotificationKind::Comment(CommentNotification {
            commenter: actor,
            match_id: match_id.clone(),
            comment_id: comment_id.clone(),
            preview: preview.clone(),
        }),
        NotificationKindRecord::Reply {
            match_id,
            comment_id,
            parent_comment_id,
            preview,
            ..
        } => NotificationKind::Reply(ReplyNotification {
            replier: actor,
            match_id: match_id.clone(),
            comment_id: comment_id.clone(),
            parent_comment_id: parent_comment_id.clone(),
            preview: preview.clone(),
        }),
        NotificationKindRecord::ScoreSubmitted {
            match_id,
            match_name,
            submission_id,
            needs_confirmation,
            ..
        } => NotificationKind::ScoreSubmitted(ScoreSubmittedNotification {
            submitted_by: actor,
            match_id: match_id.clone(),
            match_name: match_name.clone(),
            submission_id: submission_id.clone(),
            needs_confirmation: *needs_confirmation,
        }),
        NotificationKindRecord::ScoreConfirmed {
            match_id,
            match_name,
            submission_id,
            ..
        } => NotificationKind::ScoreConfirmed(ScoreConfirmedNotification {
            confirmed_by: actor,
            match_id: match_id.clone(),
            match_name: match_name.clone(),
            submission_id: submission_id.clone(),
        }),
    };
    Notification {
        id: rec.id.clone(),
        is_read: rec.is_read,
        created_at: parse_ts(&rec.created_at),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    /// Compared via JSON rather than a derived `PartialEq` (the API types
    /// don't derive it) so this doesn't need extra derives just for testing.
    #[test]
    fn football_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            FootballLiveEvent::Goal(FootballGoalEvent {
                side_id: "riverside".into(),
                scorer_player_id: Some("alvarez".into()),
                assist_player_id: None,
                own_goal: true,
                penalty: false,
                minute: Some(63),
            }),
            FootballLiveEvent::Card(FootballCardEvent {
                side_id: "oak_park".into(),
                player_id: "khan".into(),
                color: FootballCardColor::Red,
                minute: None,
            }),
            FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                side_id: "oak_park".into(),
                player_in_id: "moreno".into(),
                player_out_id: "khan".into(),
                minute: Some(70),
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeFullTime,
            }),
        ];

        for event in events {
            let input = LiveEventInput::Football(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn cricket_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            CricketLiveEvent::Delivery(CricketDelivery {
                over: 4,
                ball: 3,
                bowler_player_id: "patel".into(),
                striker_player_id: "sharma".into(),
                non_striker_player_id: "verma".into(),
                runs_off_bat: 0,
                extra: Some(CricketDeliveryExtra {
                    kind: CricketExtraKind::Wide,
                    runs: 1,
                }),
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Caught,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: Some("cole".into()),
                }),
            }),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: false,
            }),
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: InningsEndReason::Declared,
            }),
        ];

        for event in events {
            let input = LiveEventInput::Cricket(event);
            let original_json = input.to_json();
            let round_tripped = live_event_payload_from_record(&live_event_input_to_record(&input));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn match_format_round_trips_through_dao_mirror() {
        let formats = vec![
            MatchFormat::Football(FootballFormat {
                half_length_minutes: 40,
                num_halves: 2,
                extra_time: true,
                extra_time_half_length_minutes: Some(15),
                penalties: true,
            }),
            MatchFormat::Cricket(CricketFormat {
                overs_per_innings: None,
                innings_per_side: 2,
                balls_per_over: 6,
                no_ball_penalty_runs: 2,
                wide_penalty_runs: 1,
                wide_is_extra_ball: true,
                no_ball_is_extra_ball: false,
                free_hit_after_no_ball: false,
            }),
        ];

        for format in formats {
            let original_json = format.to_json();
            let round_tripped = match_format_from_record(&match_format_to_record(&format));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    #[test]
    fn score_round_trips_through_dao_mirror() {
        let scores = vec![
            Score::Simple(SimpleScore {
                entries: vec![
                    SimpleScoreEntry {
                        side_id: "side_red".into(),
                        points: 3,
                    },
                    SimpleScoreEntry {
                        side_id: "side_blue".into(),
                        points: 1,
                    },
                ],
            }),
            Score::Sets(SetsScore {
                entries: vec![
                    SetsScoreEntry {
                        side_id: "side_red".into(),
                        sets: vec![6, 4, 7],
                    },
                    SetsScoreEntry {
                        side_id: "side_blue".into(),
                        sets: vec![4, 6, 5],
                    },
                ],
            }),
            Score::Cricket(CricketScore {
                innings: vec![
                    CricketScoreInnings {
                        batting_side_id: "warriors".into(),
                        bowling_side_id: "mill_lane".into(),
                        runs: 180,
                        wickets: 6,
                        overs: Overs {
                            overs: 20,
                            balls: 0,
                        },
                        declared: false,
                    },
                    CricketScoreInnings {
                        batting_side_id: "mill_lane".into(),
                        bowling_side_id: "warriors".into(),
                        runs: 165,
                        wickets: 10,
                        overs: Overs {
                            overs: 19,
                            balls: 3,
                        },
                        declared: false,
                    },
                ],
            }),
        ];

        for score in scores {
            let original_json = score.to_json();
            let round_tripped = score_from_record(&score_to_record(&score));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }
}
