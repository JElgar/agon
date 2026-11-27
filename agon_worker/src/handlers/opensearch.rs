//! Inline handler: shadow-write match and user changes to OpenSearch.
//!
//! On any write to a match's `#META` (score, sides, status, date, ...) or to its
//! roster (`PLAYER#` items, which don't touch `#META`), rebuild that match's
//! documents from DynamoDB with [`agon_services_core::opensearch::sync_match`].
//! On any write to a user's `#PROFILE`, rebuild their `users` document with
//! [`agon_services_core::opensearch::sync_user`].
//!
//! **Resilience.** A failed sync never fails this message: that would replay
//! every other inline handler for this event (indexing, notifications, push,
//! stats, tournament, rating), and during an OpenSearch outage it would push
//! stream events into the main DLQ for a problem that has nothing to do with
//! them. Instead the target is handed to [`crate::opensearch_repair`]'s
//! [`RepairProducer`], which sends it to its own SQS queue
//! (`agon-opensearch-repair`). That queue's native redrive policy — retry a
//! few times, then its own DLQ — is the retry; see docs/opensearch.md. The
//! backfill (`agon_worker backfill-opensearch`) is the last resort if that
//! queue's retries are exhausted or a repair producer isn't configured
//! (local dev without `AGON_OPENSEARCH_REPAIR_QUEUE_URL`).
//!
//! **Noise filter.** Every like and comment rewrites a match's `#META` (it
//! bumps a counter). Those changes don't affect any OpenSearch document, so a
//! `#META` modify whose images differ only in those counters is skipped
//! without a read.

use agon_services_core::dao::Dao;
use agon_services_core::dao::keys::{Pk, Sk};
use agon_services_core::opensearch::{
    OpenSearchClient, SyncOutcome, UserSyncOutcome, sync_match, sync_user,
};

use crate::error::WorkerResult;
use crate::event::{ChangeEvent, ChangeKind, Image};
use crate::opensearch_repair::{OpenSearchSyncTarget, RepairProducer};

/// `#META` attributes whose changes never affect an OpenSearch document.
const NOISE_ATTRS: &[&str] = &["like_count", "comment_count", "live_seq", "live_tip_seq"];

/// What the handler writes through: the OpenSearch client (`None` turns the
/// handler off) and the repair producer a failed sync is handed to. Bundled
/// together to keep `route` under clippy's argument limit.
#[derive(Clone, Copy, Default)]
pub struct Sinks<'a> {
    pub opensearch: Option<&'a OpenSearchClient>,
    pub repair: Option<&'a RepairProducer>,
}

pub async fn handle(dao: &Dao, sinks: Sinks<'_>, ev: &ChangeEvent, now: &str) -> WorkerResult<()> {
    let (Some(os), repair) = (sinks.opensearch, sinks.repair) else {
        return Ok(());
    };
    match &ev.pk {
        Pk::Match(match_id) => handle_match(os, repair, dao, match_id, ev, now).await,
        Pk::User(user_id) => handle_user(os, repair, dao, user_id, ev, now).await,
        _ => Ok(()),
    }
}

async fn handle_match(
    os: &OpenSearchClient,
    repair: Option<&RepairProducer>,
    dao: &Dao,
    match_id: &str,
    ev: &ChangeEvent,
    now: &str,
) -> WorkerResult<()> {
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
            schedule_repair(
                repair,
                OpenSearchSyncTarget::Match(match_id.to_string()),
                &e,
            )
            .await;
        }
    }
    Ok(())
}

async fn handle_user(
    os: &OpenSearchClient,
    repair: Option<&RepairProducer>,
    dao: &Dao,
    user_id: &str,
    ev: &ChangeEvent,
    now: &str,
) -> WorkerResult<()> {
    if ev.sk != Sk::Profile {
        return Ok(());
    }

    match sync_user(os, dao, user_id, now, true).await {
        Ok(UserSyncOutcome::Indexed) => {
            tracing::debug!(user_id = %user_id, "opensearch: user synced");
        }
        Ok(UserSyncOutcome::Removed) => {
            tracing::debug!(user_id = %user_id, "opensearch: user removed");
        }
        Err(e) => {
            schedule_repair(repair, OpenSearchSyncTarget::User(user_id.to_string()), &e).await;
        }
    }
    Ok(())
}

/// Hand a failed sync to the repair queue. Never returns an error: see the
/// module doc's **Resilience** section.
async fn schedule_repair(
    repair: Option<&RepairProducer>,
    target: OpenSearchSyncTarget,
    error: &dyn std::fmt::Display,
) {
    let Some(repair) = repair else {
        tracing::warn!(
            ?target,
            error = %error,
            "opensearch: sync failed; no repair queue configured (run the backfill to repair)"
        );
        return;
    };
    tracing::warn!(?target, error = %error, "opensearch: sync failed");
    repair.schedule(&target).await;
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
