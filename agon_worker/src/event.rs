//! The event envelope carried on the SQS queue, and its typed interpretation.
//!
//! The EventBridge Pipe transforms each raw DynamoDB Streams record into this
//! envelope:
//!
//! ```json
//! {
//!   "event": "MODIFY",
//!   "pk": "INVITATION#i1",
//!   "sk": "#META",
//!   "old_image": { "status": { "S": "pending" }, ... },
//!   "new_image": { "status": { "S": "accepted" }, ... }
//! }
//! ```
//!
//! The table stream is `NEW_AND_OLD_IMAGES`, so both images are available. They
//! arrive in DynamoDB's attribute-value wire shape (`{"S": "..."}` etc.) — which
//! is exactly what `serde_dynamo` (de)serializes — so an image deserializes
//! straight into the shared DAO record structs via [`ChangeEvent::old`] /
//! [`ChangeEvent::new`]. No hand-written attribute digging.
//!
//! Images are **optional**: an INSERT has no `old_image`, a REMOVE has no
//! `new_image` (EventBridge omits an absent path from the template output).
//!
//! The worker uses images opportunistically — e.g. deserialize the new image
//! into a record to detect a status transition and act on the full record with
//! no extra read — and otherwise ignores them and re-reads current state (so
//! indexing always reflects the latest committed state).

use agon_core::dao::keys::{KeyError, Pk, Sk};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A DynamoDB item image in attribute-value wire shape. `serde_dynamo::Item` is
/// `HashMap<String, serde_dynamo::AttributeValue>`, and its `AttributeValue`
/// (de)serializes to/from the `{"S": "..."}` DynamoDB JSON the stream emits.
pub type Image = serde_dynamo::Item;

/// The kind of change captured off the stream. Mirrors the DynamoDB Streams
/// `eventName` (`INSERT` / `MODIFY` / `REMOVE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChangeKind {
    Insert,
    Modify,
    Remove,
}

impl ChangeKind {
    /// A remove means the item is gone; anything else means it currently exists.
    pub fn is_remove(self) -> bool {
        matches!(self, ChangeKind::Remove)
    }
}

/// The raw envelope as delivered on SQS (one per message body). Images are
/// present only for the relevant change kinds (see module docs).
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope {
    pub event: ChangeKind,
    pub pk: String,
    pub sk: String,
    #[serde(default)]
    pub old_image: Option<Image>,
    #[serde(default)]
    pub new_image: Option<Image>,
}

/// A parsed change event: the raw envelope with its keys resolved into typed
/// `Pk`/`Sk` values, plus the (optional) old/new images.
///
/// The images (and the `old_record`/`new_record` accessors) are consumed by the
/// `temporal`-gated accept-saga path and by tests; `allow(dead_code)` keeps the
/// default (feature-off) bin build clean without gating the fields themselves.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ChangeEvent {
    pub kind: ChangeKind,
    pub pk: Pk,
    pub sk: Sk,
    pub old_image: Option<Image>,
    pub new_image: Option<Image>,
}

impl ChangeEvent {
    /// Parse an envelope's key strings into typed keys, carrying the images.
    pub fn from_envelope(env: &Envelope) -> Result<Self, KeyError> {
        Ok(Self {
            kind: env.event,
            pk: env.pk.parse()?,
            sk: env.sk.parse()?,
            old_image: env.old_image.clone(),
            new_image: env.new_image.clone(),
        })
    }

    /// Deserialize the old image into a DAO record `T`. `None` if there is no old
    /// image (INSERT) or it doesn't fit `T`.
    #[allow(dead_code)] // used by the `temporal` feature + tests
    pub fn old_record<T: DeserializeOwned>(&self) -> Option<T> {
        deserialize_image(self.old_image.as_ref())
    }

    /// Deserialize the new image into a DAO record `T`. `None` if there is no new
    /// image (REMOVE) or it doesn't fit `T`.
    #[allow(dead_code)] // used by the `temporal` feature + tests
    pub fn new_record<T: DeserializeOwned>(&self) -> Option<T> {
        deserialize_image(self.new_image.as_ref())
    }
}

#[allow(dead_code)] // used by the `temporal` feature + tests
fn deserialize_image<T: DeserializeOwned>(image: Option<&Image>) -> Option<T> {
    image
        .cloned()
        .and_then(|item| serde_dynamo::from_item(item).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agon_core::dao::records::{
        InvitationContextRecord, InvitationKindRecord, InvitationRecord,
    };

    fn sample_invitation(status: &str) -> InvitationRecord {
        InvitationRecord {
            id: "inv1".into(),
            status: status.into(),
            invited_by_user_id: "u_host".into(),
            invited_user_id: Some("u_guest".into()),
            invite_token: None,
            kind: InvitationKindRecord::User {
                invited_user_id: "u_guest".into(),
            },
            context: InvitationContextRecord::Match {
                match_id: "m1".into(),
                match_name: "Tennis".into(),
            },
            invited_at: "2026-07-01T10:00:00Z".into(),
            responded_at: Some("2026-07-02T09:00:00Z".into()),
        }
    }

    /// An image (attribute-value shape) round-trips back into its record.
    #[test]
    fn new_image_deserializes_into_record() {
        let record = sample_invitation("accepted");
        let image: Image = serde_dynamo::to_item(&record).unwrap();
        let envelope = Envelope {
            event: ChangeKind::Modify,
            pk: "INVITATION#inv1".into(),
            sk: "#META".into(),
            old_image: None,
            new_image: Some(image),
        };
        let event = ChangeEvent::from_envelope(&envelope).unwrap();

        let parsed: InvitationRecord = event.new_record().expect("new image parses");
        assert_eq!(parsed, record);
        // No old image → old() is None.
        assert!(event.old_record::<InvitationRecord>().is_none());
    }

    /// The pending → accepted transition is detectable from the two images.
    #[test]
    fn detects_accept_transition() {
        let old: Image = serde_dynamo::to_item(sample_invitation("pending")).unwrap();
        let new: Image = serde_dynamo::to_item(sample_invitation("accepted")).unwrap();
        let event = ChangeEvent::from_envelope(&Envelope {
            event: ChangeKind::Modify,
            pk: "INVITATION#inv1".into(),
            sk: "#META".into(),
            old_image: Some(old),
            new_image: Some(new),
        })
        .unwrap();

        let old_accepted = event
            .old_record::<InvitationRecord>()
            .map(|r| r.status == "accepted")
            .unwrap_or(false);
        let new_accepted = event
            .new_record::<InvitationRecord>()
            .map(|r| r.status == "accepted")
            .unwrap_or(false);
        assert!(!old_accepted, "was pending before");
        assert!(new_accepted, "is accepted now");
    }

    /// Parses the literal DynamoDB Streams attribute-value JSON shape the Pipe
    /// emits (nested `{"S": ...}` / `{"M": ...}`), not just a serde round-trip —
    /// guards against the wire shape differing from what serde_dynamo expects.
    #[test]
    fn parses_literal_stream_image_shape() {
        let body = r##"{
            "event": "MODIFY",
            "pk": "INVITATION#inv1",
            "sk": "#META",
            "new_image": {
                "id": {"S": "inv1"},
                "status": {"S": "accepted"},
                "invited_by_user_id": {"S": "u_host"},
                "invited_user_id": {"S": "u_guest"},
                "kind": {"M": {
                    "type": {"S": "user"},
                    "invited_user_id": {"S": "u_guest"}
                }},
                "context": {"M": {
                    "type": {"S": "match"},
                    "match_id": {"S": "m1"},
                    "match_name": {"S": "Tennis"}
                }},
                "invited_at": {"S": "2026-07-01T10:00:00Z"},
                "responded_at": {"S": "2026-07-02T09:00:00Z"}
            }
        }"##;
        let envelope: Envelope = serde_json::from_str(body).expect("envelope parses");
        let event = ChangeEvent::from_envelope(&envelope).unwrap();
        let parsed: InvitationRecord = event.new_record().expect("stream image parses into record");
        assert_eq!(parsed.status, "accepted");
        assert_eq!(parsed.invited_user_id.as_deref(), Some("u_guest"));
        assert!(matches!(
            parsed.context,
            InvitationContextRecord::Match { .. }
        ));
    }
}
