//! Live-scoring event vocabulary: a per-sport, per-kind discriminated union
//! appended to a match's event log while it's being scored ball-by-ball (or
//! goal-by-goal, card-by-card, ...) in real time — possibly in batches, from a
//! device catching up after being offline.
//!
//! Distinct from `detailed_score`, which is the *resolved* scorecard read off
//! the derived state. This module is the write-side vocabulary, plus the pure
//! functions (in `football`/`cricket`) that fold an ordered event log into
//! that derived state — the same shapes `detailed_score` already defines,
//! since a live match's final scorecard is exactly what live scoring builds
//! up to.
//!
//! Corrections are direct mutations of the log, not layered on top of it:
//! `DELETE /matches/:id/live/events/:seq` removes an event outright, `PATCH`
//! overwrites one seq's content in place. There's no soft-delete/void marker
//! to filter out on every fold — `derive_state` folds whatever the DAO
//! actually returns, in order.

use poem_openapi::{Object, Union};

pub mod cricket;
pub mod football;

pub use cricket::CricketLiveEvent;
pub use football::FootballLiveEvent;

/// A single live-scoring event, sport-first discriminated so a new sport is a
/// new variant without touching existing ones — same pattern as
/// `detailed_score::DetailedScore`. See `football`/`cricket` for the per-kind
/// union nested inside each variant.
#[derive(Union)]
#[oai(one_of, discriminator_name = "sport")]
pub enum LiveEventInput {
    Football(FootballLiveEvent),
    Cricket(CricketLiveEvent),
}

/// One event to append, before the server has assigned it a `seq`.
#[derive(Object)]
pub struct NewLiveEventInput {
    /// When this actually happened on the recording device — may be well
    /// before the server receives it, if the device was offline.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub event: LiveEventInput,
}

/// A batch append: up to `MAX_EVENTS_PER_BATCH` events, appended atomically,
/// so a device with an offline backlog can catch up in one call per chunk.
#[derive(Object)]
pub struct AppendLiveEventsInput {
    /// The seq this device last saw for this match (0 if it has never synced
    /// this match's log). Must match the server's current tip or the whole
    /// batch is rejected as a conflict, rather than silently reordering or
    /// duplicating events. On conflict, the caller fetches the log since its
    /// old tip and diffs it against what it was about to send: identical
    /// content means its own (already-applied) submission just lost its
    /// response, so those events are dropped from the retry; a mismatch is a
    /// real conflict (another device, a genuine double-entry) for the caller
    /// to resolve before resubmitting whatever's left.
    pub expected_last_seq: u32,
    pub events: Vec<NewLiveEventInput>,
}

/// One event as read back from the log: the input fields plus what the
/// server assigned on append.
#[derive(Object)]
pub struct LiveEvent {
    pub seq: u32,
    pub recorded_by_user_id: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub event: LiveEventInput,
}

/// The derived live-scoring state for a match, sport-first discriminated like
/// `LiveEventInput`. A full fold of the event log — the same shape
/// `detailed_score` exposes once a match is done, just kept live.
#[derive(Union)]
#[oai(one_of, discriminator_name = "sport")]
pub enum LiveScoreState {
    Football(football::FootballLiveState),
    Cricket(cricket::CricketLiveState),
}

/// `LiveScoreState` plus the log position it was derived from, so a client
/// can tell whether its own queued-but-unsynced events are already reflected.
#[derive(Object)]
pub struct LiveScoreSnapshot {
    pub last_seq: u32,
    pub state: LiveScoreState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_score::football::{FootballCardColor, FootballCardEvent};
    use poem_openapi::types::{ParseFromJSON, ToJSON};

    /// The outer (`sport`) and inner (`kind`) unions must serialize as one
    /// flat JSON object carrying both discriminators, and must round-trip —
    /// this is the load-bearing assumption behind nesting a `Union` inside
    /// another `Union`'s variant.
    #[test]
    fn nested_union_serializes_flat_and_round_trips() {
        let event = LiveEventInput::Football(FootballLiveEvent::Card(FootballCardEvent {
            side_id: "side_a".into(),
            player_id: "p1".into(),
            color: FootballCardColor::Yellow,
            minute: Some(58),
        }));

        let json = event.to_json().expect("serializes");
        let obj = json.as_object().expect("flat object, not a wrapper");
        // Variant names are the discriminator values as-is (no rename_all is
        // set), matching the existing `DetailedScore`/`Score` unions.
        assert_eq!(obj.get("sport").and_then(|v| v.as_str()), Some("Football"));
        assert_eq!(obj.get("kind").and_then(|v| v.as_str()), Some("Card"));
        assert_eq!(obj.get("side_id").and_then(|v| v.as_str()), Some("side_a"));

        let parsed = match LiveEventInput::parse_from_json(Some(json)) {
            Ok(v) => v,
            Err(_) => panic!("failed to parse back"),
        };
        match parsed {
            LiveEventInput::Football(FootballLiveEvent::Card(c)) => {
                assert_eq!(c.player_id, "p1");
                assert_eq!(c.minute, Some(58));
            }
            _ => panic!("wrong variant"),
        }
    }
}
