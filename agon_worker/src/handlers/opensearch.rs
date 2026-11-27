//! Pure decision step: does this stream event call for an OpenSearch sync,
//! and for what target?
//!
//! On any write to a match's `#META` (score, sides, status, date, ...) or to
//! its roster (`PLAYER#` items, which don't touch `#META`), the match needs
//! re-syncing. On any write to a user's `#PROFILE`, so does their `users`
//! document; likewise a team's `#META` and its `teams` document. No DynamoDB
//! or OpenSearch I/O happens here — [`target_for`] just decides, from the
//! event's keys and images, whether a sync is warranted and what it targets.
//!
//! The actual sync (reading current state from DynamoDB, writing to
//! OpenSearch) happens in `opensearch_sync::sync`, called from
//! `action_consumer::ActionConsumer`'s `Action::OpenSearchSync` case: for
//! every event, [`target_for`] decides whether that sync is warranted and
//! for what target. It shares the same `agon-actions` queue and retry
//! settings as every other action — see docs/opensearch.md.
//!
//! **Noise filter.** Every like and comment rewrites a match's `#META` (it
//! bumps a counter). Those changes don't affect any OpenSearch document, so a
//! `#META` modify whose images differ only in those counters is skipped
//! without a read.

use agon_services_core::dao::keys::{Pk, Sk};

use crate::event::{ChangeEvent, ChangeKind, Image};
use crate::opensearch_sync::OpenSearchSyncTarget;

/// `#META` attributes whose changes never affect an OpenSearch document.
const NOISE_ATTRS: &[&str] = &["like_count", "comment_count", "live_seq", "live_tip_seq"];

/// `None` if this event needs no OpenSearch sync at all.
pub fn target_for(ev: &ChangeEvent) -> Option<OpenSearchSyncTarget> {
    match &ev.pk {
        Pk::Match(match_id) => {
            match &ev.sk {
                Sk::Meta => {
                    if ev.kind == ChangeKind::Modify
                        && only_noise_changed(&ev.old_image, &ev.new_image)
                    {
                        return None;
                    }
                }
                Sk::Player(_) => {}
                _ => return None,
            }
            Some(OpenSearchSyncTarget::Match(match_id.clone()))
        }
        Pk::User(user_id) => {
            if ev.sk != Sk::Profile {
                return None;
            }
            Some(OpenSearchSyncTarget::User(user_id.clone()))
        }
        Pk::Team(team_id) => {
            if ev.sk != Sk::Meta {
                return None;
            }
            Some(OpenSearchSyncTarget::Team(team_id.clone()))
        }
        _ => None,
    }
}

/// True when both images exist and differ only in [`NOISE_ATTRS`].
fn only_noise_changed(old: &Option<Image>, new: &Option<Image>) -> bool {
    let (Some(old), Some(new)) = (old, new) else {
        return false;
    };
    let strip = |img: &Image| {
        let mut img = img.clone();
        for k in NOISE_ATTRS {
            img.remove(*k);
        }
        img
    };
    strip(old) == strip(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Envelope;

    fn image(json: serde_json::Value) -> Image {
        serde_json::from_value(json).expect("image")
    }

    fn event(
        pk: &str,
        sk: &str,
        kind: ChangeKind,
        old: Option<Image>,
        new: Option<Image>,
    ) -> ChangeEvent {
        ChangeEvent::from_envelope(&Envelope {
            event: kind,
            pk: pk.into(),
            sk: sk.into(),
            old_image: old,
            new_image: new,
        })
        .unwrap()
    }

    #[test]
    fn like_only_change_is_noise() {
        let old = image(
            serde_json::json!({ "status": { "S": "completed" }, "like_count": { "N": "1" } }),
        );
        let new = image(
            serde_json::json!({ "status": { "S": "completed" }, "like_count": { "N": "2" } }),
        );
        assert!(only_noise_changed(&Some(old), &Some(new)));
    }

    #[test]
    fn status_change_is_not_noise() {
        let old = image(
            serde_json::json!({ "status": { "S": "scheduled" }, "like_count": { "N": "1" } }),
        );
        let new = image(
            serde_json::json!({ "status": { "S": "completed" }, "like_count": { "N": "1" } }),
        );
        assert!(!only_noise_changed(&Some(old), &Some(new)));
    }

    #[test]
    fn missing_old_image_is_never_noise() {
        let new = image(serde_json::json!({ "status": { "S": "completed" } }));
        assert!(!only_noise_changed(&None, &Some(new)));
    }

    #[test]
    fn match_meta_write_targets_the_match() {
        let new = image(serde_json::json!({ "status": { "S": "completed" } }));
        let ev = event("MATCH#m1", "#META", ChangeKind::Modify, None, Some(new));
        assert_eq!(
            target_for(&ev),
            Some(OpenSearchSyncTarget::Match("m1".into()))
        );
    }

    #[test]
    fn match_like_only_meta_write_has_no_target() {
        let old = image(
            serde_json::json!({ "status": { "S": "completed" }, "like_count": { "N": "1" } }),
        );
        let new = image(
            serde_json::json!({ "status": { "S": "completed" }, "like_count": { "N": "2" } }),
        );
        let ev = event(
            "MATCH#m1",
            "#META",
            ChangeKind::Modify,
            Some(old),
            Some(new),
        );
        assert_eq!(target_for(&ev), None);
    }

    #[test]
    fn match_player_write_targets_the_match() {
        let ev = event("MATCH#m1", "PLAYER#u1", ChangeKind::Insert, None, None);
        assert_eq!(
            target_for(&ev),
            Some(OpenSearchSyncTarget::Match("m1".into()))
        );
    }

    #[test]
    fn user_profile_write_targets_the_user() {
        let ev = event("USER#u1", "#PROFILE", ChangeKind::Modify, None, None);
        assert_eq!(
            target_for(&ev),
            Some(OpenSearchSyncTarget::User("u1".into()))
        );
    }

    #[test]
    fn team_meta_write_targets_the_team() {
        let ev = event("TEAM#t1", "#META", ChangeKind::Modify, None, None);
        assert_eq!(
            target_for(&ev),
            Some(OpenSearchSyncTarget::Team("t1".into()))
        );
    }

    #[test]
    fn team_non_meta_write_has_no_target() {
        let ev = event("TEAM#t1", "MEMBER#u1", ChangeKind::Insert, None, None);
        assert_eq!(target_for(&ev), None);
    }

    #[test]
    fn unrelated_write_has_no_target() {
        let ev = event("INVITATION#i1", "#META", ChangeKind::Modify, None, None);
        assert_eq!(target_for(&ev), None);
    }
}
