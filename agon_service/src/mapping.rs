//! Boundary mapping between API models (poem-openapi) and DAO records.
//!
//! The DAO owns its own record types and shares nothing with the API. This
//! module is the single place that translates between the two, so handlers stay
//! thin and the coupling lives in one file.

use poem::error::InternalServerError;

use crate::detailed_score::DetailedScore;
use crate::membership::{
    ExternalMember, Invitation, InvitationContext, InvitationKind, InvitationMatchContext,
    InvitationStatus, InvitationTeamContext, Member, TokenInvitation, UserInvitation, UserMember,
};
use crate::notification::{
    CommentNotification, FollowNotification, InvitationAcceptedNotification, LikeNotification,
    MatchInvitationNotification, Notification, NotificationKind, TeamInvitationNotification,
};
use crate::team::{Team, TeamListItem, TeamMember, TeamRole};
use crate::{
    Comment, ConfirmedScore, Location, Match, MatchPlayer, MatchSide, MatchSocial, MatchStatus,
    MatchType, PendingScore, Photo, Score, ScoreConfirmation, ScoreResponseKind, ScoreSubmission,
    ScoreSubmissionResponse, ScoreSubmissionStatus, SetsScore, SetsScoreEntry, SimpleScore,
    SimpleScoreEntry, UserProfile, UserSportStats,
};
use agon_core::dao::error::DaoError;
use agon_core::dao::records::{
    CommentRecord, ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationContextRecord,
    InvitationKindRecord, InvitationRecord, MatchDetailedScoreRecord, MatchLikeRecord,
    MatchPlayerRecord, MatchRecord, MatchSideRecord, NotificationKindRecord, NotificationRecord,
    PendingScoreRecord, ScoreConfirmationRecord, ScoreRecord, ScoreResponseRecord,
    ScoreSubmissionRecord, SetsScoreEntryRecord, SimpleScoreEntryRecord, TeamMemberRecord,
    TeamRecord, UserRecord, UserSportStatsRecord,
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

/// Build the public `UserProfile` from a stored user record, its per-sport
/// stats, and the viewer-relative follow flag.
pub fn user_profile_from_record(
    user: &UserRecord,
    stats: &[UserSportStatsRecord],
    is_followed_by_me: bool,
) -> UserProfile {
    UserProfile {
        id: user.id.clone(),
        name: user.name.clone(),
        profile_image: user.profile_image_url.as_ref().map(|url| Photo {
            image_url: url.clone(),
        }),
        stats: stats.iter().map(sport_stats_from_record).collect(),
        follower_count: user.follower_count as u32,
        following_count: user.following_count as u32,
        is_followed_by_me,
    }
}

/// Map a stored per-sport stats record to the API model, deriving win %.
pub fn sport_stats_from_record(rec: &UserSportStatsRecord) -> UserSportStats {
    let win_percentage = if rec.matches_played == 0 {
        0.0
    } else {
        (rec.wins as f32 / rec.matches_played as f32) * 100.0
    };
    UserSportStats {
        match_type: match_type_from_tag(&rec.match_type),
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
/// optional embedded invitation.
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
pub fn detailed_score_to_record(ds: &DetailedScore) -> MatchDetailedScoreRecord {
    let sport = match ds {
        DetailedScore::Football(_) => "football",
        DetailedScore::Cricket(_) => "cricket",
    }
    .to_string();
    let detail = ds.to_json().unwrap_or(serde_json::Value::Null);
    MatchDetailedScoreRecord { sport, detail }
}

/// Parse a stored detailed-score record back into the `DetailedScore` union.
/// Returns None if the stored blob can't be parsed (treated as "no detail").
pub fn detailed_score_from_record(rec: &MatchDetailedScoreRecord) -> Option<DetailedScore> {
    DetailedScore::parse_from_json(Some(rec.detail.clone())).ok()
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
    };
    Notification {
        id: rec.id.clone(),
        is_read: rec.is_read,
        created_at: parse_ts(&rec.created_at),
        kind,
    }
}
