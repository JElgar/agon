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
    // The EventBridge Pipe input template substitutes an **empty string** (not an
    // omitted key) for an image path that doesn't exist on the record — e.g.
    // `old_image` on an INSERT arrives as `"old_image": ""`. `deserialize_image`
    // maps that empty string (and null / absent) to `None`, and a real map to
    // `Some`. Without this, `""` fails to parse as a map and the whole message is
    // (wrongly) dropped as malformed.
    #[serde(default, deserialize_with = "deserialize_optional_image")]
    pub old_image: Option<Image>,
    #[serde(default, deserialize_with = "deserialize_optional_image")]
    pub new_image: Option<Image>,
}

/// Deserialize an optional DynamoDB image that may arrive as a map, `null`, or
/// an empty string (the EventBridge Pipe's rendering of an absent path).
fn deserialize_optional_image<'de, D>(deserializer: D) -> Result<Option<Image>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        // Absent path rendered as "" (or any string) → no image.
        serde_json::Value::String(_) | serde_json::Value::Null => Ok(None),
        // A real attribute-value map → the image.
        serde_json::Value::Object(_) => {
            let image: Image = serde_json::from_value(value).map_err(D::Error::custom)?;
            Ok(Some(image))
        }
        other => Err(D::Error::custom(format!(
            "unexpected image value: {other:?}"
        ))),
    }
}

/// A parsed change event: the raw envelope with its keys resolved into typed
/// `Pk`/`Sk` values, plus the (optional) old/new images.
///
/// The images (and the `old_record`/`new_record` accessors) are consumed by the
/// accept-saga routing path (`consumer::workflow_for`) and by tests.
#[derive(Debug, Clone)]
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
    pub fn old_record<T: DeserializeOwned>(&self) -> Option<T> {
        deserialize_image(self.old_image.as_ref())
    }

    /// Deserialize the new image into a DAO record `T`. `None` if there is no new
    /// image (REMOVE) or it doesn't fit `T`.
    pub fn new_record<T: DeserializeOwned>(&self) -> Option<T> {
        deserialize_image(self.new_image.as_ref())
    }
}

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

    /// The EventBridge Pipe renders an absent image path as an empty string, so
    /// an INSERT arrives as `"old_image": ""`. That must parse as `None`, not
    /// blow up as "expected a map" (which previously dropped every message).
    /// This is the verbatim shape observed from staging.
    #[test]
    fn empty_string_old_image_parses_as_none() {
        let body = r##"{
            "event": "INSERT",
            "pk": "USER#u1",
            "sk": "#PROFILE",
            "old_image": "",
            "new_image": {
                "id": {"S": "u1"},
                "name": {"S": "Test User"},
                "email": {"S": "u1@example.com"},
                "created_at": {"S": "2026-07-05T13:23:51.658Z"},
                "follower_count": {"N": "0"},
                "following_count": {"N": "0"},
                "unread_count": {"N": "0"}
            }
        }"##;
        let envelope: Envelope =
            serde_json::from_str(body).expect("envelope with empty old_image parses");
        assert!(
            envelope.old_image.is_none(),
            "empty-string old_image -> None"
        );
        assert!(envelope.new_image.is_some(), "new_image present");

        let event = ChangeEvent::from_envelope(&envelope).unwrap();
        assert_eq!(event.kind, ChangeKind::Insert);
        let user: agon_core::dao::records::UserRecord = event
            .new_record()
            .expect("new image parses into UserRecord");
        assert_eq!(user.id, "u1");
        assert_eq!(user.name, "Test User");
    }
}
