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

/// Voids an earlier event in the same match's log — the only way to correct a
/// mistake. History is never edited or deleted; voiding is itself just
/// another appended event. Shared by every sport's inner union. A `Void`
/// event can't itself be voided.
#[derive(Object)]
pub struct VoidEvent {
    /// The `seq` of the event being voided (see `LiveEvent.seq` on read).
    pub target_seq: u32,
}

/// One event to append, before the server has assigned it a `seq`.
#[derive(Object)]
pub struct NewLiveEventInput {
    /// Client-generated id (e.g. a UUID), unique per match. Carried through
    /// to the stored event so a client can recognise its own event after a
    /// resync without relying on `seq` alone.
    pub client_event_id: String,
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
    /// batch is rejected as a conflict — the concurrency + idempotency gate
    /// for concurrent or retried submissions.
    pub expected_last_seq: u32,
    pub events: Vec<NewLiveEventInput>,
}

/// One event as read back from the log: the input fields plus what the
/// server assigned on append.
#[derive(Object)]
pub struct LiveEvent {
    pub seq: u32,
    pub client_event_id: String,
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

/// Removes voided events (and the `Void` markers themselves) from an ordered
/// `(seq, event)` log, leaving only what should actually be folded into the
/// derived state. `Void` events reference their target by `seq` regardless of
/// sport, so this runs once over the whole log rather than per sport.
pub fn effective_events(events: Vec<(u32, LiveEventInput)>) -> Vec<(u32, LiveEventInput)> {
    let mut voided = std::collections::HashSet::new();
    for (_, event) in &events {
        let target = match event {
            LiveEventInput::Football(FootballLiveEvent::Void(v)) => Some(v.target_seq),
            LiveEventInput::Cricket(CricketLiveEvent::Void(v)) => Some(v.target_seq),
            _ => None,
        };
        if let Some(target_seq) = target {
            voided.insert(target_seq);
        }
    }
    events
        .into_iter()
        .filter(|(seq, event)| {
            !voided.contains(seq)
                && !matches!(
                    event,
                    LiveEventInput::Football(FootballLiveEvent::Void(_))
                        | LiveEventInput::Cricket(CricketLiveEvent::Void(_))
                )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_score::cricket::CricketInningsStartEvent;
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

    #[test]
    fn voiding_a_wrongly_placed_innings_end_re_flows_events_into_the_earlier_innings() {
        use crate::detailed_score::cricket::CricketDelivery;
        use crate::live_score::cricket::{
            CricketInningsEndEvent, CricketLiveEvent, InningsEndReason, derive_state,
        };

        let ball = |runs: u32| CricketDelivery {
            over: 0,
            ball: 1,
            bowler_player_id: "patel".into(),
            striker_player_id: "sharma".into(),
            non_striker_player_id: "verma".into(),
            runs_off_bat: runs,
            extra: None,
            wicket: None,
        };

        // A scorer wrongly taps "end innings" after one ball, immediately
        // voids it, and carries on scoring the same innings.
        let raw = vec![
            (
                1,
                LiveEventInput::Cricket(CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                    batting_side_id: "warriors".into(),
                    bowling_side_id: "mill_lane".into(),
                })),
            ),
            (
                2,
                LiveEventInput::Cricket(CricketLiveEvent::Delivery(ball(4))),
            ),
            (
                3,
                LiveEventInput::Cricket(CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                    reason: InningsEndReason::Declared,
                })),
            ),
            (
                4,
                LiveEventInput::Cricket(CricketLiveEvent::Void(VoidEvent { target_seq: 3 })),
            ),
            (
                5,
                LiveEventInput::Cricket(CricketLiveEvent::Delivery(ball(2))),
            ),
        ];

        let effective = effective_events(raw);
        let cricket_events: Vec<_> = effective
            .into_iter()
            .filter_map(|(_, e)| match e {
                LiveEventInput::Cricket(c) => Some(c),
                _ => None,
            })
            .collect();
        let state = derive_state(&cricket_events);

        assert_eq!(
            state.innings.len(),
            1,
            "the voided end must not split the log into two innings"
        );
        assert_eq!(state.innings[0].runs, 6);
        assert!(
            !state.awaiting_next_innings,
            "the innings is still open once its End is voided away"
        );
    }

    #[test]
    fn voiding_strips_target_and_the_void_marker_itself() {
        let events = vec![
            (
                1,
                LiveEventInput::Cricket(CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                    batting_side_id: "a".into(),
                    bowling_side_id: "b".into(),
                })),
            ),
            (
                2,
                LiveEventInput::Cricket(CricketLiveEvent::Void(VoidEvent { target_seq: 1 })),
            ),
        ];
        let effective = effective_events(events);
        assert!(
            effective.is_empty(),
            "both the target and the void itself should be gone"
        );
    }
}
