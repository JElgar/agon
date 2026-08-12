//! Boundary mapping between API models (poem-openapi) and DAO records.
//!
//! The DAO owns its own record types and shares nothing with the API. This
//! module is the single place that translates between the two, so handlers stay
//! thin and the coupling lives in one file.

use poem::error::InternalServerError;
use tracing::error;

use crate::detailed_score::cricket::{
    CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDeliveryExtra,
    CricketDeliveryWicket, CricketDismissal, CricketDismissalKind, CricketExtraKind, CricketExtras,
    CricketFallOfWicket, NextBallContext, Overs,
};
use crate::detailed_score::football::{
    FootballCardColor, FootballCardEvent, FootballGoalEvent, FootballPenaltyShootoutKick,
    FootballPeriod, FootballSubstitutionEvent,
};
use crate::detailed_score::netball::{
    NetballFoulEvent, NetballFoulKind, NetballGoalEvent, NetballPeriod, NetballPosition,
};
use crate::live_score::{
    LiveEvent, LiveEventInput, NewLiveEventInput,
    cricket::{
        CricketInningsEndEvent, CricketInningsStartEvent, CricketLiveEvent, CricketRetireEvent,
        InningsEndReason,
    },
    football::{FootballLiveEvent, FootballPeriodEvent},
    netball::{NetballLiveEvent, NetballPeriodEvent},
};
use crate::match_format::{CricketFormat, FootballFormat, MatchFormat, NetballFormat};
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
    Comment, ConfirmedScore, CricketScore, CricketScoreInnings, FeedMatch, FootballScore, Location,
    Match, MatchOutcome, MatchPlayer, MatchSide, MatchSocial, MatchStatus, MatchType, NetballScore,
    PendingScore, Photo, RosterPreviewPlayer, Score, ScoreConfirmation, ScoreResponseKind,
    ScoreSubmission, ScoreSubmissionResponse, ScoreSubmissionStatus, SearchMatch, SetsScore,
    SimpleScore, UserProfile, UserSportStats,
};
use agon_core::dao::error::DaoError;
use agon_core::dao::live_score_ops::NewLiveEvent;
use agon_core::dao::records::{
    CommentRecord, ConfirmedScoreRecord, CricketBattingEntryRecord, CricketBowlingEntryRecord,
    CricketDeliveryExtraRecord, CricketDeliveryRecord, CricketDeliveryWicketRecord,
    CricketDismissalKindRecord, CricketDismissalRecord, CricketExtraKindRecord,
    CricketExtrasRecord, CricketFallOfWicketRecord, CricketFormatRecord,
    CricketInningsEndEventRecord, CricketInningsStartEventRecord, CricketLiveEventRecord,
    CricketRetireEventRecord, CricketScoreInningsRecord, EmbeddedInvitationRecord,
    FootballCardColorRecord, FootballCardEventRecord, FootballFormatRecord,
    FootballGoalEventRecord, FootballLiveEventRecord, FootballPenaltyShootoutKickRecord,
    FootballPeriodEventRecord, FootballPeriodRecord, FootballSubstitutionEventRecord,
    InningsEndReasonRecord, InvitationContextRecord, InvitationKindRecord, InvitationRecord,
    LiveEventPayloadRecord, LiveEventRecord, MatchFormatRecord, MatchLikeRecord, MatchPlayerRecord,
    MatchRecord, MatchScoreRecord, MatchSideRecord, NetballFormatRecord, NetballFoulEventRecord,
    NetballFoulKindRecord, NetballGoalEventRecord, NetballLiveEventRecord,
    NetballPeriodEventRecord, NetballPeriodRecord, NetballPositionRecord, NextBallContextRecord,
    NotificationKindRecord, NotificationRecord, OversRecord, PendingScoreRecord,
    ScoreConfirmationRecord, ScoreRecord, ScoreResponseRecord, ScoreSubmissionRecord,
    TeamMemberRecord, TeamRecord, UserRecord, UserSportStatsRecord,
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
            asset_id: None,
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
        None
    } else {
        Some((rec.wins as f32 / rec.matches_played as f32) * 100.0)
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
            entries: entries.clone(),
        }),
        ScoreRecord::Sets { entries } => Score::Sets(SetsScore {
            entries: entries.clone(),
        }),
        ScoreRecord::Cricket {
            innings,
            recent_deliveries,
            next_ball_context,
            awaiting_next_innings,
        } => Score::Cricket(CricketScore {
            innings: innings
                .iter()
                .map(cricket_score_innings_from_record)
                .collect(),
            recent_deliveries: recent_deliveries
                .as_ref()
                .map(|ds| ds.iter().map(cricket_delivery_from_record).collect()),
            next_ball_context: next_ball_context
                .as_ref()
                .map(next_ball_context_from_record),
            awaiting_next_innings: *awaiting_next_innings,
            // Not stored — `Api::hydrate_score_players` fills this afterward
            // (see `CricketScore::players`' doc comment).
            players: std::collections::HashMap::new(),
        }),
        ScoreRecord::Football {
            score,
            goals,
            cards,
            substitutions,
            period,
            period_times,
            penalty_shootout,
            penalty_shootout_score,
        } => Score::Football(FootballScore {
            score: score.clone(),
            goals: goals
                .as_ref()
                .map(|gs| gs.iter().map(football_goal_event_from_record).collect()),
            cards: cards
                .as_ref()
                .map(|cs| cs.iter().map(football_card_event_from_record).collect()),
            substitutions: substitutions.as_ref().map(|subs| {
                subs.iter()
                    .map(football_substitution_event_from_record)
                    .collect()
            }),
            period: period.as_ref().map(football_period_from_record),
            period_times: period_times.as_ref().map(|pts| {
                pts.iter()
                    .map(|(p, t)| (football_period_from_record(p), parse_ts(t)))
                    .collect()
            }),
            penalty_shootout: penalty_shootout.as_ref().map(|ks| {
                ks.iter()
                    .map(|k| FootballPenaltyShootoutKick {
                        side_id: k.side_id.clone(),
                        scored: k.scored,
                    })
                    .collect()
            }),
            penalty_shootout_score: penalty_shootout_score.clone(),
            // Not stored — `Api::hydrate_score_players` fills this afterward.
            players: std::collections::HashMap::new(),
        }),
        ScoreRecord::Netball {
            score,
            goals,
            fouls,
            period,
            period_times,
            period_scores,
        } => Score::Netball(NetballScore {
            score: score.clone(),
            goals: goals
                .as_ref()
                .map(|gs| gs.iter().map(netball_goal_event_from_record).collect()),
            fouls: fouls
                .as_ref()
                .map(|fs| fs.iter().map(netball_foul_event_from_record).collect()),
            period: period.as_ref().map(netball_period_from_record),
            period_times: period_times.as_ref().map(|pts| {
                pts.iter()
                    .map(|(p, t)| (netball_period_from_record(p), parse_ts(t)))
                    .collect()
            }),
            period_scores: period_scores.as_ref().map(|pss| {
                pss.iter()
                    .map(|(p, entries)| (netball_period_from_record(p), entries.clone()))
                    .collect()
            }),
            // Not stored — `Api::hydrate_score_players` fills this afterward.
            players: std::collections::HashMap::new(),
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

fn cricket_dismissal_to_record(d: &CricketDismissal) -> CricketDismissalRecord {
    CricketDismissalRecord {
        kind: cricket_dismissal_kind_to_record(&d.kind),
        bowler_player_id: d.bowler_player_id.clone(),
        fielder_player_id: d.fielder_player_id.clone(),
    }
}

fn cricket_dismissal_from_record(rec: &CricketDismissalRecord) -> CricketDismissal {
    CricketDismissal {
        kind: cricket_dismissal_kind_from_record(&rec.kind),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn cricket_batting_entry_to_record(b: &CricketBattingEntry) -> CricketBattingEntryRecord {
    CricketBattingEntryRecord {
        player_id: b.player_id.clone(),
        runs: b.runs,
        balls_faced: b.balls_faced,
        fours: b.fours,
        sixes: b.sixes,
        dismissal: b.dismissal.as_ref().map(cricket_dismissal_to_record),
        batting_position: b.batting_position,
    }
}

fn cricket_batting_entry_from_record(rec: &CricketBattingEntryRecord) -> CricketBattingEntry {
    CricketBattingEntry {
        player_id: rec.player_id.clone(),
        runs: rec.runs,
        balls_faced: rec.balls_faced,
        fours: rec.fours,
        sixes: rec.sixes,
        dismissal: rec.dismissal.as_ref().map(cricket_dismissal_from_record),
        batting_position: rec.batting_position,
    }
}

fn cricket_bowling_entry_to_record(b: &CricketBowlingEntry) -> CricketBowlingEntryRecord {
    CricketBowlingEntryRecord {
        player_id: b.player_id.clone(),
        overs: overs_to_record(&b.overs),
        maidens: b.maidens,
        runs_conceded: b.runs_conceded,
        wickets: b.wickets,
        wides: b.wides,
        no_balls: b.no_balls,
    }
}

fn cricket_bowling_entry_from_record(rec: &CricketBowlingEntryRecord) -> CricketBowlingEntry {
    CricketBowlingEntry {
        player_id: rec.player_id.clone(),
        overs: overs_from_record(&rec.overs),
        maidens: rec.maidens,
        runs_conceded: rec.runs_conceded,
        wickets: rec.wickets,
        wides: rec.wides,
        no_balls: rec.no_balls,
    }
}

fn cricket_extras_to_record(e: &CricketExtras) -> CricketExtrasRecord {
    CricketExtrasRecord {
        byes: e.byes,
        leg_byes: e.leg_byes,
        wides: e.wides,
        no_balls: e.no_balls,
        penalty: e.penalty,
    }
}

fn cricket_extras_from_record(rec: &CricketExtrasRecord) -> CricketExtras {
    CricketExtras {
        byes: rec.byes,
        leg_byes: rec.leg_byes,
        wides: rec.wides,
        no_balls: rec.no_balls,
        penalty: rec.penalty,
    }
}

fn cricket_fall_of_wicket_to_record(f: &CricketFallOfWicket) -> CricketFallOfWicketRecord {
    CricketFallOfWicketRecord {
        wicket: f.wicket,
        runs: f.runs,
        player_id: f.player_id.clone(),
        overs: f.overs.map(|o| overs_to_record(&o)),
    }
}

fn cricket_fall_of_wicket_from_record(rec: &CricketFallOfWicketRecord) -> CricketFallOfWicket {
    CricketFallOfWicket {
        wicket: rec.wicket,
        runs: rec.runs,
        player_id: rec.player_id.clone(),
        overs: rec.overs.map(|o| overs_from_record(&o)),
    }
}

fn cricket_score_innings_to_record(i: &CricketScoreInnings) -> CricketScoreInningsRecord {
    CricketScoreInningsRecord {
        batting_side_id: i.batting_side_id.clone(),
        bowling_side_id: i.bowling_side_id.clone(),
        runs: i.runs,
        wickets: i.wickets,
        overs: overs_to_record(&i.overs),
        declared: i.declared,
        batting: i
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(cricket_batting_entry_to_record).collect()),
        bowling: i
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(cricket_bowling_entry_to_record).collect()),
        fall_of_wickets: i
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(cricket_fall_of_wicket_to_record).collect()),
        extras: i.extras.as_ref().map(cricket_extras_to_record),
    }
}

fn cricket_score_innings_from_record(rec: &CricketScoreInningsRecord) -> CricketScoreInnings {
    CricketScoreInnings {
        batting_side_id: rec.batting_side_id.clone(),
        bowling_side_id: rec.bowling_side_id.clone(),
        runs: rec.runs,
        wickets: rec.wickets,
        overs: overs_from_record(&rec.overs),
        declared: rec.declared,
        batting: rec
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(cricket_batting_entry_from_record).collect()),
        bowling: rec
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(cricket_bowling_entry_from_record).collect()),
        fall_of_wickets: rec
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(cricket_fall_of_wicket_from_record).collect()),
        extras: rec.extras.as_ref().map(cricket_extras_from_record),
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
        Score::Cricket(s) => ScoreRecord::Cricket {
            innings: s
                .innings
                .iter()
                .map(cricket_score_innings_to_record)
                .collect(),
            recent_deliveries: s
                .recent_deliveries
                .as_ref()
                .map(|ds| ds.iter().map(cricket_delivery_to_record).collect()),
            next_ball_context: s
                .next_ball_context
                .as_ref()
                .map(next_ball_context_to_record),
            awaiting_next_innings: s.awaiting_next_innings,
        },
        Score::Football(s) => ScoreRecord::Football {
            score: s.score.clone(),
            goals: s
                .goals
                .as_ref()
                .map(|gs| gs.iter().map(football_goal_event_to_record).collect()),
            cards: s
                .cards
                .as_ref()
                .map(|cs| cs.iter().map(football_card_event_to_record).collect()),
            substitutions: s.substitutions.as_ref().map(|subs| {
                subs.iter()
                    .map(football_substitution_event_to_record)
                    .collect()
            }),
            period: s.period.as_ref().map(football_period_to_record),
            period_times: s.period_times.as_ref().map(|pts| {
                pts.iter()
                    .map(|(p, t)| {
                        (
                            football_period_to_record(p),
                            t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        )
                    })
                    .collect()
            }),
            penalty_shootout: s.penalty_shootout.as_ref().map(|ks| {
                ks.iter()
                    .map(|k| FootballPenaltyShootoutKickRecord {
                        side_id: k.side_id.clone(),
                        scored: k.scored,
                    })
                    .collect()
            }),
            penalty_shootout_score: s.penalty_shootout_score.clone(),
        },
        Score::Netball(s) => ScoreRecord::Netball {
            score: s.score.clone(),
            goals: s
                .goals
                .as_ref()
                .map(|gs| gs.iter().map(netball_goal_event_to_record).collect()),
            fouls: s
                .fouls
                .as_ref()
                .map(|fs| fs.iter().map(netball_foul_event_to_record).collect()),
            period: s.period.as_ref().map(netball_period_to_record),
            period_times: s.period_times.as_ref().map(|pts| {
                pts.iter()
                    .map(|(p, t)| {
                        (
                            netball_period_to_record(p),
                            t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        )
                    })
                    .collect()
            }),
            period_scores: s.period_scores.as_ref().map(|pss| {
                pss.iter()
                    .map(|(p, entries)| (netball_period_to_record(p), entries.clone()))
                    .collect()
            }),
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
        MatchFormat::Netball(f) => MatchFormatRecord::Netball(NetballFormatRecord {
            num_quarters: f.num_quarters,
            quarter_length_minutes: f.quarter_length_minutes,
            two_point_zone: f.two_point_zone,
            extra_time: f.extra_time,
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
        MatchFormatRecord::Netball(f) => MatchFormat::Netball(NetballFormat {
            num_quarters: f.num_quarters,
            quarter_length_minutes: f.quarter_length_minutes,
            two_point_zone: f.two_point_zone,
            extra_time: f.extra_time,
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
            LiveEventPayloadRecord::Football(football_live_event_to_record(f))
        }
        LiveEventInput::Cricket(c) => {
            LiveEventPayloadRecord::Cricket(cricket_live_event_to_record(c))
        }
        LiveEventInput::Netball(n) => {
            LiveEventPayloadRecord::Netball(netball_live_event_to_record(n))
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
        LiveEventPayloadRecord::Netball(n) => {
            LiveEventInput::Netball(netball_live_event_from_record(n))
        }
    }
}

// ---- Football ---------------------------------------------------------

/// Shared by the live-event mapping below and `score_to_record`'s `Football`
/// arm — both carry the same `FootballGoalEvent`/`FootballGoalEventRecord`
/// shape.
fn football_goal_event_to_record(g: &FootballGoalEvent) -> FootballGoalEventRecord {
    FootballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        assist_player_id: g.assist_player_id.clone(),
        own_goal: g.own_goal,
        penalty: g.penalty,
        minute: g.minute,
    }
}

fn football_goal_event_from_record(rec: &FootballGoalEventRecord) -> FootballGoalEvent {
    FootballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        assist_player_id: rec.assist_player_id.clone(),
        own_goal: rec.own_goal,
        penalty: rec.penalty,
        minute: rec.minute,
    }
}

/// Shared by the live-event mapping below and `score_to_record`'s `Football`
/// arm, same reasoning as `football_goal_event_to_record`.
fn football_card_event_to_record(c: &FootballCardEvent) -> FootballCardEventRecord {
    FootballCardEventRecord {
        side_id: c.side_id.clone(),
        player_id: c.player_id.clone(),
        color: match c.color {
            FootballCardColor::Yellow => FootballCardColorRecord::Yellow,
            FootballCardColor::Red => FootballCardColorRecord::Red,
        },
        minute: c.minute,
    }
}

fn football_card_event_from_record(rec: &FootballCardEventRecord) -> FootballCardEvent {
    FootballCardEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        color: match rec.color {
            FootballCardColorRecord::Yellow => FootballCardColor::Yellow,
            FootballCardColorRecord::Red => FootballCardColor::Red,
        },
        minute: rec.minute,
    }
}

fn football_substitution_event_to_record(
    s: &FootballSubstitutionEvent,
) -> FootballSubstitutionEventRecord {
    FootballSubstitutionEventRecord {
        side_id: s.side_id.clone(),
        player_in_id: s.player_in_id.clone(),
        player_out_id: s.player_out_id.clone(),
        minute: s.minute,
    }
}

fn football_substitution_event_from_record(
    rec: &FootballSubstitutionEventRecord,
) -> FootballSubstitutionEvent {
    FootballSubstitutionEvent {
        side_id: rec.side_id.clone(),
        player_in_id: rec.player_in_id.clone(),
        player_out_id: rec.player_out_id.clone(),
        minute: rec.minute,
    }
}

/// Shared by the live-event mapping below and `score_to_record`'s `Football`
/// arm (`period`/`period_times`' map keys) — both need the same
/// `FootballPeriod`/`FootballPeriodRecord` correspondence.
fn football_period_to_record(period: &FootballPeriod) -> FootballPeriodRecord {
    match period {
        FootballPeriod::KickOff => FootballPeriodRecord::KickOff,
        FootballPeriod::HalfTime => FootballPeriodRecord::HalfTime,
        FootballPeriod::SecondHalfKickOff => FootballPeriodRecord::SecondHalfKickOff,
        FootballPeriod::FullTime => FootballPeriodRecord::FullTime,
        FootballPeriod::ExtraTimeKickOff => FootballPeriodRecord::ExtraTimeKickOff,
        FootballPeriod::ExtraTimeHalfTime => FootballPeriodRecord::ExtraTimeHalfTime,
        FootballPeriod::ExtraTimeSecondHalfKickOff => {
            FootballPeriodRecord::ExtraTimeSecondHalfKickOff
        }
        FootballPeriod::ExtraTimeFullTime => FootballPeriodRecord::ExtraTimeFullTime,
        FootballPeriod::PenaltiesComplete => FootballPeriodRecord::PenaltiesComplete,
    }
}

fn football_period_from_record(rec: &FootballPeriodRecord) -> FootballPeriod {
    match rec {
        FootballPeriodRecord::KickOff => FootballPeriod::KickOff,
        FootballPeriodRecord::HalfTime => FootballPeriod::HalfTime,
        FootballPeriodRecord::SecondHalfKickOff => FootballPeriod::SecondHalfKickOff,
        FootballPeriodRecord::FullTime => FootballPeriod::FullTime,
        FootballPeriodRecord::ExtraTimeKickOff => FootballPeriod::ExtraTimeKickOff,
        FootballPeriodRecord::ExtraTimeHalfTime => FootballPeriod::ExtraTimeHalfTime,
        FootballPeriodRecord::ExtraTimeSecondHalfKickOff => {
            FootballPeriod::ExtraTimeSecondHalfKickOff
        }
        FootballPeriodRecord::ExtraTimeFullTime => FootballPeriod::ExtraTimeFullTime,
        FootballPeriodRecord::PenaltiesComplete => FootballPeriod::PenaltiesComplete,
    }
}

fn football_live_event_to_record(event: &FootballLiveEvent) -> FootballLiveEventRecord {
    match event {
        FootballLiveEvent::Goal(g) => {
            FootballLiveEventRecord::Goal(football_goal_event_to_record(g))
        }
        FootballLiveEvent::Card(c) => {
            FootballLiveEventRecord::Card(football_card_event_to_record(c))
        }
        FootballLiveEvent::Substitution(s) => {
            FootballLiveEventRecord::Substitution(football_substitution_event_to_record(s))
        }
        FootballLiveEvent::Period(p) => {
            FootballLiveEventRecord::Period(FootballPeriodEventRecord {
                period: football_period_to_record(&p.period),
            })
        }
        FootballLiveEvent::PenaltyShootoutKick(k) => {
            FootballLiveEventRecord::PenaltyShootoutKick(FootballPenaltyShootoutKickRecord {
                side_id: k.side_id.clone(),
                scored: k.scored,
            })
        }
    }
}

fn football_live_event_from_record(rec: &FootballLiveEventRecord) -> FootballLiveEvent {
    match rec {
        FootballLiveEventRecord::Goal(g) => {
            FootballLiveEvent::Goal(football_goal_event_from_record(g))
        }
        FootballLiveEventRecord::Card(c) => {
            FootballLiveEvent::Card(football_card_event_from_record(c))
        }
        FootballLiveEventRecord::Substitution(s) => {
            FootballLiveEvent::Substitution(football_substitution_event_from_record(s))
        }
        FootballLiveEventRecord::Period(p) => FootballLiveEvent::Period(FootballPeriodEvent {
            period: football_period_from_record(&p.period),
        }),
        FootballLiveEventRecord::PenaltyShootoutKick(k) => {
            FootballLiveEvent::PenaltyShootoutKick(FootballPenaltyShootoutKick {
                side_id: k.side_id.clone(),
                scored: k.scored,
            })
        }
    }
}

// ---- Netball --------------------------------------------------------------

/// Shared by the live-event mapping below and `score_to_record`'s `Netball`
/// arm — both carry the same `NetballGoalEvent`/`NetballGoalEventRecord`
/// shape.
fn netball_goal_event_to_record(g: &NetballGoalEvent) -> NetballGoalEventRecord {
    NetballGoalEventRecord {
        side_id: g.side_id.clone(),
        scorer_player_id: g.scorer_player_id.clone(),
        scorer_position: g.scorer_position.as_ref().map(netball_position_to_record),
        two_points: g.two_points,
        minute: g.minute,
    }
}

fn netball_goal_event_from_record(rec: &NetballGoalEventRecord) -> NetballGoalEvent {
    NetballGoalEvent {
        side_id: rec.side_id.clone(),
        scorer_player_id: rec.scorer_player_id.clone(),
        scorer_position: rec
            .scorer_position
            .as_ref()
            .map(netball_position_from_record),
        two_points: rec.two_points,
        minute: rec.minute,
    }
}

fn netball_position_to_record(p: &NetballPosition) -> NetballPositionRecord {
    match p {
        NetballPosition::GoalShooter => NetballPositionRecord::GoalShooter,
        NetballPosition::GoalAttack => NetballPositionRecord::GoalAttack,
        NetballPosition::WingAttack => NetballPositionRecord::WingAttack,
        NetballPosition::Centre => NetballPositionRecord::Centre,
        NetballPosition::WingDefence => NetballPositionRecord::WingDefence,
        NetballPosition::GoalDefence => NetballPositionRecord::GoalDefence,
        NetballPosition::GoalKeeper => NetballPositionRecord::GoalKeeper,
    }
}

fn netball_position_from_record(rec: &NetballPositionRecord) -> NetballPosition {
    match rec {
        NetballPositionRecord::GoalShooter => NetballPosition::GoalShooter,
        NetballPositionRecord::GoalAttack => NetballPosition::GoalAttack,
        NetballPositionRecord::WingAttack => NetballPosition::WingAttack,
        NetballPositionRecord::Centre => NetballPosition::Centre,
        NetballPositionRecord::WingDefence => NetballPosition::WingDefence,
        NetballPositionRecord::GoalDefence => NetballPosition::GoalDefence,
        NetballPositionRecord::GoalKeeper => NetballPosition::GoalKeeper,
    }
}

/// Shared by the live-event mapping below and `score_to_record`'s `Netball`
/// arm, same reasoning as `netball_goal_event_to_record`.
fn netball_foul_event_to_record(fo: &NetballFoulEvent) -> NetballFoulEventRecord {
    NetballFoulEventRecord {
        side_id: fo.side_id.clone(),
        player_id: fo.player_id.clone(),
        foul_kind: match fo.foul_kind {
            NetballFoulKind::Contact => NetballFoulKindRecord::Contact,
            NetballFoulKind::Obstruction => NetballFoulKindRecord::Obstruction,
            NetballFoulKind::Footwork => NetballFoulKindRecord::Footwork,
            NetballFoulKind::Offside => NetballFoulKindRecord::Offside,
            NetballFoulKind::HeldBall => NetballFoulKindRecord::HeldBall,
            NetballFoulKind::Other => NetballFoulKindRecord::Other,
        },
        minute: fo.minute,
    }
}

fn netball_foul_event_from_record(rec: &NetballFoulEventRecord) -> NetballFoulEvent {
    NetballFoulEvent {
        side_id: rec.side_id.clone(),
        player_id: rec.player_id.clone(),
        foul_kind: match rec.foul_kind {
            NetballFoulKindRecord::Contact => NetballFoulKind::Contact,
            NetballFoulKindRecord::Obstruction => NetballFoulKind::Obstruction,
            NetballFoulKindRecord::Footwork => NetballFoulKind::Footwork,
            NetballFoulKindRecord::Offside => NetballFoulKind::Offside,
            NetballFoulKindRecord::HeldBall => NetballFoulKind::HeldBall,
            NetballFoulKindRecord::Other => NetballFoulKind::Other,
        },
        minute: rec.minute,
    }
}

/// Shared by the live-event mapping below and `score_to_record`'s `Netball`
/// arm (`period`/`period_times`/`period_scores`' map keys) — both need the
/// same `NetballPeriod`/`NetballPeriodRecord` correspondence.
fn netball_period_to_record(period: &NetballPeriod) -> NetballPeriodRecord {
    match period {
        NetballPeriod::Start => NetballPeriodRecord::Start,
        NetballPeriod::QuarterOneEnd => NetballPeriodRecord::QuarterOneEnd,
        NetballPeriod::QuarterTwoEnd => NetballPeriodRecord::QuarterTwoEnd,
        NetballPeriod::QuarterThreeEnd => NetballPeriodRecord::QuarterThreeEnd,
        NetballPeriod::FullTime => NetballPeriodRecord::FullTime,
        NetballPeriod::ExtraTimeStart => NetballPeriodRecord::ExtraTimeStart,
        NetballPeriod::ExtraTimeEnd => NetballPeriodRecord::ExtraTimeEnd,
    }
}

fn netball_period_from_record(rec: &NetballPeriodRecord) -> NetballPeriod {
    match rec {
        NetballPeriodRecord::Start => NetballPeriod::Start,
        NetballPeriodRecord::QuarterOneEnd => NetballPeriod::QuarterOneEnd,
        NetballPeriodRecord::QuarterTwoEnd => NetballPeriod::QuarterTwoEnd,
        NetballPeriodRecord::QuarterThreeEnd => NetballPeriod::QuarterThreeEnd,
        NetballPeriodRecord::FullTime => NetballPeriod::FullTime,
        NetballPeriodRecord::ExtraTimeStart => NetballPeriod::ExtraTimeStart,
        NetballPeriodRecord::ExtraTimeEnd => NetballPeriod::ExtraTimeEnd,
    }
}

fn netball_live_event_to_record(event: &NetballLiveEvent) -> NetballLiveEventRecord {
    match event {
        NetballLiveEvent::Goal(g) => NetballLiveEventRecord::Goal(netball_goal_event_to_record(g)),
        NetballLiveEvent::Foul(fo) => {
            NetballLiveEventRecord::Foul(netball_foul_event_to_record(fo))
        }
        NetballLiveEvent::Period(p) => NetballLiveEventRecord::Period(NetballPeriodEventRecord {
            period: netball_period_to_record(&p.period),
            score: p.score.clone(),
        }),
    }
}

fn netball_live_event_from_record(rec: &NetballLiveEventRecord) -> NetballLiveEvent {
    match rec {
        NetballLiveEventRecord::Goal(g) => {
            NetballLiveEvent::Goal(netball_goal_event_from_record(g))
        }
        NetballLiveEventRecord::Foul(fo) => {
            NetballLiveEvent::Foul(netball_foul_event_from_record(fo))
        }
        NetballLiveEventRecord::Period(p) => NetballLiveEvent::Period(NetballPeriodEvent {
            period: netball_period_from_record(&p.period),
            score: p.score.clone(),
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

fn next_ball_context_to_record(ctx: &NextBallContext) -> NextBallContextRecord {
    NextBallContextRecord {
        striker_player_id: ctx.striker_player_id.clone(),
        non_striker_player_id: ctx.non_striker_player_id.clone(),
        bowler_player_id: ctx.bowler_player_id.clone(),
        over: ctx.over,
        ball: ctx.ball,
        previous_over_bowler_player_id: ctx.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: ctx.runs_conceded_this_over,
    }
}

fn next_ball_context_from_record(rec: &NextBallContextRecord) -> NextBallContext {
    NextBallContext {
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        over: rec.over,
        ball: rec.ball,
        previous_over_bowler_player_id: rec.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: rec.runs_conceded_this_over,
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
                    LiveEventPayloadRecord::Football(f) => {
                        Some((parse_ts(&r.occurred_at), football_live_event_from_record(f)))
                    }
                    LiveEventPayloadRecord::Cricket(_) | LiveEventPayloadRecord::Netball(_) => None,
                })
                .collect();
            Some(Score::Football(FootballScore::from_events(&events)))
        }
        "cricket" => {
            let events: Vec<CricketLiveEvent> = records
                .iter()
                .filter_map(|r| match &r.payload {
                    LiveEventPayloadRecord::Cricket(c) => Some(cricket_live_event_from_record(c)),
                    LiveEventPayloadRecord::Football(_) | LiveEventPayloadRecord::Netball(_) => {
                        None
                    }
                })
                .collect();
            let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) =
                cricket_format_args(format);
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
                    LiveEventPayloadRecord::Netball(n) => {
                        Some((parse_ts(&r.occurred_at), netball_live_event_from_record(n)))
                    }
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
    use std::collections::HashMap;

    use super::*;
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
            }),
            NetballLiveEvent::Foul(NetballFoulEvent {
                side_id: "harriers".into(),
                player_id: Some("defender_1".into()),
                foul_kind: NetballFoulKind::Contact,
                minute: Some(6),
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
                    },
                    FootballGoalEvent {
                        side_id: "side_blue".into(),
                        scorer_player_id: None,
                        assist_player_id: None,
                        own_goal: true,
                        penalty: false,
                        minute: None,
                    },
                ]),
                cards: Some(vec![FootballCardEvent {
                    side_id: "side_blue".into(),
                    player_id: "player_3".into(),
                    color: FootballCardColor::Yellow,
                    minute: Some(60),
                }]),
                substitutions: Some(vec![FootballSubstitutionEvent {
                    side_id: "side_red".into(),
                    player_in_id: "player_4".into(),
                    player_out_id: "player_1".into(),
                    minute: Some(75),
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
                    },
                    NetballGoalEvent {
                        side_id: "kestrels".into(),
                        scorer_player_id: Some("player_2".into()),
                        scorer_position: Some(NetballPosition::GoalAttack),
                        two_points: true,
                        minute: Some(7),
                    },
                ]),
                fouls: Some(vec![NetballFoulEvent {
                    side_id: "harriers".into(),
                    player_id: Some("player_3".into()),
                    foul_kind: NetballFoulKind::Obstruction,
                    minute: Some(5),
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
