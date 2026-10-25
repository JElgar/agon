//! The API side of a modeled sport: the [`Sport`] trait, implemented for the
//! sport's marker type from `agon_core::sports::<sport>`. See
//! `agon_core::sports` for the full list of steps to add a sport and the
//! registry every sport-shaped API type is generated from (`MatchType`,
//! `Score`, `MatchFormat`, `LiveEventInput`, `UserStats` and the dispatch in
//! `mapping.rs`).
//!
//! `netball.rs` is the smallest complete example.

use std::collections::HashMap;

use agon_core::sports::SportRecord;
use chrono::{DateTime, Utc};
use poem_openapi::types::{ParseFromJSON, ToJSON, Type};

use crate::RosterPreviewPlayer;

pub mod cricket;
pub mod football;
pub mod netball;

/// Bounds every API type of a sport shares, so it can sit in the generated
/// poem-openapi unions and structs.
pub(crate) trait ApiType: Type + ParseFromJSON + ToJSON {}
impl<T: Type + ParseFromJSON + ToJSON> ApiType for T {}

/// Everything the API needs from a modeled sport: its API types, how live
/// events fold into a score, and how each API type maps to its stored record
/// (the associated types of [`SportRecord`]).
pub(crate) trait Sport: SportRecord {
    /// The sport's score: its `Score` variant. Serves as the confirmable
    /// result, the persisted live score and the live-poll response.
    type Score: ApiType;
    /// The sport's match format: its `MatchFormat` variant.
    type Format: ApiType + Clone;
    /// One live-scoring event: its `LiveEventInput` variant.
    type LiveEvent: ApiType + Clone;
    /// A user's lifetime stats: its `UserStats` field.
    type PlayerStats: ApiType;

    /// The score before any live event has been recorded.
    fn new_score() -> Self::Score;

    /// Folds one live event into `score`. `occurred_at` is the recording
    /// device's clock, which is the authoritative time for the event.
    fn apply_event(
        score: &mut Self::Score,
        occurred_at: DateTime<Utc>,
        event: &Self::LiveEvent,
        format: Option<&Self::Format>,
    );

    /// Folds a whole ordered event log into a score. Appends use
    /// [`Sport::apply_event`] directly, so both paths share one fold.
    fn from_events(
        events: &[(DateTime<Utc>, Self::LiveEvent)],
        format: Option<&Self::Format>,
    ) -> Self::Score {
        let mut score = Self::new_score();
        for (occurred_at, event) in events {
            Self::apply_event(&mut score, *occurred_at, event, format);
        }
        score
    }

    /// The winning side, when the score decides one. `side_ids` is the
    /// match's own sides, since a side with nothing scored may be absent
    /// from the score.
    fn winner(score: &Self::Score, side_ids: &[String]) -> Option<String>;

    /// Every side id the score references, to validate a submitted score
    /// against the match's sides.
    fn side_ids(score: &Self::Score) -> Vec<&str>;

    /// Every player id the score references (repeats allowed), for
    /// resolving player names.
    fn player_ids(score: &Self::Score) -> Vec<String>;

    /// Re-points a score submitted at match creation from the request's
    /// client ids to the real side and player ids. `None` if it references
    /// an unknown side or player.
    fn resolve_ids(
        score: &Self::Score,
        side_ids: &HashMap<String, String>,
        player_ids: &HashMap<String, String>,
    ) -> Option<Self::Score>;

    /// Attaches the resolved players (keyed by player id) to the score.
    fn set_players(score: &mut Self::Score, players: HashMap<String, RosterPreviewPlayer>);

    fn score_to_record(score: &Self::Score) -> Self::ScoreRecord;
    fn score_from_record(rec: &Self::ScoreRecord) -> Self::Score;
    fn format_to_record(format: &Self::Format) -> Self::FormatRecord;
    fn format_from_record(rec: &Self::FormatRecord) -> Self::Format;
    fn live_event_to_record(event: &Self::LiveEvent) -> Self::LiveEventRecord;
    fn live_event_from_record(rec: &Self::LiveEventRecord) -> Self::LiveEvent;
    fn stats_from_record(rec: &Self::StatsRecord) -> Self::PlayerStats;
}
