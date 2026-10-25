//! Boundary mapping between API models (poem-openapi) and DAO records.
//!
//! The DAO owns its own record types and shares nothing with the API. This
//! module is the single place that translates between the two, so handlers stay
//! thin and the coupling lives in one file.

use std::collections::HashMap;

use poem::error::InternalServerError;
use tracing::error;

use crate::live_score::{LiveEvent, LiveEventInput, NewLiveEventInput};
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
    BestFigure, Comment, ConfirmedScore, CreateMatchSeriesEndAfterNInput,
    CreateMatchSeriesEndInput, CreateMatchSeriesEndNeverInput, CreateMatchSeriesEndOnDateInput,
    DevicePlatform, FeedMatch, GenericPlayerStats, Location, Match, MatchOutcome, MatchPlayer,
    MatchSeries, MatchSeriesOccurrence, MatchSeriesSide, MatchSeriesStatus, MatchSide, MatchSocial,
    MatchStatus, MatchType, PendingScore, Photo, RatingOptIn, RosterPreviewPlayer, Score,
    ScoreConfirmation, ScoreResponseKind, ScoreSubmission, ScoreSubmissionResponse,
    ScoreSubmissionStatus, SearchMatch, SetsScore, SimpleScore, UserProfile, UserStats,
};
use agon_core::dao::error::DaoError;
use agon_core::dao::live_score_ops::NewLiveEvent;
use agon_core::dao::records::{
    BestFigureRecord, CommentRecord, ConfirmedScoreRecord, DevicePlatform as DevicePlatformRecord,
    EmbeddedInvitationRecord, GenericSportStatsRecord, InvitationContextRecord,
    InvitationKindRecord, InvitationRecord, JoinLinkRecord, JoinLinkScopeRecord,
    LiveEventPayloadRecord, LiveEventRecord, LocationRecord, MatchFormatRecord, MatchLikeRecord,
    MatchPlayerRecord, MatchPlayerRole as MatchPlayerRoleRecord, MatchRecord, MatchScoreRecord,
    MatchSideRecord, NotificationKindRecord, NotificationRecord, PendingScoreRecord,
    ScoreConfirmationRecord, ScoreRecord, ScoreResponseRecord, ScoreSubmissionRecord,
    SeriesEndRecord, SeriesOccurrenceRecord, SeriesRecord, SeriesSideRecord, TeamMemberRecord,
    TeamRecord, UserRecord, UserStatsRecord,
};
use agon_core::rating::ladder_for_tag;

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

/// x-macro template for `agon_sports!` (see `crate::sports`'s doc comment):
/// every `MatchType`/`Score`/`MatchFormat`/`LiveEventInput` conversion and
/// sport-tag/dispatch function this boundary layer needs, generated from the
/// shared sport list in one place. This is the function-level counterpart to
/// the type-level macros in `main.rs`/`match_format.rs`/`live_score/mod.rs` —
/// together they're what used to be ~40 hand-written, easy-to-forget
/// per-sport functions and match arms scattered through this file.
macro_rules! define_sport_dispatch {
    ($( $variant:ident {
        tag: $tag:literal,
        module: $module:ident,
        score: $score:ty,
        format: $format:ty,
        live_event: $live_event:ty,
        stats: $stats:ty $(,)?
    } ),+ $(,)?) => {
        /// Parse a stored sport tag into the API enum, defaulting unknown values
        /// rather than failing a read.
        pub fn match_type_from_tag(tag: &str) -> MatchType {
            match tag {
                "tennis" => MatchType::Tennis,
                "badminton" => MatchType::Badminton,
                "squash" => MatchType::Squash,
                "table_tennis" => MatchType::TableTennis,
                $( $tag => MatchType::$variant, )+
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
                $( MatchType::$variant => $tag, )+
                MatchType::Other => "other",
            }
        }

        /// How a sport is named to people (e.g. share-card headings).
        pub fn match_type_label(mt: &MatchType) -> &'static str {
            match mt {
                MatchType::Tennis => "Tennis",
                MatchType::Badminton => "Badminton",
                MatchType::Squash => "Squash",
                MatchType::TableTennis => "Table Tennis",
                $( MatchType::$variant => crate::sports::$module::LABEL, )+
                MatchType::Other => "Match",
            }
        }

        /// Whether a stored sport tag is one of the fully-modeled sports —
        /// the ones with a live event log and a persisted live score record.
        pub fn is_live_scored_sport(tag: &str) -> bool {
            matches!(tag, $( $tag )|+)
        }

        pub fn score_from_record(rec: &ScoreRecord) -> Score {
            match rec {
                ScoreRecord::Simple { entries } => Score::Simple(SimpleScore {
                    entries: entries.clone(),
                }),
                ScoreRecord::Sets { entries } => Score::Sets(SetsScore {
                    entries: entries.clone(),
                }),
                $( ScoreRecord::$variant(r) => Score::$variant(crate::sports::$module::score_from_record(r)), )+
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
                $( Score::$variant(s) => ScoreRecord::$variant(crate::sports::$module::score_to_record(s)), )+
            }
        }

        /// Wrap a `Score` into the `(sport, score)` record shape. The sport tag
        /// mirrors the union variant so a read can pick the right variant.
        pub fn match_score_to_record(score: &Score, last_seq: Option<u32>) -> MatchScoreRecord {
            let sport = match score {
                Score::Simple(_) => "simple",
                Score::Sets(_) => "sets",
                $( Score::$variant(_) => $tag, )+
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

        pub fn match_format_to_record(fmt: &MatchFormat) -> MatchFormatRecord {
            match fmt {
                $( MatchFormat::$variant(f) => MatchFormatRecord::$variant(crate::sports::$module::format_to_record(f)), )+
            }
        }

        /// Build the API `MatchFormat` from a stored record. Infallible — `format`
        /// is a typed DAO enum, not JSON, so there's no parse step that can fail.
        pub fn match_format_from_record(rec: &MatchFormatRecord) -> MatchFormat {
            match rec {
                $( MatchFormatRecord::$variant(f) => MatchFormat::$variant(crate::sports::$module::format_from_record(f)), )+
            }
        }

        /// The sport tag a `MatchFormat` is for, mirroring the union variant (same
        /// convention as `live_event_sport_tag`) — used to reject a format that
        /// doesn't match the match's own sport.
        pub fn match_format_sport_tag(fmt: &MatchFormat) -> &'static str {
            match fmt {
                $( MatchFormat::$variant(_) => $tag, )+
            }
        }

        /// The stored sport tag for a live event, mirroring the union variant so a
        /// read can pick the right variant back out (same convention as
        /// `match_score_to_record`).
        pub fn live_event_sport_tag(event: &LiveEventInput) -> &'static str {
            match event {
                $( LiveEventInput::$variant(_) => $tag, )+
            }
        }

        pub fn live_event_input_to_record(event: &LiveEventInput) -> LiveEventPayloadRecord {
            match event {
                $( LiveEventInput::$variant(e) => LiveEventPayloadRecord::$variant(crate::sports::$module::live_event_to_record(e)), )+
            }
        }

        pub fn live_event_payload_from_record(rec: &LiveEventPayloadRecord) -> LiveEventInput {
            match rec {
                $( LiveEventPayloadRecord::$variant(e) => LiveEventInput::$variant(crate::sports::$module::live_event_from_record(e)), )+
            }
        }

        /// Folds a match's live event log into its full detail. `None` if
        /// `match_type` isn't a sport live scoring supports yet.
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
                $(
                    $tag => {
                        let events: Vec<_> = records
                            .iter()
                            .filter_map(|r| match &r.payload {
                                LiveEventPayloadRecord::$variant(e) => Some((
                                    parse_ts(&r.occurred_at),
                                    crate::sports::$module::live_event_from_record(e),
                                )),
                                _ => None,
                            })
                            .collect();
                        Some(Score::$variant(crate::sports::$module::from_events(&events, format)))
                    }
                )+
                _ => None,
            }
        }

        /// Map the stored, one-field-per-sport `UserStatsRecord` to the public
        /// `UserStats` — `None` for any sport with no stored entry, same as the
        /// DAO side.
        pub fn user_stats_from_record(stats: &UserStatsRecord) -> UserStats {
            UserStats {
                $( $module: stats.$module.as_ref().map(crate::sports::$module::stats_from_record), )+
                tennis: stats.tennis.as_ref().map(generic_stats_from_record),
                badminton: stats.badminton.as_ref().map(generic_stats_from_record),
                squash: stats.squash.as_ref().map(generic_stats_from_record),
                table_tennis: stats.table_tennis.as_ref().map(generic_stats_from_record),
                other: stats.other.as_ref().map(generic_stats_from_record),
            }
        }

        /// Every side id a score references — what a submitted score is
        /// validated against (each must be one of the match's own sides).
        pub fn score_side_ids(score: &Score) -> Vec<&str> {
            match score {
                Score::Simple(s) => s.entries.keys().map(|k| k.as_str()).collect(),
                Score::Sets(s) => s.entries.keys().map(|k| k.as_str()).collect(),
                $( Score::$variant(s) => crate::sports::$module::side_ids(s), )+
            }
        }

        /// Re-point a `Score`'s request-scoped client ids to the real side/player
        /// ids assigned at creation — side ids via `CreateMatchSideInput.client_id`,
        /// player ids via a real user's own id or a guest's
        /// `CreateMatchExternalInviteInput.client_id` (see `create_match`).
        /// Returns `None` if any referenced side or player is unknown.
        pub fn resolve_score_ids(
            score: &Score,
            side_ids: &HashMap<String, String>,
            player_ids: &HashMap<String, String>,
        ) -> Option<Score> {
            let map = |client_id: &str| side_ids.get(client_id).cloned();
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
                $(
                    Score::$variant(s) => Some(Score::$variant(
                        crate::sports::$module::resolve_ids(s, side_ids, player_ids)?,
                    )),
                )+
            }
        }

        /// Set a score's resolved-players map, whichever variant it is. No-op
        /// for `Score::Simple`/`Score::Sets`, which have no such field.
        pub fn set_score_players(score: &mut Score, resolved: HashMap<String, RosterPreviewPlayer>) {
            match score {
                $( Score::$variant(s) => crate::sports::$module::set_players(s, resolved), )+
                Score::Simple(_) | Score::Sets(_) => {}
            }
        }

        /// Every player id referenced anywhere in a score — the set
        /// `Api::hydrate_score_players` resolves names for. Empty (nothing to
        /// resolve) for `Score::Simple`/`Score::Sets`, which don't reference
        /// players by id at all.
        pub fn score_player_ids(score: &Score) -> Vec<String> {
            match score {
                $( Score::$variant(s) => crate::sports::$module::player_ids(s), )+
                Score::Simple(_) | Score::Sets(_) => Vec::new(),
            }
        }

        /// Derives the winner (when decidable) from a live-scored match's
        /// persisted score — used by `update_match` to finish a live-scored
        /// match without the client having to work out and resubmit a margin
        /// the server already has enough to compute (see `LiveScoringPage`/
        /// `CricketLiveScoringPage`'s `finishMatch`). `side_ids` is the match's
        /// own two side ids, needed because a tally map only holds entries for
        /// sides that are actually on the board — "absence means zero" — so the
        /// winner comparison needs to know both ids to look up, not just
        /// iterate whatever's present. `None` for `Score::Simple`/`Score::Sets`
        /// — winner derivation for those sports isn't this function's job (the
        /// client always supplies `winner_side_id` itself for a manual
        /// entry/correction on those sports).
        pub fn winner_from_score(score: &Score, side_ids: &[String]) -> Option<String> {
            match score {
                $( Score::$variant(s) => crate::sports::$module::winner(s, side_ids), )+
                Score::Simple(_) | Score::Sets(_) => None,
            }
        }

        /// The incremental live-scoring fast path: folds `new_events` into
        /// `score` in place. `false` if they couldn't be applied — an event for
        /// a different sport than `score`'s (already rejected earlier in
        /// `append_live_events`, so unreachable in practice), or a
        /// `Simple`/`Sets` score (live scoring never creates one).
        pub fn apply_new_live_events(
            score: &mut Score,
            new_events: &[NewLiveEventInput],
            format: Option<&MatchFormatRecord>,
        ) -> bool {
            match score {
                $( Score::$variant(s) => crate::sports::$module::apply_new_events(s, new_events, format).is_some(), )+
                Score::Simple(_) | Score::Sets(_) => false,
            }
        }
    };
}
crate::agon_sports!(define_sport_dispatch);

/// The largest `max_players` at which a side with no team and no custom
/// name is identified by its players' names ("Sam" or "Sam & Alex"). Past
/// this a roster-derived name stops reading as an identity, so a bigger
/// side has to be named (see `create_match`/`update_match`).
pub const ROSTER_IDENTITY_MAX_PLAYERS: u32 = 2;

/// Whether a side capped at `max_players` is small enough to be identified
/// by its roster rather than needing a name. An uncapped side never is.
pub fn has_roster_identity(max_players: Option<u32>) -> bool {
    max_players.is_some_and(|n| n <= ROSTER_IDENTITY_MAX_PLAYERS)
}

/// A side's identity derived purely from who's on it: the lone player's
/// name for a 1-player side, or both players' names joined with "&" for a
/// 2-player side. Only meaningful for a side whose cap passes
/// [`has_roster_identity`] — a bigger side with one early joiner isn't "that
/// player's side", it just hasn't filled up yet, so callers gate this
/// behind that check rather than calling it whenever a side happens to have
/// 1-2 players right now. `None` for any other roster size.
pub fn roster_identity_name<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let names: Vec<&str> = names.collect();
    match names.as_slice() {
        [a] => Some(a.to_string()),
        [a, b] => Some(format!("{a} & {b}")),
        _ => None,
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

/// The ladders a user has opted in to, for their own `/users/me`, ordered by
/// ladder.
///
/// Sorted because the record is a map, whose iteration order is arbitrary, and
/// a list that reshuffles between two reads of an unchanged profile makes a
/// client re-render or diff it for nothing. By ladder rather than by date
/// because ladder names are unique, so the order is total.
pub fn rating_opt_ins_from_record(user: &UserRecord) -> Vec<RatingOptIn> {
    let mut opt_ins: Vec<RatingOptIn> = user
        .rating_opt_ins
        .iter()
        .map(|(ladder, opted_in_at)| RatingOptIn {
            ladder: ladder.clone(),
            opted_in_at: parse_ts(opted_in_at),
        })
        .collect();
    opt_ins.sort_by(|a, b| a.ladder.cmp(&b.ladder));
    opt_ins
}

/// Map the counters common to every sport, deriving win % from matches
/// played. Shared by every sport's own `stats_from_record`
/// (`crate::sports::<sport>`) and the generic sports' entries in
/// `user_stats_from_record`.
pub(crate) fn generic_stats_from_record(rec: &GenericSportStatsRecord) -> GenericPlayerStats {
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

/// Map a stored personal-best entry to the API model.
pub(crate) fn best_figure_from_record(b: &BestFigureRecord) -> BestFigure {
    BestFigure {
        value: b.value as i32,
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
// Match series: SeriesRecord <-> MatchSeries, and the shared
// CreateMatchSeriesEndInput <-> SeriesEndRecord / side templates used by both
// the create input and the `MatchSeries` read model (same reuse pattern as
// `Score`, which also serves both a create-time input and a read field).
// ===========================================================================

pub fn match_series_status_from_str(s: &str) -> MatchSeriesStatus {
    match s {
        "generating" => MatchSeriesStatus::Generating,
        "active" => MatchSeriesStatus::Active,
        "paused" => MatchSeriesStatus::Paused,
        "ended" => MatchSeriesStatus::Ended,
        _ => MatchSeriesStatus::Generating,
    }
}

pub fn match_series_status_str(s: &MatchSeriesStatus) -> &'static str {
    match s {
        MatchSeriesStatus::Generating => "generating",
        MatchSeriesStatus::Active => "active",
        MatchSeriesStatus::Paused => "paused",
        MatchSeriesStatus::Ended => "ended",
    }
}

/// `CreateMatchSeriesEndInput` -> `SeriesEndRecord`.
pub fn series_end_record_from_input(end: &CreateMatchSeriesEndInput) -> SeriesEndRecord {
    match end {
        CreateMatchSeriesEndInput::OnDate(d) => SeriesEndRecord::OnDate {
            date: d.date.clone(),
        },
        CreateMatchSeriesEndInput::AfterN(n) => SeriesEndRecord::AfterN { count: n.count },
        CreateMatchSeriesEndInput::Never(_) => SeriesEndRecord::Never,
    }
}

/// `SeriesEndRecord` -> `CreateMatchSeriesEndInput` (reused as the `ends`
/// field's output type too — see this section's doc comment).
pub fn series_end_input_from_record(rec: &SeriesEndRecord) -> CreateMatchSeriesEndInput {
    match rec {
        SeriesEndRecord::OnDate { date } => {
            CreateMatchSeriesEndInput::OnDate(CreateMatchSeriesEndOnDateInput {
                date: date.clone(),
            })
        }
        SeriesEndRecord::AfterN { count } => {
            CreateMatchSeriesEndInput::AfterN(CreateMatchSeriesEndAfterNInput { count: *count })
        }
        SeriesEndRecord::Never => {
            CreateMatchSeriesEndInput::Never(CreateMatchSeriesEndNeverInput {})
        }
    }
}

pub fn match_series_side_from_record(rec: &SeriesSideRecord) -> MatchSeriesSide {
    MatchSeriesSide {
        client_id: rec.client_id.clone(),
        team_id: rec.team_id.clone(),
        name: rec.name.clone(),
        colour: rec.colour.clone(),
        max_players: rec.max_players,
    }
}

pub fn match_series_from_record(rec: &SeriesRecord) -> MatchSeries {
    MatchSeries {
        id: rec.id.clone(),
        created_by_user_id: rec.created_by_user_id.clone(),
        match_type: match_type_from_tag(&rec.match_type),
        name: rec.name.clone(),
        description: rec.description.clone(),
        location: rec.location.as_ref().map(location_from_record),
        frequency: rec.frequency.clone(),
        weekday: rec.weekday,
        ends: series_end_input_from_record(&rec.ends),
        sides: rec
            .sides
            .iter()
            .map(match_series_side_from_record)
            .collect(),
        status: match_series_status_from_str(&rec.status),
        occurrences_generated: rec.occurrences_generated,
        created_at: parse_ts(&rec.created_at),
    }
}

pub fn match_series_occurrence_from_record(rec: &SeriesOccurrenceRecord) -> MatchSeriesOccurrence {
    MatchSeriesOccurrence {
        index: rec.index,
        starts_at: parse_ts(&rec.starts_at),
        match_id: rec.match_id.clone(),
    }
}

// ===========================================================================
// Score (union) <-> ScoreRecord
// ===========================================================================

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

/// The match an invitation context points at, if it points at one — the id
/// [`invitation_context_from_record`] needs a current record for.
pub fn invitation_context_match_id(rec: &InvitationContextRecord) -> Option<&str> {
    match rec {
        InvitationContextRecord::Match { match_id, .. } => Some(match_id),
        InvitationContextRecord::Team { .. } => None,
    }
}

/// Build the API `InvitationContext`. `matches` holds the current record of
/// each match the caller's contexts point at (see
/// [`invitation_context_match_id`]): a match context carries `ranked` and
/// `rating_ladder`, and those are read from the match when the response is
/// built, never from the invitation — see `InvitationMatchContext::ranked`
/// for why a copy would go stale.
///
/// A match missing from `matches` reads as a friendly. Nothing deletes
/// matches, so that is a record the table has lost rather than a state to
/// design for; failing a whole inbox page over one dangling invitation would
/// be worse than leaving it unmarked, and a match that doesn't exist rates
/// nobody.
pub fn invitation_context_from_record(
    rec: &InvitationContextRecord,
    matches: &std::collections::HashMap<String, MatchRecord>,
) -> InvitationContext {
    match rec {
        InvitationContextRecord::Match {
            match_id,
            match_name,
        } => {
            let current = matches.get(match_id);
            InvitationContext::Match(InvitationMatchContext {
                match_id: match_id.clone(),
                match_name: match_name.clone(),
                ranked: current.is_some_and(|m| m.ranked),
                rating_ladder: current.and_then(rating_ladder),
            })
        }
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
/// stored invitation record. `matches` must hold the invitation's match, if it
/// has one, for the context's `ranked` flag — see
/// [`invitation_context_from_record`].
pub fn invitation_detail_from_record(
    rec: &InvitationRecord,
    matches: &std::collections::HashMap<String, MatchRecord>,
) -> crate::membership::InvitationDetail {
    crate::membership::InvitationDetail {
        invitation: invitation_from_record(rec),
        context: invitation_context_from_record(&rec.context, matches),
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

/// `match_record` supplies `authority` and `created_by_user_id` — see
/// `agon_service::main::caller_match_authority_role`, the single place that
/// lookup is implemented; this mirrors it per-player rather than importing
/// it, since `main.rs` already depends on this module.
pub fn match_player_from_record(
    rec: &MatchPlayerRecord,
    match_record: &MatchRecord,
) -> MatchPlayer {
    let authority_role = rec.user_id.as_deref().and_then(|uid| {
        match_record.authority.get(uid).copied().or_else(|| {
            (match_record.created_by_user_id == uid).then_some(MatchPlayerRoleRecord::Owner)
        })
    });
    MatchPlayer {
        member: member_from_parts(
            &rec.player_id,
            rec.user_id.as_deref(),
            rec.display_name.as_deref(),
            rec.invitation.as_ref(),
        ),
        side_id: rec.side_id.clone(),
        is_member_of_team: rec.is_member_of_team,
        role: match_player_role_from_record(
            authority_role.unwrap_or(MatchPlayerRoleRecord::Player),
        ),
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
        colour: rec.colour.clone(),
        max_players: rec.max_players,
        team_join_enabled: rec.team_join_enabled,
        // Live-overwritten for `Match` (`Api::hydrate_sides`); left as
        // the denormalized cache value for a feed's `FeedMatch`/a search
        // hit's `SearchMatch`, same as `roster_preview` below.
        player_count: rec.player_count,
        // Filled in afterward: live team-meta lookup for `Match`
        // (`Api::hydrate_sides`), or the same batch for a feed's
        // `FeedMatch`/a search hit's `SearchMatch`
        // (`Api::hydrate_sides_from_cache`).
        team_logo: None,
        // Filled in afterward, alongside `team_logo`: same live/cached
        // lookup as above.
        team_name: None,
        // Filled in afterward: live from `players` for `Match`
        // (`Api::hydrate_sides`), or from the denormalized cache for a
        // feed's `FeedMatch` (`feed_roster_preview`, below).
        roster_preview: None,
    }
}

/// Build a feed side's `roster_preview` from the denormalized cache
/// (`MatchSideRecord::player_count`/`roster_preview`) — the feed's
/// counterpart to `Api::hydrate_sides`'s live computation, since a feed
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
// Location: Location (API) <-> LocationRecord (DAO). Both are the same flat
// shape today, but kept as explicit functions (rather than field-for-field
// literals repeated at each call site) so a future divergence is a
// one-place change.
// ===========================================================================

pub fn location_to_record(loc: &Location) -> LocationRecord {
    LocationRecord {
        text: loc.text.clone(),
        latitude: loc.latitude,
        longitude: loc.longitude,
        place_id: loc.place_id.clone(),
        address: loc.address.clone(),
    }
}

pub fn location_from_record(rec: &LocationRecord) -> Location {
    Location {
        text: rec.text.clone(),
        latitude: rec.latitude,
        longitude: rec.longitude,
        place_id: rec.place_id.clone(),
        address: rec.address.clone(),
    }
}

// ===========================================================================
// Match aggregate
// ===========================================================================

/// The ladder a match's result counts towards, for the API: `Some` only when
/// the match is ranked.
///
/// Derived on every read from `match_type` through `rating::ladder_for_tag`
/// rather than stored, so it can never disagree with the ladder the rating
/// pipeline picks for the same match. A friendly answers `None` even in a
/// sport that has a ladder — the field says where *this* result goes, not
/// what the sport could do.
///
/// A ranked match in a sport with no ladder answers `None` too. The API
/// refuses to create one (`unrankable_sport_reason`), so that arm means a
/// record the API didn't write, and `None` is the honest answer for it:
/// nothing would rate it.
pub fn rating_ladder(rec: &MatchRecord) -> Option<String> {
    rating_ladder_for(&rec.match_type, rec.ranked)
}

/// [`rating_ladder`] for a match state that isn't a stored record: a sport tag
/// and whether it would be ranked. `update_match` needs the ladder of the state
/// a PATCH would leave the match in, before anything is written.
pub fn rating_ladder_for(match_type: &str, ranked: bool) -> Option<String> {
    if !ranked {
        return None;
    }
    ladder_for_tag(match_type).map(|ladder| ladder.as_str().to_owned())
}

/// Why a match in sport `match_type` (a stored tag) can't be ranked, or
/// `None` if it can. A sport rates only if `rating::ladder_for_tag` gives it
/// a ladder, and `other` deliberately has none (see `rating::ladder_for`) —
/// asking the ladder rather than naming `other` keeps this check and the
/// rating engine from ever disagreeing. One message for both `create_match`
/// and `update_match`, so the two refusals read the same.
pub fn unrankable_sport_reason(match_type: &str) -> Option<String> {
    ladder_for_tag(match_type)
        .is_none()
        .then(|| format!("a `{match_type}` match can't be ranked: that sport has no rating ladder"))
}

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
        location: rec.location.as_ref().map(location_from_record),
        header_photos: rec
            .header_photos
            .iter()
            .map(|p| Photo {
                image_url: p.url.clone(),
                asset_id: Some(p.asset_id.clone()),
            })
            .collect(),
        sides: sides.iter().map(match_side_from_record).collect(),
        players: players
            .iter()
            .map(|p| match_player_from_record(p, rec))
            .collect(),
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
        ranked: rec.ranked,
        rating_ladder: rating_ladder(rec),
        // Set by the caller right after this call, from the same aggregate
        // this function already consumed (see `caller_match_role`'s call
        // sites in `main.rs`) — a placeholder here, since this pure mapping
        // function has no viewer to resolve it against.
        viewer_role: None,
        viewer_team_join_side_ids: None,
        series_id: rec.series_id.clone(),
    }
}

/// Whether a match's `ranked` flag is still free to change, and if not, why.
///
/// `None` means it may still change; `Some(reason)` is the message the caller
/// gets back. The rule is that the choice has to be made **before the result
/// is knowable** — otherwise you log a game, see how it went, and only then
/// decide whether it counts towards your rating.
///
/// Three closing conditions, in this order. A submitted score first, because
/// it is the one that matters, read off the record's own score fields. That
/// alone is not enough: **a dispute clears `pending_score`**, so a match that
/// has been scored and disputed has neither score set and would otherwise
/// reopen — on precisely the match whose result everyone has already seen.
/// `status != "scheduled"` closes that (and covers live scoring having
/// started, and a cancelled match), and is the same test the format lock in
/// `update_match` uses, so the two locks agree about what "scoring has
/// started" means. The clock comes last because a match can be scored before
/// its own start time.
///
/// **This judges one match state.** `update_match` must not call it on the
/// stored record alone: a PATCH carries its own state change, and the flag
/// has to be closed against both. See [`ranked_lock_reason_for_update`], the
/// only thing that endpoint should use.
///
/// It lives here rather than beside its caller because the rule needs unit
/// tests over `MatchRecord`s, which is what this module's tests build, while
/// `main.rs`'s one test module covers token scopes.
#[must_use]
pub fn ranked_lock_reason(
    rec: &MatchRecord,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'static str> {
    if rec.confirmed_score.is_some() || rec.pending_score.is_some() {
        return Some("a score has already been submitted");
    }
    if rec.status != "scheduled" {
        return Some("the match is no longer scheduled");
    }
    if parse_ts(&rec.starts_at) <= now {
        return Some("the match has already started");
    }
    None
}

/// The same lock, judged for a PATCH: against the match as stored **and** the
/// state this one request would leave it in.
///
/// Neither half is redundant.
///
/// *Stored*, because otherwise a request that moved `starts_at` back into the
/// future and flipped `ranked` in the same call would unlock itself — the
/// resulting state of that request is an innocent scheduled match.
///
/// *Resulting*, because the stored state alone lets one PATCH walk through
/// the lock and close it behind itself. `{ score, ranked: false }` on a match
/// still stored `scheduled` with a future start passes — nothing has been
/// submitted *yet* — and that same request then records the score. That is
/// the "log the game, see that you lost, then de-rank it" case the lock
/// exists for, reachable without trickery because playing before the
/// scheduled slot is ordinary. The first version of this lock read the stored
/// record only and had exactly that hole.
///
/// The three resulting-state conditions mirror the stored ones one for one,
/// so a PATCH can't reach in one step a state it couldn't reach in two. Two
/// steps — the flag first, the score after — stay allowed on an unplayed
/// match, and have to: nothing distinguishes that order from choosing before
/// playing.
///
/// `resulting_status` and `resulting_starts_at` are what `update_match`
/// already computes for the rest of its validation, passed in rather than
/// re-derived so the lock can't disagree with the write about what the
/// request does. `submits_score` is whether the request carries a `score` at
/// all: on a match with no score yet (the only kind that gets this far), any
/// score is a new submission.
#[must_use]
pub fn ranked_lock_reason_for_update(
    stored: &MatchRecord,
    resulting_status: &str,
    resulting_starts_at: &str,
    submits_score: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'static str> {
    if let Some(reason) = ranked_lock_reason(stored, now) {
        return Some(reason);
    }
    if submits_score {
        return Some("this request submits a score");
    }
    if resulting_status != "scheduled" {
        return Some("this request takes the match out of scheduled");
    }
    if parse_ts(resulting_starts_at) <= now {
        return Some("this request moves the start time into the past");
    }
    None
}

/// What has to happen about a caller's rating consent before they get into a
/// ranked match. Decided by [`rating_consent_step`]; `main.rs`'s
/// `settle_rating_consent` carries it out.
#[derive(Debug, PartialEq, Eq)]
pub enum RatingConsentStep {
    /// Let them in: they're already opted in to the match's ladder.
    Proceed,
    /// Record their opt-in to this ladder, then let them in.
    OptIn(String),
    /// Refuse, with this message: not opted in, and no consent given.
    Refuse(String),
}

/// Whether `caller` may get into a match ranked on `ladder` (by creating it
/// as a player, accepting an invitation to it, joining it, or joining its
/// waitlist), and what has to happen first. `consent` is the request's
/// `rating_consent`.
///
/// Only ever asked about a ranked match: `ladder` is its `rating_ladder`, which
/// a friendly doesn't have. That is the whole of "a friendly ignores consent
/// and never opts anyone in", with no branch of its own here to get wrong.
///
/// Being opted in wins over `consent` either way. A second ranked game in the
/// same sport asks nothing, and ticking the box again isn't a second opt-in:
/// `Dao::opt_in_to_rating` would keep the first date regardless, but there's
/// no reason to spend its two round trips finding that out.
#[must_use]
pub fn rating_consent_step(ladder: &str, caller: &UserRecord, consent: bool) -> RatingConsentStep {
    if caller.has_opted_in(ladder) {
        RatingConsentStep::Proceed
    } else if consent {
        RatingConsentStep::OptIn(ladder.to_owned())
    } else {
        RatingConsentStep::Refuse(format!(
            "this is a ranked {ladder} match, and playing in it starts your {ladder} rating: \
             send `rating_consent: true` to agree to that"
        ))
    }
}

/// The players a match ranked on some ladder can't have in it, sorted by why.
/// Built by [`unratable_players`]; empty means all of them can play.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UnratablePlayers {
    /// Players with no Agon account, by display name. Nobody can consent to a
    /// rating for them and there's no profile to keep one on, so they can't be
    /// in a ranked match at all.
    pub without_account: Vec<String>,
    /// Linked players who haven't opted in to the ladder, by name.
    pub not_opted_in: Vec<String>,
}

impl UnratablePlayers {
    fn is_empty(&self) -> bool {
        self.without_account.is_empty() && self.not_opted_in.is_empty()
    }
}

/// Sort `players` into those who couldn't take a spot in a match ranked on
/// `ladder`. Each player is a `(user_id, display_name)` pair, the part of a
/// roster row, an `added_players` entry or a waitlist entry this needs.
/// `users` holds the profiles of the linked ones, from one `batch_get_users`.
///
/// A user id with no profile in `users` counts as not opted in: nothing says
/// they are, and the check is there to keep out exactly the players nothing
/// vouches for.
///
/// This is for the paths that put *someone else* in a ranked match (an
/// organiser's `added_players`, switching a match to ranked, moving someone in
/// off the waitlist), which is why it refuses rather than asks: consent has to
/// come from the player, and those paths have no way to ask them. The caller's
/// own way in is [`rating_consent_step`].
pub fn unratable_players<'a>(
    ladder: &str,
    players: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
    users: &std::collections::HashMap<String, UserRecord>,
) -> UnratablePlayers {
    let mut unratable = UnratablePlayers::default();
    for (user_id, display_name) in players {
        match user_id {
            None => unratable
                .without_account
                .push(display_name.unwrap_or("an unnamed player").to_owned()),
            Some(id) => match users.get(id) {
                Some(user) if user.has_opted_in(ladder) => {}
                Some(user) => unratable.not_opted_in.push(user.name.clone()),
                None => unratable
                    .not_opted_in
                    .push(display_name.unwrap_or(id).to_owned()),
            },
        }
    }
    unratable
}

/// The refusal for an organiser's `added_players` on a ranked match, or `None`
/// if everyone listed may be added.
///
/// Adding someone directly is the one way into a match that doesn't go through
/// the person being added, so it's the one that can't collect their consent.
/// Hence "invite them instead": an invitee consents when they accept.
#[must_use]
pub fn ranked_added_players_reason(ladder: &str, unratable: &UnratablePlayers) -> Option<String> {
    if !unratable.without_account.is_empty() {
        return Some(format!(
            "players without an Agon account can't be added to a ranked match: {}",
            unratable.without_account.join(", ")
        ));
    }
    if !unratable.not_opted_in.is_empty() {
        return Some(format!(
            "players who haven't opted in to {ladder} ratings can't be added straight into a \
             ranked match; invite them instead, and they can opt in when they accept: {}",
            unratable.not_opted_in.join(", ")
        ));
    }
    None
}

/// The refusal for switching a match to ranked, or `None` if everyone it
/// would rate or could move in can be rated on `ladder`.
///
/// `in_a_spot` is judged because those are the players the match would rate.
/// Pending invitees aren't: they're asked for consent when they accept.
///
/// `waitlisted` is judged too, which a first version of this rule left out.
/// A player waiting on a friendly that turns ranked would otherwise have no
/// way to consent on it: joining the waitlist again is a 409, an invite-derived
/// entry's invitation is already accepted so there's nothing left to accept,
/// and move-in refuses them because an admin can't consent for them. Refusing
/// the switch instead keeps a ranked match's waitlist to opted-in players only.
/// The organiser can remove the entry, and the player can come back by a way
/// in that asks them. (Recording an opt-in as a side effect of the repeat
/// waitlist join was the alternative, and was rejected as surprising.)
///
/// Players in a spot are reported ahead of the waitlist, and within each,
/// players without an account ahead of the not-opted-in, because nothing a
/// not-opted-in player could do (be invited, opt in) helps the former.
#[must_use]
pub fn switch_to_ranked_reason(
    ladder: &str,
    in_a_spot: &UnratablePlayers,
    waitlisted: &UnratablePlayers,
) -> Option<String> {
    if !in_a_spot.without_account.is_empty() {
        return Some(format!(
            "a match with players who have no Agon account can't be ranked; remove them first: {}",
            in_a_spot.without_account.join(", ")
        ));
    }
    if !in_a_spot.not_opted_in.is_empty() {
        return Some(format!(
            "a match can't become ranked until everyone taking a spot in it has opted in to \
             {ladder} ratings, and these players haven't: {}",
            in_a_spot.not_opted_in.join(", ")
        ));
    }
    if !waitlisted.is_empty() {
        let names: Vec<&str> = waitlisted
            .without_account
            .iter()
            .chain(&waitlisted.not_opted_in)
            .map(String::as_str)
            .collect();
        return Some(format!(
            "a match can't become ranked while players on its waitlist haven't opted in to \
             {ladder} ratings, because nobody could move them in; remove them from the waitlist \
             first, and they'll be asked to agree when they next join or accept an invitation: {}",
            names.join(", ")
        ));
    }
    None
}

/// The refusal for moving a waitlisted player into a spot on a ranked match,
/// or `None` if they can be rated on `ladder`.
///
/// [`switch_to_ranked_reason`] keeps a ranked match's waitlist to opted-in
/// players, and joining the waitlist of a ranked match asks for consent, so
/// this is a backstop: a waitlist join that raced a switch to ranked, or an
/// entry from before ranked matches asked for consent at all. The organiser
/// moving them in can't consent for them, so they have to opt in themselves
/// first.
///
/// A waitlist entry always names an account (`MatchWaitlistEntryRecord`), so
/// `without_account` is empty in practice. It's handled anyway, so that if that
/// ever changes the refusal stays right rather than reading an external as fine.
#[must_use]
pub fn ranked_move_in_reason(ladder: &str, unratable: &UnratablePlayers) -> Option<String> {
    if !unratable.without_account.is_empty() {
        return Some(
            "that player has no Agon account, so they can't take a spot in a ranked match".into(),
        );
    }
    unratable.not_opted_in.first().map(|name| {
        format!(
            "{name} hasn't opted in to {ladder} ratings, so they can't take a spot in a ranked \
             match. Nobody can opt in on their behalf: remove them from the waitlist and invite \
             them instead, and they can agree to it when they accept"
        )
    })
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
    viewer_is_going: bool,
    viewer_invitation_pending: bool,
    viewer_can_join_side_ids: Vec<String>,
    i_liked: bool,
) -> FeedMatch {
    FeedMatch {
        id: rec.id.clone(),
        name: rec.name.clone(),
        description: rec.description.clone(),
        match_type: match_type_from_tag(&rec.match_type),
        status: match_status_from_str(&rec.status),
        starts_at: parse_ts(&rec.starts_at),
        location: rec.location.as_ref().map(location_from_record),
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
        viewer_is_going,
        viewer_invitation_pending,
        viewer_can_join_side_ids,
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
        total_player_count: rec.total_player_count as u32,
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
        location: rec.location.as_ref().map(location_from_record),
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

// `match_score_to_record`/`match_score_from_record` are generated by
// `define_sport_dispatch!` above.

// ===========================================================================
// Match format: MatchFormat (API, poem-openapi) <-> MatchFormatRecord (DAO,
// plain serde) — `match_format_to_record`/`match_format_from_record`/
// `match_format_sport_tag` are generated by `define_sport_dispatch!` above.
// ===========================================================================

// ===========================================================================
// Live scoring: LiveEventInput (API, poem-openapi) <-> LiveEventPayloadRecord
// (DAO, plain serde) — `live_event_sport_tag`/`live_event_input_to_record`/
// `live_event_payload_from_record` are generated by `define_sport_dispatch!`
// above.
// ===========================================================================

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

// `derive_live_score` is generated by `define_sport_dispatch!` above.

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
        stats: user_stats_from_record(&UserStatsRecord::default()),
        follower_count: 0,
        following_count: 0,
        is_followed_by_me: false,
    }
}

/// Build the API `Notification` from a record, given the resolved actor profile
/// (already hydrated by the caller). `matches` holds the current record of the
/// match behind each accepted-invitation notification on the page, for its
/// context's `ranked` flag — see [`invitation_context_from_record`].
pub fn notification_from_record(
    rec: &NotificationRecord,
    actor: UserProfile,
    matches: &std::collections::HashMap<String, MatchRecord>,
) -> Notification {
    let kind = match &rec.kind {
        NotificationKindRecord::MatchInvitation {
            invitation_id,
            match_id,
            match_name,
            status,
            ..
        } => NotificationKind::MatchInvitation(MatchInvitationNotification {
            inviter: actor,
            invitation_id: invitation_id.clone(),
            match_id: match_id.clone(),
            match_name: match_name.clone(),
            status: invitation_status_from_str(status),
        }),
        NotificationKindRecord::TeamInvitation {
            invitation_id,
            team_id,
            team_name,
            status,
            ..
        } => NotificationKind::TeamInvitation(TeamInvitationNotification {
            inviter: actor,
            invitation_id: invitation_id.clone(),
            team_id: team_id.clone(),
            team_name: team_name.clone(),
            status: invitation_status_from_str(status),
        }),
        NotificationKindRecord::InvitationAccepted {
            invitation_id,
            context,
            ..
        } => NotificationKind::InvitationAccepted(InvitationAcceptedNotification {
            accepted_by: actor,
            invitation_id: invitation_id.clone(),
            context: invitation_context_from_record(context, matches),
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
    use poem_openapi::types::ToJSON;

    /// Round-tripping through the DAO mirror must reproduce the same wire
    /// JSON as the original — the property that actually matters here, since
    /// a mapping bug is a value silently changing shape or going missing.
    /// Compared via JSON rather than a derived `PartialEq` (the API types
    /// don't derive it) so this doesn't need extra derives just for testing.
    /// Each sport's own cases live alongside that sport's types in
    /// `crate::sports::{football,cricket,netball}`'s own tests — this only
    /// covers the two sport-agnostic shapes (`Simple`/`Sets`) that live here
    /// in `mapping.rs` itself.
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
        ];

        for score in scores {
            let original_json = score.to_json();
            let round_tripped = score_from_record(&score_to_record(&score));
            assert_eq!(original_json, round_tripped.to_json());
        }
    }

    // -----------------------------------------------------------------------
    // Ranked matches: the ladder, and the lock on changing the flag
    // -----------------------------------------------------------------------

    /// A ranked squash match nobody has played yet, starting at `starts_at`.
    fn scheduled_match(starts_at: &str) -> MatchRecord {
        MatchRecord {
            id: "m1".to_string(),
            created_by_user_id: "u1".to_string(),
            name: "Tuesday".to_string(),
            description: String::new(),
            match_type: "squash".to_string(),
            status: "scheduled".to_string(),
            starts_at: starts_at.to_string(),
            allow_unassigned: true,
            total_player_count: 0,
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
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    /// The "now" the lock tests judge against: midday on the day the fixtures
    /// are scheduled.
    fn lock_now() -> chrono::DateTime<chrono::Utc> {
        parse_ts("2026-06-01T12:00:00.000Z")
    }

    /// The flag is free to change right up to the start. Anything stricter
    /// would stop an organiser correcting a mis-click on a match nobody has
    /// played yet, which is the only time the choice is genuinely free.
    #[test]
    fn ranked_is_open_until_the_match_starts() {
        assert_eq!(
            ranked_lock_reason(&scheduled_match("2026-06-01T18:00:00.000Z"), lock_now()),
            None
        );
    }

    /// The bug the lock exists to prevent: play the game, see the result, and
    /// only *then* decide whether it counts. Once `starts_at` has passed the
    /// outcome may be known, so the choice is closed even with no score typed
    /// in.
    #[test]
    fn ranked_locks_once_the_match_has_started() {
        assert_eq!(
            ranked_lock_reason(&scheduled_match("2026-06-01T09:00:00.000Z"), lock_now()),
            Some("the match has already started")
        );
    }

    /// Nothing stops a match being scored before its scheduled start, so the
    /// clock alone isn't enough: a submitted score closes the choice by itself.
    #[test]
    fn ranked_locks_on_a_submitted_score_even_before_the_start_time() {
        let mut m = scheduled_match("2026-06-01T18:00:00.000Z");
        m.pending_score = Some(PendingScoreRecord {
            submission_id: "s1".to_string(),
            score: ScoreRecord::Simple {
                entries: HashMap::new(),
            },
            winner_side_id: None,
            confirmations: Vec::new(),
        });
        assert_eq!(
            ranked_lock_reason(&m, lock_now()),
            Some("a score has already been submitted")
        );
    }

    /// What a check on `confirmed_score`/`pending_score` alone would miss: a
    /// dispute *clears* `pending_score`, so a scored-then-disputed match has
    /// neither field set. Without the status arm the lock would reopen on
    /// exactly the match whose result everybody has already seen.
    #[test]
    fn ranked_stays_locked_after_a_submitted_score_is_disputed() {
        let mut m = scheduled_match("2026-06-01T18:00:00.000Z");
        m.status = "completed".to_string();
        assert_eq!(
            ranked_lock_reason(&m, lock_now()),
            Some("the match is no longer scheduled")
        );
    }

    /// **Regression.** The first version of this lock read the stored record
    /// alone, which let a single PATCH walk through it and close it behind
    /// itself: `{ score, ranked: false }` on a match still stored `scheduled`
    /// with a future start recorded the result *and* de-ranked it. That is the
    /// "log the game, see that you lost, then de-rank it" bug itself, and it
    /// needs no trickery — playing before the scheduled slot is ordinary.
    #[test]
    fn ranked_cannot_be_flipped_by_the_same_request_that_submits_the_score() {
        let scheduled = scheduled_match("2026-06-01T18:00:00.000Z");
        assert_eq!(
            ranked_lock_reason(&scheduled, lock_now()),
            None,
            "the stored state alone still says the flag is open, which is the trap"
        );
        assert_eq!(
            ranked_lock_reason_for_update(
                &scheduled,
                "scheduled",
                "2026-06-01T18:00:00.000Z",
                true,
                lock_now()
            ),
            Some("this request submits a score")
        );
    }

    /// The same hole reached without a `score`: starting, completing or
    /// cancelling the match in the same call. Mirrors the stored status arm,
    /// so a PATCH can't reach in one step a state it couldn't reach in two.
    #[test]
    fn ranked_cannot_be_flipped_by_the_same_request_that_moves_the_status_on() {
        for status in ["in_progress", "completed", "cancelled"] {
            assert_eq!(
                ranked_lock_reason_for_update(
                    &scheduled_match("2026-06-01T18:00:00.000Z"),
                    status,
                    "2026-06-01T18:00:00.000Z",
                    false,
                    lock_now()
                ),
                Some("this request takes the match out of scheduled"),
                "{status}"
            );
        }
    }

    /// ...and the clock arm: backdating the match into the past is the same
    /// claim as "it has already been played".
    #[test]
    fn ranked_cannot_be_flipped_by_the_same_request_that_backdates_the_match() {
        assert_eq!(
            ranked_lock_reason_for_update(
                &scheduled_match("2026-06-01T18:00:00.000Z"),
                "scheduled",
                "2026-05-30T09:00:00.000Z",
                false,
                lock_now()
            ),
            Some("this request moves the start time into the past")
        );
    }

    /// The other direction, and why the stored record still counts: pushing
    /// `starts_at` back into the future must not *re*open the flag on a match
    /// that has already started. That request's resulting state is an innocent
    /// scheduled match, so a resulting-state-only lock would let it through.
    #[test]
    fn ranked_cannot_be_reopened_by_rescheduling_a_started_match_into_the_future() {
        assert_eq!(
            ranked_lock_reason_for_update(
                &scheduled_match("2026-06-01T09:00:00.000Z"),
                "scheduled",
                "2026-06-08T18:00:00.000Z",
                false,
                lock_now()
            ),
            Some("the match has already started")
        );
    }

    /// An ordinary edit to a match nobody has played yet leaves the flag open:
    /// the lock must stay off until something genuinely closes it, or an
    /// organiser could never correct a mis-click.
    #[test]
    fn ranked_is_still_open_on_an_unplayed_match_being_edited() {
        assert_eq!(
            ranked_lock_reason_for_update(
                &scheduled_match("2026-06-01T18:00:00.000Z"),
                "scheduled",
                "2026-06-02T18:00:00.000Z",
                false,
                lock_now()
            ),
            None
        );
    }

    /// `rating_ladder` says where *this* result goes, not what the sport could
    /// do: nothing for a friendly, even in a sport with a ladder. A ranked
    /// match in a ladderless sport (which the API refuses to create) gets
    /// nothing either, because nothing would rate it.
    #[test]
    fn a_rating_ladder_is_only_given_for_a_ranked_match() {
        let mut m = scheduled_match("2026-06-01T18:00:00.000Z");
        assert_eq!(rating_ladder(&m).as_deref(), Some("squash"));
        m.ranked = false;
        assert_eq!(rating_ladder(&m), None);
        m.ranked = true;
        m.match_type = "other".to_string();
        assert_eq!(rating_ladder(&m), None);
    }

    /// `other` can't be ranked and every modelled sport can, checked on the
    /// tags `MatchType` really stores. The API's check takes a stored tag
    /// while the engine maps its own `Sport` mirror, so this is what catches
    /// the two drifting apart — a tag the engine didn't recognise would read
    /// as `other` and quietly become unrankable.
    #[test]
    fn only_a_sport_without_a_ladder_is_unrankable() {
        for sport in [
            MatchType::Tennis,
            MatchType::Badminton,
            MatchType::Squash,
            MatchType::TableTennis,
            MatchType::Football,
            MatchType::Cricket,
            MatchType::Netball,
        ] {
            let tag = match_type_tag(&sport);
            assert_eq!(unrankable_sport_reason(tag), None, "{tag}");
        }
        assert!(
            unrankable_sport_reason(match_type_tag(&MatchType::Other))
                .is_some_and(|reason| reason.contains("no rating ladder"))
        );
    }

    /// An invitation's match context reports whether the match is ranked
    /// *now*, read from the match rather than the invitation: the organiser can
    /// switch a match to ranked after inviting people, and the warning on the
    /// accept screen has to follow. A match that can't be found reads as a
    /// friendly instead of failing the page.
    #[test]
    fn an_invitation_context_reports_the_matchs_current_ranked_flag() {
        let context = InvitationContextRecord::Match {
            match_id: "m1".to_string(),
            match_name: "Tuesday".to_string(),
        };
        let resolve = |matches: &HashMap<String, MatchRecord>| {
            let InvitationContext::Match(ctx) = invitation_context_from_record(&context, matches)
            else {
                panic!("expected a match context");
            };
            (ctx.ranked, ctx.rating_ladder)
        };

        let mut m = scheduled_match("2026-06-01T18:00:00.000Z");
        assert_eq!(
            resolve(&HashMap::from([("m1".to_string(), m.clone())])),
            (true, Some("squash".to_string()))
        );
        m.ranked = false;
        assert_eq!(
            resolve(&HashMap::from([("m1".to_string(), m)])),
            (false, None)
        );
        assert_eq!(resolve(&HashMap::new()), (false, None));
    }

    // -----------------------------------------------------------------------
    // Rating consent: who may take a spot in, or wait for, a ranked match
    // -----------------------------------------------------------------------

    /// A profile named `name`, opted in to exactly `ladders`.
    fn user_opted_in_to(id: &str, name: &str, ladders: &[&str]) -> UserRecord {
        UserRecord {
            id: id.to_string(),
            email: format!("{id}@example.com"),
            name: name.to_string(),
            profile_image_url: None,
            follower_count: 0,
            following_count: 0,
            unread_count: 0,
            stats: UserStatsRecord::default(),
            ratings: HashMap::new(),
            rating_opt_ins: ladders
                .iter()
                .map(|ladder| (ladder.to_string(), "2026-09-15T10:00:00.000Z".to_string()))
                .collect(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    /// The case the whole check exists for: a player who isn't opted in and
    /// didn't tick the box is refused, not let in on the strength of the
    /// organiser having made the match ranked. The refusal names the sport and
    /// the flag that answers it, and ticking the box is what opts them in.
    #[test]
    fn a_player_not_opted_in_needs_consent_to_get_into_a_ranked_match() {
        let newcomer = user_opted_in_to("u1", "Sam", &[]);
        let RatingConsentStep::Refuse(message) = rating_consent_step("squash", &newcomer, false)
        else {
            panic!("no opt-in and no consent must be refused");
        };
        assert!(message.contains("squash"), "{message}");
        assert!(message.contains("rating_consent"), "{message}");
        assert_eq!(
            rating_consent_step("squash", &newcomer, true),
            RatingConsentStep::OptIn("squash".to_string())
        );
    }

    /// Consent is per ladder and permanent. An opted-in player gets into every
    /// later ranked game in that sport without being asked, and ticking the box
    /// again isn't a second opt-in. A different sport is a different consent,
    /// and so is a ladder whose name merely starts with one they're opted in
    /// to, which is `has_opted_in`'s exact-key lookup showing through.
    #[test]
    fn consent_is_asked_once_per_ladder() {
        let player = user_opted_in_to("u1", "Sam", &["tennis"]);
        assert_eq!(
            rating_consent_step("tennis", &player, false),
            RatingConsentStep::Proceed
        );
        assert_eq!(
            rating_consent_step("tennis", &player, true),
            RatingConsentStep::Proceed
        );
        for other in ["squash", "tennis:doubles"] {
            assert!(
                matches!(
                    rating_consent_step(other, &player, false),
                    RatingConsentStep::Refuse(_)
                ),
                "{other}"
            );
        }
    }

    /// The paths that put someone *else* in a ranked match can't ask them, so
    /// they sort players instead: no account, linked but not opted in, or fine.
    /// A user id whose profile didn't come back counts as not opted in. The
    /// check exists to keep out players nothing vouches for, so a missing
    /// profile must not read as a pass.
    #[test]
    fn players_are_sorted_by_why_they_cannot_be_rated() {
        let users = HashMap::from([
            (
                "in".to_string(),
                user_opted_in_to("in", "Opted", &["squash"]),
            ),
            (
                "out".to_string(),
                user_opted_in_to("out", "Not Opted", &["tennis"]),
            ),
        ]);
        let unratable = unratable_players(
            "squash",
            [
                (Some("in"), None),
                (Some("out"), None),
                (None, Some("Ringer")),
                (Some("ghost"), None),
                (None, None),
            ],
            &users,
        );
        assert_eq!(
            unratable,
            UnratablePlayers {
                without_account: vec!["Ringer".to_string(), "an unnamed player".to_string()],
                not_opted_in: vec!["Not Opted".to_string(), "ghost".to_string()],
            }
        );
        assert_eq!(
            unratable_players("squash", [(Some("in"), None)], &users),
            UnratablePlayers::default()
        );
    }

    /// Each refusal is silent when there's nobody to refuse, and otherwise
    /// names who. For an organiser's direct add it also names the way round
    /// it: an invitation, which does ask the player. Players without an account
    /// are reported ahead of the not-opted-in, because nothing a not-opted-in
    /// player could do (be invited, opt in) helps them.
    #[test]
    fn the_roster_refusals_name_who_and_what_to_do() {
        let nobody = UnratablePlayers::default();
        assert_eq!(ranked_added_players_reason("squash", &nobody), None);
        assert_eq!(switch_to_ranked_reason("squash", &nobody, &nobody), None);
        assert_eq!(ranked_move_in_reason("squash", &nobody), None);

        let not_opted_in = UnratablePlayers {
            without_account: Vec::new(),
            not_opted_in: vec!["Sam".to_string()],
        };
        let added = ranked_added_players_reason("squash", &not_opted_in).unwrap();
        assert!(added.contains("invite them instead"), "{added}");
        assert!(added.contains("Sam"), "{added}");
        let switch = switch_to_ranked_reason("squash", &not_opted_in, &nobody).unwrap();
        assert!(switch.contains("opted in to squash ratings"), "{switch}");
        assert!(switch.contains("Sam"), "{switch}");
        let move_in = ranked_move_in_reason("squash", &not_opted_in).unwrap();
        assert!(move_in.starts_with("Sam hasn't opted in"), "{move_in}");
        assert!(move_in.contains("invite them"), "{move_in}");

        let both = UnratablePlayers {
            without_account: vec!["Ringer".to_string()],
            not_opted_in: vec!["Sam".to_string()],
        };
        for reason in [
            ranked_added_players_reason("squash", &both).unwrap(),
            switch_to_ranked_reason("squash", &both, &nobody).unwrap(),
        ] {
            assert!(reason.contains("Agon account"), "{reason}");
            assert!(reason.contains("Ringer"), "{reason}");
            assert!(!reason.contains("Sam"), "{reason}");
        }
        assert!(
            ranked_move_in_reason("squash", &both)
                .unwrap()
                .contains("no Agon account")
        );
    }

    /// The gap the re-port onto the separate waitlist closed: a player waiting
    /// on a friendly has no way to consent on it once it turns ranked (a
    /// repeat waitlist join is a 409, and an invite-derived entry has no
    /// invitation left to accept), and move-in would then refuse them for
    /// good. So switching to ranked refuses while anyone waiting isn't opted
    /// in, naming them and the way out. It's a refusal of its own, reported
    /// only once everyone in a spot is fine, so the organiser fixes the roster
    /// first and the waitlist second rather than seeing one message for both.
    #[test]
    fn switching_to_ranked_also_judges_the_waitlist() {
        let nobody = UnratablePlayers::default();
        let waiting = UnratablePlayers {
            without_account: Vec::new(),
            not_opted_in: vec!["Wendy".to_string()],
        };
        let reason = switch_to_ranked_reason("squash", &nobody, &waiting).unwrap();
        assert!(reason.contains("waitlist"), "{reason}");
        assert!(reason.contains("squash"), "{reason}");
        assert!(reason.contains("Wendy"), "{reason}");

        let in_a_spot = UnratablePlayers {
            without_account: Vec::new(),
            not_opted_in: vec!["Sam".to_string()],
        };
        let reason = switch_to_ranked_reason("squash", &in_a_spot, &waiting).unwrap();
        assert!(reason.contains("Sam"), "{reason}");
        assert!(!reason.contains("Wendy"), "{reason}");
    }

    /// `/users/me` lists opt-ins in ladder order with their stored dates,
    /// whatever order the map iterates in, and a profile opted in nowhere lists
    /// nothing.
    #[test]
    fn opt_ins_are_listed_by_ladder_with_their_dates() {
        let mut user = user_opted_in_to("u1", "Sam", &[]);
        assert!(rating_opt_ins_from_record(&user).is_empty());

        user.rating_opt_ins = HashMap::from([
            ("tennis".to_string(), "2026-09-15T10:00:00.000Z".to_string()),
            ("squash".to_string(), "2026-09-01T08:30:00.000Z".to_string()),
        ]);
        let listed: Vec<(String, chrono::DateTime<chrono::Utc>)> =
            rating_opt_ins_from_record(&user)
                .into_iter()
                .map(|opt_in| (opt_in.ladder, opt_in.opted_in_at))
                .collect();
        assert_eq!(
            listed,
            vec![
                ("squash".to_string(), parse_ts("2026-09-01T08:30:00.000Z")),
                ("tennis".to_string(), parse_ts("2026-09-15T10:00:00.000Z")),
            ]
        );
    }
}
