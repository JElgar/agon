//! Live-scoring event vocabulary: a per-sport, per-kind discriminated union
//! appended to a match's event log while it's being scored ball-by-ball (or
//! goal-by-goal, card-by-card, ...) in real time — possibly in batches, from a
//! device catching up after being offline.
//!
//! This is the write-side vocabulary, plus the pure functions (in
//! `football`/`cricket`) that fold an ordered event log into `Score`'s
//! optional rich-detail fields — there's no separate "live" read shape:
//! `GET /matches/:id/score` serves the same `Score` whether the match is
//! still being scored or long finished, live or confirmed (see `Score`'s
//! doc comment on `main.rs`).
//!
//! Corrections are restricted to undoing the most recently recorded event —
//! `DELETE /matches/:id/live/events/:seq` only succeeds when `seq` is the
//! current tip. An arbitrary-position edit would need real conflict
//! resolution to reconcile against a device's own offline queue; undoing
//! only the tip doesn't (see the DELETE handler's doc comment).

use poem_openapi::{Object, Union};

pub mod cricket;
pub mod football;
pub mod netball;

pub use cricket::CricketLiveEvent;
pub use football::FootballLiveEvent;
pub use netball::NetballLiveEvent;

/// A single live-scoring event, sport-first discriminated so a new sport is a
/// new variant without touching existing ones — same pattern as `Score`.
/// See `football`/`cricket` for the per-kind union nested inside each
/// variant.
#[derive(Union)]
#[oai(one_of, discriminator_name = "sport")]
pub enum LiveEventInput {
    Football(FootballLiveEvent),
    Cricket(CricketLiveEvent),
    Netball(NetballLiveEvent),
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

/// A `Score` plus the log position it reflects, so a client can tell whether
/// its own queued-but-unsynced events are already applied. Returned by every
/// endpoint that touches the live event log (append, delete) —
/// `GET /matches/:id/score` itself doesn't need this wrapper, since a plain
/// read has no "position I was expecting" to compare against.
#[derive(Object)]
pub struct LiveScoreSnapshot {
    pub last_seq: u32,
    pub score: crate::Score,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detailed_score::football::{FootballCardColor, FootballCardEvent};
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
        // set), matching the existing `Score` union.
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

    /// `NetballFoulEvent::foul_kind` must survive serialization distinctly
    /// from `NetballLiveEvent`'s own `kind` discriminator (`Goal`/`Foul`/
    /// `Period`) once the two are flattened onto the same JSON object —
    /// regression test for exactly this collision, which previously silently
    /// dropped the foul's own kind (a naming clash, `NetballFoulEvent` field
    /// also called `kind`, that `NetballGoalEvent`/`NetballPeriodEvent` don't
    /// have — see `NetballFoulEvent::foul_kind`'s doc comment).
    #[test]
    fn netball_foul_kind_does_not_collide_with_the_outer_discriminator() {
        use crate::detailed_score::netball::{NetballFoulEvent, NetballFoulKind};
        use crate::live_score::netball::NetballLiveEvent;

        let event = LiveEventInput::Netball(NetballLiveEvent::Foul(NetballFoulEvent {
            side_id: "side_a".into(),
            player_id: Some("p1".into()),
            foul_kind: NetballFoulKind::Contact,
            minute: Some(10),
            occurred_at: None,
        }));

        let json = event.to_json().expect("serializes");
        let obj = json.as_object().expect("flat object, not a wrapper");
        assert_eq!(obj.get("sport").and_then(|v| v.as_str()), Some("Netball"));
        // The outer union's own discriminator.
        assert_eq!(obj.get("kind").and_then(|v| v.as_str()), Some("Foul"));
        // The foul's own kind, under its own (non-colliding) key.
        assert_eq!(
            obj.get("foul_kind").and_then(|v| v.as_str()),
            Some("contact")
        );

        let parsed = match LiveEventInput::parse_from_json(Some(json)) {
            Ok(v) => v,
            Err(_) => panic!("failed to parse back"),
        };
        match parsed {
            LiveEventInput::Netball(NetballLiveEvent::Foul(fo)) => {
                assert!(matches!(fo.foul_kind, NetballFoulKind::Contact));
                assert_eq!(fo.player_id.as_deref(), Some("p1"));
            }
            _ => panic!("wrong variant"),
        }
    }
}
