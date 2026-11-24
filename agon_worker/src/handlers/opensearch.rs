//! Inline handler: shadow-write match and user changes to OpenSearch.
//!
//! On any write to a match's `#META` (score, sides, status, date, ...) or to its
//! roster (`PLAYER#` items, which don't touch `#META`), rebuild that match's
//! documents from DynamoDB with [`agon_services_core::opensearch::sync_match`].
//! On any write to a user's `#PROFILE`, rebuild their `users` document with
//! [`agon_services_core::opensearch::sync_user`].
//!
//! **Resilience.** A failed sync doesn't fail the message: that would replay
//! every other handler for this event and, during an OpenSearch outage, push
//! stream events into the DLQ. Instead the sync is handed to a Temporal
//! `RepairOpenSearch` workflow, which retries with backoff (for up to three
//! days) until it lands, so team stats, leaderboards and admin search heal by
//! themselves. Only if Temporal itself can't be reached does the message fail
//! and redeliver. Without a Temporal client (local runs) the failure is just
//! logged, and the backfill (`agon_worker backfill-opensearch`) repairs it.
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

use crate::error::{WorkerError, WorkerResult};
use crate::event::{ChangeEvent, ChangeKind, Image};
use crate::temporal::client::TemporalClient;
use crate::temporal::workflows::OpenSearchSyncTarget;

/// `#META` attributes whose changes never affect an OpenSearch document.
const NOISE_ATTRS: &[&str] = &["like_count", "comment_count", "live_seq", "live_tip_seq"];

/// What the handler writes through: the OpenSearch client (`None` turns the
/// handler off) and the Temporal client that failed syncs are handed to.
#[derive(Clone, Copy, Default)]
pub struct Sinks<'a> {
    pub opensearch: Option<&'a OpenSearchClient>,
    pub temporal: Option<&'a TemporalClient>,
}

pub async fn handle(dao: &Dao, sinks: Sinks<'_>, ev: &ChangeEvent, now: &str) -> WorkerResult<()> {
    let (Some(os), temporal) = (sinks.opensearch, sinks.temporal) else {
        return Ok(());
    };
    match &ev.pk {
        Pk::Match(match_id) => handle_match(os, temporal, dao, match_id, ev, now).await,
        Pk::User(user_id) => handle_user(os, temporal, dao, user_id, ev, now).await,
        _ => Ok(()),
    }
}

/// Hand a failed sync to a `RepairOpenSearch` workflow. Fails the message
/// (so SQS redelivers it) only if the workflow can't be started.
async fn schedule_repair(
    temporal: Option<&TemporalClient>,
    target: OpenSearchSyncTarget,
    error: &dyn std::fmt::Display,
) -> WorkerResult<()> {
    let Some(temporal) = temporal else {
        tracing::warn!(
            ?target,
            error = %error,
            "opensearch: sync failed; no Temporal client to retry it (run the backfill to repair)"
        );
        return Ok(());
    };
    tracing::warn!(?target, error = %error, "opensearch: sync failed; scheduling a repair");
    temporal
        .start_opensearch_repair(target)
        .await
        .map_err(|e| WorkerError::Sqs(format!("start opensearch repair: {e}")))
}

async fn handle_match(
    os: &OpenSearchClient,
    temporal: Option<&TemporalClient>,
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
            return schedule_repair(
                temporal,
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
    temporal: Option<&TemporalClient>,
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
            return schedule_repair(
                temporal,
                OpenSearchSyncTarget::User(user_id.to_string()),
                &e,
            )
            .await;
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
