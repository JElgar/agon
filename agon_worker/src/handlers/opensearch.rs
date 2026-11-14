//! Inline handler: shadow-write match changes to OpenSearch.
//!
//! On any write to a match's `#META` (score, sides, status, date, ...) or to its
//! roster (`PLAYER#` items, which don't touch `#META`), rebuild that match's
//! documents from DynamoDB with [`agon_core::opensearch::sync_match`].
//!
//! **Shadow mode.** Nothing reads OpenSearch on a request path yet, so a
//! failure here is logged and swallowed rather than failing the message: an
//! OpenSearch outage must not push stream events into the DLQ or delay the
//! handlers that matter today. The backfill (`agon_worker backfill-opensearch`)
//! repairs anything missed. Revisit this once reads move over.
//!
//! **Noise filter.** Every like and comment rewrites `#META` (it bumps a
//! counter). Those changes don't affect any OpenSearch document, so a `#META`
//! modify whose images differ only in those counters is skipped without a read.

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::opensearch::{OpenSearchClient, SyncOutcome, sync_match};

use crate::error::WorkerResult;
use crate::event::{ChangeEvent, ChangeKind, Image};

/// `#META` attributes whose changes never affect an OpenSearch document.
const NOISE_ATTRS: &[&str] = &["like_count", "comment_count", "live_seq", "live_tip_seq"];

pub async fn handle(
    dao: &Dao,
    os: Option<&OpenSearchClient>,
    ev: &ChangeEvent,
    now: &str,
) -> WorkerResult<()> {
    let Some(os) = os else {
        return Ok(());
    };
    let Pk::Match(match_id) = &ev.pk else {
        return Ok(());
    };
    match &ev.sk {
        Sk::Meta => {
            if ev.kind == ChangeKind::Modify && only_noise_changed(&ev.old_image, &ev.new_image) {
                return Ok(());
            }
        }
        Sk::Player(_) => {}
        _ => return Ok(()),
    }

    match sync_match(os, dao, match_id, now, true).await {
        Ok(SyncOutcome::Indexed { player_rows }) => {
            tracing::debug!(match_id = %match_id, player_rows, "opensearch: match synced");
        }
        Ok(SyncOutcome::Removed) => {
            tracing::debug!(match_id = %match_id, "opensearch: match removed");
        }
        Err(e) => {
            tracing::warn!(
                match_id = %match_id,
                error = %e,
                "opensearch: sync failed (shadow mode, not retried; the backfill repairs it)"
            );
        }
    }
    Ok(())
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

    fn image(json: serde_json::Value) -> Image {
        serde_json::from_value(json).expect("image")
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
}
