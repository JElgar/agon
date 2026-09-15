//! Boundary mapping between API models (poem-openapi) and DAO records.
//!
//! The DAO owns its own record types and shares nothing with the API. This
//! module is the single place that translates between the two, so handlers stay
//! thin and the coupling lives in one file.

use poem::error::InternalServerError;
use tracing::error;

use crate::detailed_score::cricket::{Overs, balls_to_overs};
use crate::live_score::{
    LiveEvent, LiveEventInput, NewLiveEventInput, cricket::CricketLiveEvent,
    football::FootballLiveEvent, netball::NetballLiveEvent,
};
use crate::match_format::MatchFormat;
use crate::membership::{
    ExternalMember, Invitation, InvitationContext, InvitationKind, InvitationMatchContext,
    InvitationStatus, InvitationTeamContext, JoinLink, JoinLinkScope, MatchPlayerRole, Member,
    TokenInvitation, UserInvitation, UserMember,
};
use crate::notification::{
    CommentNotification, FollowNotification, InvitationAcceptedNotification, LikeNotification,
    MatchInvitationNotification, Notification, NotificationKind, ReplyNotification,
    ScoreConfirmedNotification, ScoreSubmittedNotification, TeamInvitationNotification,
    TeamMatchJoinableNotification,
};
use crate::team::{AssignableTeamRole, Team, TeamListItem, TeamMember, TeamRole};
use crate::{
    BestBowlingFigures, BestFigure, Comment, ConfirmedScore, CricketPlayerStats, CricketScore,
    DevicePlatform, FeedMatch, FootballPlayerStats, FootballScore, GenericPlayerStats, Location,
    Match, MatchOutcome, MatchPlayer, MatchSide, MatchSocial, MatchStatus, MatchType, NetballScore,
    PendingScore, Photo, RosterPreviewPlayer, Score, ScoreConfirmation, ScoreResponseKind,
    ScoreSubmission, ScoreSubmissionResponse, ScoreSubmissionStatus, SearchMatch, SetsScore,
    SimpleScore, UserProfile, UserStats,
};
use agon_core::dao::error::DaoError;
use agon_core::dao::live_score_ops::NewLiveEvent;
use agon_core::dao::records::{
    BestBowlingFiguresRecord, BestFigureRecord, CommentRecord, ConfirmedScoreRecord,
    CricketStatsRecord, DevicePlatform as DevicePlatformRecord, EmbeddedInvitationRecord,
    FootballStatsRecord, GenericSportStatsRecord, InvitationContextRecord, InvitationKindRecord,
    InvitationRecord, JoinLinkRecord, JoinLinkScopeRecord, LiveEventPayloadRecord, LiveEventRecord,
    MatchFormatRecord, MatchLikeRecord, MatchPlayerRecord,
    MatchPlayerRole as MatchPlayerRoleRecord, MatchRecord, MatchScoreRecord, MatchSideRecord,
    NotificationKindRecord, NotificationRecord, PendingScoreRecord, ScoreConfirmationRecord,
    ScoreRecord, ScoreResponseRecord, ScoreSubmissionRecord, TeamMemberRecord, TeamRecord,
    UserRecord, UserStatsRecord,
};

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
/// Logs the underlying error first — `InternalServerError` on its own gives
/// the client (and, until now, our own logs) nothing but a bare 500, which
/// makes anything unexpected here unreproducible after the fact.
pub fn dao_internal(err: DaoError) -> poem::Error {
    error!("DAO error: {err}");
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
        "netball" => MatchType::Netball,
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
        MatchType::Netball => "netball",
        MatchType::Other => "other",
    }
}

/// Map the API's device-platform enum to the DAO-owned one.
pub fn device_platform_to_record(p: &DevicePlatform) -> DevicePlatformRecord {
    match p {
        DevicePlatform::Web => DevicePlatformRecord::Web,
        DevicePlatform::Android => DevicePlatformRecord::Android,
        DevicePlatform::Ios => DevicePlatformRecord::Ios,
    }
}

/// Build the public `UserProfile` from a stored user record (its inline
/// per-sport `stats` map) and the viewer-relative follow flag.
pub fn user_profile_from_record(user: &UserRecord, is_followed_by_me: bool) -> UserProfile {
    UserProfile {
        id: user.id.clone(),
        name: user.name.clone(),
        profile_image: user.profile_image_url.as_ref().map(|url| Photo {
            image_url: url.clone(),
            asset_id: None,
        }),
        stats: user_stats_from_record(&user.stats),
        follower_count: user.follower_count as u32,
        following_count: user.following_count as u32,
        is_followed_by_me,
    }
}

/// Map the stored, one-field-per-sport `UserStatsRecord` to the public
/// `UserStats` — `None` for any sport with no stored entry, same as the DAO
/// side.
pub fn user_stats_from_record(stats: &UserStatsRecord) -> UserStats {
    UserStats {
        cricket: stats.cricket.as_ref().map(cricket_stats_from_record),
        football: stats.football.as_ref().map(football_stats_from_record),
        tennis: stats.tennis.as_ref().map(generic_stats_from_record),
        badminton: stats.badminton.as_ref().map(generic_stats_from_record),
        squash: stats.squash.as_ref().map(generic_stats_from_record),
        table_tennis: stats.table_tennis.as_ref().map(generic_stats_from_record),
        netball: stats.netball.as_ref().map(generic_stats_from_record),
        other: stats.other.as_ref().map(generic_stats_from_record),
    }
}

/// Map the counters common to every sport, deriving win % from matches
/// played. Shared by every per-sport mapping function below.
fn generic_stats_from_record(rec: &GenericSportStatsRecord) -> GenericPlayerStats {
    let win_percentage = if rec.matches_played == 0 {
        None
    } else {
        Some((rec.wins as f32 / rec.matches_played as f32) * 100.0)
    };
    GenericPlayerStats {
        matches_played: rec.matches_played as i32,
        wins: rec.wins as i32,
        draws: rec.draws as i32,
        losses: rec.losses as i32,
        win_percentage,
    }
}

fn cricket_stats_from_record(rec: &CricketStatsRecord) -> CricketPlayerStats {
    let strike_rate =
        (rec.balls_faced > 0).then(|| (rec.runs as f32 / rec.balls_faced as f32) * 100.0);
    let batting_average = (rec.dismissals > 0).then(|| rec.runs as f32 / rec.dismissals as f32);
    let economy =
        (rec.balls_bowled > 0).then(|| rec.runs_conceded as f32 / (rec.balls_bowled as f32 / 6.0));
    CricketPlayerStats {
        common: generic_stats_from_record(&rec.common),
        runs: rec.runs as i32,
        wickets: rec.wickets as i32,
        fours: rec.fours as i32,
        sixes: rec.sixes as i32,
        balls_faced: rec.balls_faced as i32,
        dismissals: rec.dismissals as i32,
        catches: rec.catches as i32,
        runs_conceded: rec.runs_conceded as i32,
        overs_bowled: balls_to_overs(rec.balls_bowled as u32, 6),
        strike_rate,
        batting_average,
        economy,
        best_runs: rec.best_runs.as_ref().map(best_figure_from_record),
        best_bowling: rec.best_bowling.as_ref().map(best_bowling_from_record),
    }
}

fn football_stats_from_record(rec: &FootballStatsRecord) -> FootballPlayerStats {
    FootballPlayerStats {
        common: generic_stats_from_record(&rec.common),
        goals: rec.goals as i32,
        assists: rec.assists as i32,
        best_goals: rec.best_goals.as_ref().map(best_figure_from_record),
        best_goal_contributions: rec
            .best_goal_contributions
            .as_ref()
            .map(best_figure_from_record),
    }
}

/// Map a stored personal-best entry to the API model.
fn best_figure_from_record(b: &BestFigureRecord) -> BestFigure {
    BestFigure {
        value: b.value as i32,
        match_id: b.match_id.clone(),
    }
}

/// Map a stored best-bowling-spell entry to the API model.
fn best_bowling_from_record(b: &BestBowlingFiguresRecord) -> BestBowlingFigures {
    BestBowlingFigures {
        wickets: b.wickets as i32,
        runs_conceded: b.runs_conceded as i32,
        // The exact figure from that one match — no conversion/assumption
        // needed, unlike `overs_bowled` below (a cross-match total).
        overs: Overs {
            overs: b.overs.overs,
            balls: b.overs.balls,
        },
        match_id: b.match_id.clone(),
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
            entries: entries.clone(),
        }),
        ScoreRecord::Sets { entries } => Score::Sets(SetsScore {
            entries: entries.clone(),
        }),
        ScoreRecord::Cricket(rec) => Score::Cricket(crate::sports::cricket::score_from_record(rec)),
        ScoreRecord::Football(rec) => Score::Football(crate::sports::football::score_from_record(rec)),
        ScoreRecord::Netball(rec) => Score::Netball(crate::sports::netball::score_from_record(rec)),
    }
}

pub fn score_to_record(score: &Score) -> ScoreRecord {
    match score {
        Score::Simple(s) => ScoreRecord::Simple {
            entries: s.entries.clone(),
        },
        Score::Sets(s) => ScoreRecord::Sets {
            entries: s.entries.clone(),
        },
        Score::Cricket(s) => ScoreRecord::Cricket(crate::sports::cricket::score_to_record(s)),
        Score::Football(s) => ScoreRecord::Football(crate::sports::football::score_to_record(s)),
        Score::Netball(s) => ScoreRecord::Netball(crate::sports::netball::score_to_record(s)),
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
        "owner" => TeamRole::Owner,
        "admin" => TeamRole::Admin,
        _ => TeamRole::Member,
    }
}

pub fn team_role_str(r: &TeamRole) -> &'static str {
    match r {
        TeamRole::Owner => "owner",
        TeamRole::Admin => "admin",
        TeamRole::Member => "member",
    }
}

/// The stored string for a role assignable via `PATCH
/// /teams/{team_id}/members/{member_id}` — never `"owner"`, which
/// `AssignableTeamRole` excludes at the type level (see its doc comment).
pub fn assignable_team_role_str(r: &AssignableTeamRole) -> &'static str {
    match r {
        AssignableTeamRole::Admin => "admin",
        AssignableTeamRole::Member => "member",
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

pub fn team_from_records(team: &TeamRecord, is_followed_by_me: bool) -> Team {
    Team {
        id: team.id.clone(),
        name: team.name.clone(),
        logo: team.logo_url.as_ref().map(|url| Photo {
            image_url: url.clone(),
            asset_id: None,
        }),
        invite_token: team.invite_token.clone(),
        follower_count: team.follower_count as u32,
        is_followed_by_me,
    }
}

pub fn team_list_item_from_record(team: &TeamRecord, is_followed_by_me: bool) -> TeamListItem {
    TeamListItem {
        id: team.id.clone(),
        name: team.name.clone(),
        logo: team.logo_url.as_ref().map(|url| Photo {
            image_url: url.clone(),
            asset_id: None,
        }),
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
        role: match_player_role_from_record(rec.role),
    }
}

/// See `agon_core::dao::records::MatchPlayerRole`'s doc comment for why this
/// is its own type rather than a reuse of the team role.
pub fn match_player_role_from_record(rec: MatchPlayerRoleRecord) -> MatchPlayerRole {
    match rec {
        MatchPlayerRoleRecord::Owner => MatchPlayerRole::Owner,
        MatchPlayerRoleRecord::Admin => MatchPlayerRole::Admin,
        MatchPlayerRoleRecord::Player => MatchPlayerRole::Player,
    }
}

pub fn match_player_role_to_record(role: MatchPlayerRole) -> MatchPlayerRoleRecord {
    match role {
        MatchPlayerRole::Owner => MatchPlayerRoleRecord::Owner,
        MatchPlayerRole::Admin => MatchPlayerRoleRecord::Admin,
        MatchPlayerRole::Player => MatchPlayerRoleRecord::Player,
    }
}

pub fn join_link_scope_from_record(rec: &JoinLinkScopeRecord) -> JoinLinkScope {
    JoinLinkScope {
        side_ids: rec.side_ids.clone(),
        allow_unassigned: rec.allow_unassigned,
    }
}

pub fn join_link_scope_to_record(scope: &JoinLinkScope) -> JoinLinkScopeRecord {
    JoinLinkScopeRecord {
        side_ids: scope.side_ids.clone(),
        allow_unassigned: scope.allow_unassigned,
    }
}

/// Build the API `JoinLink` from a record. `match_id` falls back to empty for
/// a (currently impossible — team join-links aren't built yet) non-match
/// context, rather than panicking on what would still just be a read path.
pub fn join_link_from_record(rec: &JoinLinkRecord) -> JoinLink {
    let match_id = match &rec.context {
        InvitationContextRecord::Match { match_id, .. } => match_id.clone(),
        InvitationContextRecord::Team { .. } => String::new(),
    };
    JoinLink {
        id: rec.id.clone(),
        match_id,
        token: rec.token.clone(),
        scope: join_link_scope_from_record(&rec.scope),
        created_by_user_id: rec.created_by_user_id.clone(),
        created_at: parse_ts(&rec.created_at),
        revoked_at: parse_ts_opt(&rec.revoked_at),
    }
}

pub fn match_side_from_record(rec: &MatchSideRecord) -> MatchSide {
    MatchSide {
        id: rec.side_id.clone(),
        team_id: rec.team_id.clone(),
        name: rec.name.clone(),
        max_players: rec.max_players,
        team_join_enabled: rec.team_join_enabled,
        // Live-overwritten for `Match` (`Api::resolve_side_names`); left as
        // the denormalized cache value for a feed's `FeedMatch`/a search
        // hit's `SearchMatch`, same as `roster_preview` below.
        player_count: rec.player_count,
        // Filled in afterward, alongside `name`: live team-meta lookup for
        // `Match` (`Api::resolve_side_names`), or the same batch for a feed's
        // `FeedMatch`/a search hit's `SearchMatch`
        // (`Api::resolve_side_names_from_cache`).
        team_logo: None,
        // Filled in afterward, alongside `team_logo`: same live/cached
        // lookup as above.
        team_name: None,
        // Filled in afterward: live from `players` for `Match`
        // (`Api::resolve_side_names`), or from the denormalized cache for a
        // feed's `FeedMatch` (`feed_roster_preview`, below).
        roster_preview: None,
    }
}

/// Build a feed side's `roster_preview` from the denormalized cache
/// (`MatchSideRecord::player_count`/`roster_preview`) — the feed's
/// counterpart to `Api::resolve_side_names`'s live computation, since a feed
/// match never fetches the full player collection. `users` is the page-wide
/// `batch_get_users` map the caller already built (same one
/// `known_participants` hydrates from); a linked player's live name/avatar
/// comes from there, an external player's stored `display_name` directly.
///
/// `None` when the cache is empty — either nobody's assigned yet, or the side
/// exceeded `ROSTER_PREVIEW_CAP` players at its last refresh (by construction
/// the cache is never a partial peek, see `MatchSideRecord::roster_preview`'s
/// doc comment) — callers should fall back to `name`/`team_id`.
pub fn feed_roster_preview(
    side: &MatchSideRecord,
    users: &std::collections::HashMap<String, UserRecord>,
) -> Option<Vec<RosterPreviewPlayer>> {
    if side.roster_preview.is_empty() {
        return None;
    }
    Some(
        side.roster_preview
            .iter()
            .map(|p| roster_preview_player(p.user_id.as_deref(), p.display_name.as_deref(), users))
            .collect(),
    )
}

/// Build one `RosterPreviewPlayer` for a player identified either by a
/// linked account (`user_id`, live name/avatar looked up in `users`) or an
/// external display name. Shared by [`feed_roster_preview`] (a side's cached
/// roster preview) and `Api`'s live-scoring current-batter resolution
/// (`batch_get_match_players`) — same "prefer a live user record over
/// whatever's stored on the player" rule either way.
pub fn roster_preview_player(
    user_id: Option<&str>,
    display_name: Option<&str>,
    users: &std::collections::HashMap<String, UserRecord>,
) -> RosterPreviewPlayer {
    match user_id {
        Some(uid) => {
            let record = users.get(uid);
            RosterPreviewPlayer {
                user_id: Some(uid.to_string()),
                name: record.map(|u| u.name.clone()).unwrap_or_default(),
                avatar_url: record.and_then(|u| u.profile_image_url.clone()),
            }
        }
        None => RosterPreviewPlayer {
            user_id: None,
            name: display_name.unwrap_or_default().to_string(),
            avatar_url: None,
        },
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
        allow_unassigned: rec.allow_unassigned,
        location: rec.location.as_ref().map(|l| Location {
            latitude: l.latitude,
            longitude: l.longitude,
        }),
        header_photos: rec
            .header_photos
            .iter()
            .map(|p| Photo {
                image_url: p.url.clone(),
                asset_id: Some(p.asset_id.clone()),
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
        // Set by the caller right after this call, from the same aggregate
        // this function already consumed (see `caller_match_role`'s call
        // sites in `main.rs`) — a placeholder here, since this pure mapping
        // function has no viewer to resolve it against.
        viewer_role: None,
        viewer_team_join_side_ids: None,
    }
}

/// Build the feed's `FeedMatch` from a match summary (meta + sides, no
/// players) and the viewer's already-resolved `known_participants` — the
/// feed's equivalent of [`match_from_records`], but never touches the
/// player roster.
pub fn feed_match_from_records(
    rec: &MatchRecord,
    sides: &[MatchSideRecord],
    users: &std::collections::HashMap<String, UserRecord>,
    known_participants: Vec<UserProfile>,
    known_participants_count: u32,
    viewer_side_id: Option<String>,
    i_liked: bool,
) -> FeedMatch {
    FeedMatch {
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
            .header_photos
            .iter()
            .map(|p| Photo {
                image_url: p.url.clone(),
                asset_id: Some(p.asset_id.clone()),
            })
            .collect(),
        sides: sides
            .iter()
            .map(|s| {
                let mut side = match_side_from_record(s);
                side.roster_preview = feed_roster_preview(s, users);
                side
            })
            .collect(),
        known_participants,
        known_participants_count,
        viewer_side_id,
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

/// Build a search hit's `SearchMatch` from a match summary (meta + sides, no
/// players) — the search counterpart to [`feed_match_from_records`]. `users`
/// hydrates side `roster_preview` entries' live name/avatar, same as the
/// feed. `outcome` is the search client's already-resolved result for the
/// queried participant, if any (see `SearchClient::search_matches`).
pub fn search_match_from_records(
    rec: &MatchRecord,
    sides: &[MatchSideRecord],
    users: &std::collections::HashMap<String, UserRecord>,
    outcome: Option<agon_core::search::MatchOutcome>,
    i_liked: bool,
) -> SearchMatch {
    SearchMatch {
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
            .header_photos
            .iter()
            .map(|p| Photo {
                image_url: p.url.clone(),
                asset_id: Some(p.asset_id.clone()),
            })
            .collect(),
        sides: sides
            .iter()
            .map(|s| {
                let mut side = match_side_from_record(s);
                side.roster_preview = feed_roster_preview(s, users);
                side
            })
            .collect(),
        outcome: outcome.map(match_outcome_from_search),
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

/// Map the search client's outcome enum to the API's.
fn match_outcome_from_search(outcome: agon_core::search::MatchOutcome) -> MatchOutcome {
    match outcome {
        agon_core::search::MatchOutcome::Won => MatchOutcome::Won,
        agon_core::search::MatchOutcome::Lost => MatchOutcome::Lost,
        agon_core::search::MatchOutcome::Draw => MatchOutcome::Draw,
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
// Live-scoring score record: MatchScoreRecord (DAO) <-> Score (API). Reuses
// score_from_record/score_to_record directly — a match's live-scoring score
// is the exact same type as its confirmed/pending score, just a separate
// DynamoDB item for write-frequency/stream-isolation reasons (see
// `MatchScoreRecord`'s doc comment).
// ===========================================================================

/// Wrap a `Score` into the `(sport, score)` record shape. The sport tag
/// mirrors the union variant so a read can pick the right variant.
pub fn match_score_to_record(score: &Score, last_seq: Option<u32>) -> MatchScoreRecord {
    let sport = match score {
        Score::Football(_) => "football",
        Score::Cricket(_) => "cricket",
        Score::Netball(_) => "netball",
        Score::Simple(_) => "simple",
        Score::Sets(_) => "sets",
    }
    .to_string();
    MatchScoreRecord {
        sport,
        score: score_to_record(score),
        last_seq,
    }
}

pub fn match_score_from_record(rec: &MatchScoreRecord) -> Score {
    score_from_record(&rec.score)
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
        MatchFormat::Football(f) => MatchFormatRecord::Football(crate::sports::football::format_to_record(f)),
        MatchFormat::Cricket(f) => MatchFormatRecord::Cricket(crate::sports::cricket::format_to_record(f)),
        MatchFormat::Netball(f) => MatchFormatRecord::Netball(crate::sports::netball::format_to_record(f)),
    }
}

/// Build the API `MatchFormat` from a stored record. Infallible — `format`
/// is a typed DAO enum, not JSON, so there's no parse step that can fail.
pub fn match_format_from_record(rec: &MatchFormatRecord) -> MatchFormat {
    match rec {
        MatchFormatRecord::Football(f) => MatchFormat::Football(crate::sports::football::format_from_record(f)),
        MatchFormatRecord::Cricket(f) => MatchFormat::Cricket(crate::sports::cricket::format_from_record(f)),
        MatchFormatRecord::Netball(f) => MatchFormat::Netball(crate::sports::netball::format_from_record(f)),
    }
}

/// The sport tag a `MatchFormat` is for, mirroring the union variant (same
/// convention as `live_event_sport_tag`) — used to reject a format that
/// doesn't match the match's own sport.
pub fn match_format_sport_tag(fmt: &MatchFormat) -> &'static str {
    match fmt {
        MatchFormat::Football(_) => "football",
        MatchFormat::Cricket(_) => "cricket",
        MatchFormat::Netball(_) => "netball",
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
/// `match_score_to_record`).
pub fn live_event_sport_tag(event: &LiveEventInput) -> &'static str {
    match event {
        LiveEventInput::Football(_) => "football",
        LiveEventInput::Cricket(_) => "cricket",
        LiveEventInput::Netball(_) => "netball",
    }
}

pub fn live_event_input_to_record(event: &LiveEventInput) -> LiveEventPayloadRecord {
    match event {
        LiveEventInput::Football(f) => {
            LiveEventPayloadRecord::Football(crate::sports::football::live_event_to_record(f))
        }
        LiveEventInput::Cricket(c) => {
            LiveEventPayloadRecord::Cricket(crate::sports::cricket::live_event_to_record(c))
        }
        LiveEventInput::Netball(n) => {
            LiveEventPayloadRecord::Netball(crate::sports::netball::live_event_to_record(n))
        }
    }
}

pub fn live_event_payload_from_record(rec: &LiveEventPayloadRecord) -> LiveEventInput {
    match rec {
        LiveEventPayloadRecord::Football(f) => {
            LiveEventInput::Football(crate::sports::football::live_event_from_record(f))
        }
        LiveEventPayloadRecord::Cricket(c) => {
            LiveEventInput::Cricket(crate::sports::cricket::live_event_from_record(c))
        }
        LiveEventPayloadRecord::Netball(n) => {
            LiveEventInput::Netball(crate::sports::netball::live_event_from_record(n))
        }
    }
}

// Cricket's, football's and netball's API<->DAO mapping (score/format/
// live-event) have moved to `crate::sports::{cricket,football,netball}` —
// see their module doc comments.

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
pub fn derive_live_score(
    match_type: &str,
    records: &[LiveEventRecord],
    format: Option<&MatchFormatRecord>,
) -> Option<Score> {
    match match_type {
        "football" => {
            let events: Vec<(chrono::DateTime<chrono::Utc>, FootballLiveEvent)> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Football(f) => Some((
                        parse_ts(&r.occurred_at),
                        crate::sports::football::live_event_from_record(f),
                    )),
                    LiveEventPayloadRecord::Cricket(_) | LiveEventPayloadRecord::Netball(_) => None,
                })
                .collect();
            Some(Score::Football(FootballScore::from_events(&events)))
        }
        "cricket" => {
            let events: Vec<(chrono::DateTime<chrono::Utc>, CricketLiveEvent)> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Cricket(c) => Some((
                        parse_ts(&r.occurred_at),
                        crate::sports::cricket::live_event_from_record(c),
                    )),
                    LiveEventPayloadRecord::Football(_) | LiveEventPayloadRecord::Netball(_) => {
                        None
                    }
                })
                .collect();
            let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) =
                crate::sports::cricket::format_args(format);
            Some(Score::Cricket(CricketScore::from_events(
                &events,
                balls_per_over,
                wide_is_extra_ball,
                no_ball_is_extra_ball,
            )))
        }
        "netball" => {
            let events: Vec<(chrono::DateTime<chrono::Utc>, NetballLiveEvent)> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Netball(n) => Some((
                        parse_ts(&r.occurred_at),
                        crate::sports::netball::live_event_from_record(n),
                    )),
                    LiveEventPayloadRecord::Football(_) | LiveEventPayloadRecord::Cricket(_) => {
                        None
                    }
                })
                .collect();
            Some(Score::Netball(NetballScore::from_events(&events)))
        }
        _ => None,
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
        NotificationKindRecord::TeamMatchJoinable { actor_user_id, .. } => actor_user_id,
    }
}

/// Placeholder profile for a notification whose actor id no longer resolves to
/// a live user record — the account was deleted after the notification was
/// created. (There's no account-deletion feature yet, so this can't happen
/// today, but a notification's actor reference outlives the account by
/// design — see `Notification`'s doc comment — so a read-time fallback here
/// is the whole fix; deleting an account never needs to touch or clean up any
/// notification.) The id is preserved rather than scrubbed, so a client that
/// links through to it (Follow's "View profile"/follow-back button) hits the
/// same "user not found" it already handles for any other stale profile
/// link, rather than a broken/empty route.
pub fn deleted_user_profile(actor_id: &str) -> UserProfile {
    UserProfile {
        id: actor_id.to_string(),
        name: "Deleted user".to_string(),
        profile_image: None,
        stats: UserStats {
            cricket: None,
            football: None,
            tennis: None,
            badminton: None,
            squash: None,
            table_tennis: None,
            netball: None,
            other: None,
        },
        follower_count: 0,
        following_count: 0,
        is_followed_by_me: false,
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
        NotificationKindRecord::TeamMatchJoinable {
            team_id,
            team_name,
            match_id,
            match_name,
            ..
        } => NotificationKind::TeamMatchJoinable(TeamMatchJoinableNotification {
            organizer: actor,
            team_id: team_id.clone(),
            team_name: team_name.clone(),
            match_id: match_id.clone(),
            match_name: match_name.clone(),
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
    use std::collections::HashMap;

    use super::*;
    use crate::detailed_score::cricket::{
        CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDeliveryExtra,
        CricketDeliveryWicket, CricketDismissal, CricketDismissalKind, CricketExtraKind,
        CricketExtras, CricketFallOfWicket, NextBallContext,
    };
    use crate::detailed_score::football::{
        FootballCardColor, FootballCardEvent, FootballGoalEvent, FootballPenaltyShootoutKick,
        FootballPeriod, FootballSubstitutionEvent,
    };
    use crate::detailed_score::netball::{
        NetballFoulEvent, NetballFoulKind, NetballGoalEvent, NetballPeriod, NetballPosition,
    };
    use crate::live_score::cricket::{
        CricketInningsEndEvent, CricketInningsStartEvent, CricketRetireEvent, InningsEndReason,
    };
    use crate::live_score::football::FootballPeriodEvent;
    use crate::live_score::netball::NetballPeriodEvent;
    use crate::match_format::{CricketFormat, FootballFormat, NetballFormat};
    use crate::CricketScoreInnings;
    use poem_openapi::types::ToJSON;

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
                occurred_at: Some(parse_ts("2024-05-01T20:03:00.000Z")),
            }),
            FootballLiveEvent::Card(FootballCardEvent {
                side_id: "oak_park".into(),
                player_id: "khan".into(),
                color: FootballCardColor::Red,
                minute: None,
                occurred_at: None,
            }),
            FootballLiveEvent::Substitution(FootballSubstitutionEvent {
                side_id: "oak_park".into(),
                player_in_id: "moreno".into(),
                player_out_id: "khan".into(),
                minute: Some(70),
                occurred_at: Some(parse_ts("2024-05-01T20:10:00.000Z")),
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeKickOff,
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeSecondHalfKickOff,
            }),
            FootballLiveEvent::Period(FootballPeriodEvent {
                period: FootballPeriod::ExtraTimeFullTime,
            }),
            FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                side_id: "riverside".into(),
                scored: true,
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
                occurred_at: Some(parse_ts("2024-05-01T20:04:03.000Z")),
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
    fn netball_live_event_round_trips_through_dao_mirror() {
        let events = vec![
            NetballLiveEvent::Goal(NetballGoalEvent {
                side_id: "kestrels".into(),
                scorer_player_id: Some("shooter_1".into()),
                scorer_position: Some(NetballPosition::GoalShooter),
                two_points: false,
                minute: Some(4),
                occurred_at: Some(parse_ts("2024-05-01T20:04:00.000Z")),
            }),
            NetballLiveEvent::Foul(NetballFoulEvent {
                side_id: "harriers".into(),
                player_id: Some("defender_1".into()),
                foul_kind: NetballFoulKind::Contact,
                minute: Some(6),
                occurred_at: Some(parse_ts("2024-05-01T20:06:00.000Z")),
            }),
            NetballLiveEvent::Period(NetballPeriodEvent {
                period: NetballPeriod::QuarterOneEnd,
                score: HashMap::from([("kestrels".to_string(), 12), ("harriers".to_string(), 9)]),
            }),
        ];

        for event in events {
            let input = LiveEventInput::Netball(event);
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
            MatchFormat::Netball(NetballFormat {
                num_quarters: 4,
                quarter_length_minutes: 15,
                two_point_zone: false,
                extra_time: true,
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
                entries: HashMap::from([("side_red".to_string(), 3), ("side_blue".to_string(), 1)]),
            }),
            Score::Sets(SetsScore {
                entries: HashMap::from([
                    ("side_red".to_string(), vec![6, 4, 7]),
                    ("side_blue".to_string(), vec![4, 6, 5]),
                ]),
            }),
            // A manually-entered cricket result: totals only, no per-player detail.
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
                        batting: None,
                        bowling: None,
                        fall_of_wickets: None,
                        extras: None,
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
                        batting: None,
                        bowling: None,
                        fall_of_wickets: None,
                        extras: None,
                    },
                ],
                recent_deliveries: None,
                next_ball_context: None,
                awaiting_next_innings: None,
                players: HashMap::new(),
            }),
            // A live-scored cricket result: full per-player detail.
            Score::Cricket(CricketScore {
                innings: vec![CricketScoreInnings {
                    batting_side_id: "warriors".into(),
                    bowling_side_id: "mill_lane".into(),
                    runs: 45,
                    wickets: 1,
                    overs: Overs { overs: 8, balls: 2 },
                    declared: false,
                    batting: Some(vec![CricketBattingEntry {
                        player_id: "player_1".into(),
                        runs: 30,
                        balls_faced: 20,
                        fours: 4,
                        sixes: 1,
                        dismissal: Some(CricketDismissal {
                            kind: CricketDismissalKind::Caught,
                            bowler_player_id: Some("player_5".into()),
                            fielder_player_id: Some("player_6".into()),
                        }),
                        batting_position: Some(1),
                    }]),
                    bowling: Some(vec![CricketBowlingEntry {
                        player_id: "player_5".into(),
                        overs: Overs { overs: 4, balls: 0 },
                        maidens: 1,
                        runs_conceded: 20,
                        wickets: 1,
                        wides: 0,
                        no_balls: 0,
                    }]),
                    fall_of_wickets: Some(vec![CricketFallOfWicket {
                        wicket: 1,
                        runs: 30,
                        player_id: "player_1".into(),
                        overs: Some(Overs { overs: 7, balls: 4 }),
                    }]),
                    extras: Some(CricketExtras {
                        byes: 1,
                        leg_byes: 2,
                        wides: 3,
                        no_balls: 0,
                        penalty: 0,
                    }),
                }],
                recent_deliveries: Some(vec![CricketDelivery {
                    over: 8,
                    ball: 3,
                    bowler_player_id: "player_5".into(),
                    striker_player_id: "player_1".into(),
                    non_striker_player_id: "player_2".into(),
                    runs_off_bat: 1,
                    extra: None,
                    wicket: None,
                    occurred_at: Some(parse_ts("2024-05-01T20:08:03.000Z")),
                }]),
                next_ball_context: Some(NextBallContext {
                    striker_player_id: Some("player_2".into()),
                    non_striker_player_id: Some("player_1".into()),
                    bowler_player_id: Some("player_5".into()),
                    over: 8,
                    ball: 4,
                    previous_over_bowler_player_id: None,
                    runs_conceded_this_over: 1,
                }),
                awaiting_next_innings: Some(false),
                players: HashMap::new(),
            }),
            // A manually-entered football result: totals only, no detail.
            Score::Football(FootballScore {
                score: HashMap::from([("side_red".to_string(), 3), ("side_blue".to_string(), 1)]),
                goals: None,
                cards: None,
                substitutions: None,
                period: None,
                period_times: None,
                penalty_shootout: None,
                penalty_shootout_score: None,
                players: HashMap::new(),
            }),
            // A live-scored football result: full goal/card/sub detail.
            Score::Football(FootballScore {
                score: HashMap::from([("side_red".to_string(), 2), ("side_blue".to_string(), 1)]),
                goals: Some(vec![
                    FootballGoalEvent {
                        side_id: "side_red".into(),
                        scorer_player_id: Some("player_1".into()),
                        assist_player_id: Some("player_2".into()),
                        own_goal: false,
                        penalty: false,
                        minute: Some(23),
                        occurred_at: Some(parse_ts("2024-05-01T20:23:00.000Z")),
                    },
                    FootballGoalEvent {
                        side_id: "side_blue".into(),
                        scorer_player_id: None,
                        assist_player_id: None,
                        own_goal: true,
                        penalty: false,
                        minute: None,
                        occurred_at: None,
                    },
                ]),
                cards: Some(vec![FootballCardEvent {
                    side_id: "side_blue".into(),
                    player_id: "player_3".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(60),
                    occurred_at: Some(parse_ts("2024-05-01T21:00:00.000Z")),
                }]),
                substitutions: Some(vec![FootballSubstitutionEvent {
                    side_id: "side_red".into(),
                    player_in_id: "player_4".into(),
                    player_out_id: "player_1".into(),
                    minute: Some(75),
                    occurred_at: Some(parse_ts("2024-05-01T21:15:00.000Z")),
                }]),
                period: Some(FootballPeriod::FullTime),
                period_times: Some(HashMap::from([
                    (
                        FootballPeriod::KickOff,
                        parse_ts("2024-05-01T20:00:00.000Z"),
                    ),
                    (
                        FootballPeriod::FullTime,
                        parse_ts("2024-05-01T21:45:00.000Z"),
                    ),
                ])),
                penalty_shootout: Some(vec![FootballPenaltyShootoutKick {
                    side_id: "side_red".into(),
                    scored: true,
                }]),
                penalty_shootout_score: Some(HashMap::from([("side_red".to_string(), 1)])),
                players: HashMap::new(),
            }),
            // A manually-entered / quarter-only-scored netball result: goal
            // tally only, no goal-by-goal detail.
            Score::Netball(NetballScore {
                score: HashMap::from([("kestrels".to_string(), 45), ("harriers".to_string(), 38)]),
                goals: None,
                fouls: None,
                period: Some(NetballPeriod::FullTime),
                period_times: None,
                period_scores: Some(HashMap::from([
                    (
                        NetballPeriod::QuarterOneEnd,
                        HashMap::from([("kestrels".to_string(), 12), ("harriers".to_string(), 9)]),
                    ),
                    (
                        NetballPeriod::FullTime,
                        HashMap::from([("kestrels".to_string(), 45), ("harriers".to_string(), 38)]),
                    ),
                ])),
                players: HashMap::new(),
            }),
            // A live-scored (event-by-event) netball result: full goal/foul
            // detail.
            Score::Netball(NetballScore {
                score: HashMap::from([("kestrels".to_string(), 2), ("harriers".to_string(), 1)]),
                goals: Some(vec![
                    NetballGoalEvent {
                        side_id: "kestrels".into(),
                        scorer_player_id: Some("player_1".into()),
                        scorer_position: Some(NetballPosition::GoalShooter),
                        two_points: false,
                        minute: Some(3),
                        occurred_at: Some(parse_ts("2024-05-01T20:03:00.000Z")),
                    },
                    NetballGoalEvent {
                        side_id: "kestrels".into(),
                        scorer_player_id: Some("player_2".into()),
                        scorer_position: Some(NetballPosition::GoalAttack),
                        two_points: true,
                        minute: Some(7),
                        occurred_at: Some(parse_ts("2024-05-01T20:07:00.000Z")),
                    },
                ]),
                fouls: Some(vec![NetballFoulEvent {
                    side_id: "harriers".into(),
                    player_id: Some("player_3".into()),
                    foul_kind: NetballFoulKind::Obstruction,
                    minute: Some(5),
                    occurred_at: Some(parse_ts("2024-05-01T20:05:00.000Z")),
                }]),
                period: Some(NetballPeriod::QuarterOneEnd),
                period_times: Some(HashMap::from([(
                    NetballPeriod::QuarterOneEnd,
                    parse_ts("2024-05-01T20:15:00.000Z"),
                )])),
                period_scores: Some(HashMap::from([(
                    NetballPeriod::QuarterOneEnd,
                    HashMap::from([("kestrels".to_string(), 2), ("harriers".to_string(), 1)]),
                )])),
                players: HashMap::new(),
            }),
        ];

        for score in scores {
            let original_json = score.to_json();
            let round_tripped = score_from_record(&score_to_record(&score));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }
}
