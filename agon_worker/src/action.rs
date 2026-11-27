//! The action tag a fanned-out message carries.
//!
//! The main consumer no longer runs any handler itself: for every stream
//! event it fans the same envelope out into one message per [`Action`] on
//! the shared `agon-actions` queue (see `action_consumer`), so a failure in
//! one handler only redelivers that handler's own message — never its
//! neighbours'. OpenSearch sync is one of these actions like any other
//! (dispatched via `handlers::opensearch::target_for` — see
//! `opensearch_sync`), sharing the same queue and retry settings as the rest
//! rather than needing a queue of its own — see docs/opensearch.md.

use serde::{Deserialize, Serialize};

use crate::event::Envelope;

/// One of the inline handlers that used to all run, in sequence, inside a
/// single `handlers::route` call for one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Index,
    Notify,
    Push,
    Stats,
    Tournament,
    Rating,
    OpenSearchSync,
}

impl Action {
    /// Every action, fanned out for each stream event.
    pub const ALL: [Action; 7] = [
        Action::Index,
        Action::Notify,
        Action::Push,
        Action::Stats,
        Action::Tournament,
        Action::Rating,
        Action::OpenSearchSync,
    ];
}

/// The message body sent to `agon-actions`: the same envelope the main events
/// queue carries, tagged with which single handler should run it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionMessage {
    pub action: Action,
    pub envelope: Envelope,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ChangeKind;

    #[test]
    fn action_message_round_trips_through_json() {
        let msg = ActionMessage {
            action: Action::Notify,
            envelope: Envelope {
                event: ChangeKind::Modify,
                pk: "MATCH#m1".into(),
                sk: "#META".into(),
                old_image: None,
                new_image: None,
            },
        };
        let body = serde_json::to_string(&msg).unwrap();
        let parsed: ActionMessage = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.action, Action::Notify);
        assert_eq!(parsed.envelope.pk, "MATCH#m1");
    }
}
