//! The S3 asset-upload consumer loop.
//!
//! Long-polls the asset-events queue (fed by S3 → EventBridge → SQS on
//! `Object Created`), and flips the corresponding `Asset` from `pending` to
//! `uploaded`, recording its canonical CloudFront serving URL.
//!
//! This is the transition the API deliberately does NOT perform itself: the API
//! only issues the presigned PUT, and the object actually landing in the bucket
//! is what marks the asset uploaded — so the asset status reflects reality, not
//! the client's word. Match-header URLs are stored unsigned (canonical); the API
//! signs them at read time.
//!
//! ACK discipline mirrors the main consumer: delete only on success, so a
//! transient DynamoDB failure redelivers after the visibility timeout. The
//! handler is idempotent (marking an already-uploaded asset uploaded is a no-op
//! write), so redelivery is safe.

use std::sync::Arc;

use agon_core::dao::Dao;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sqs::types::Message;
use futures::future::join_all;
use serde::Deserialize;

use crate::config::Config;
use crate::error::{WorkerError, WorkerResult};

/// The slice of the S3 EventBridge "Object Created" event we need: the object
/// key. EventBridge wraps the S3 detail under `detail`; we ignore the rest.
///
/// ```json
/// { "detail": { "bucket": { "name": "agon-assets-staging" },
///               "object": { "key": "profile_image/abc123" } } }
/// ```
#[derive(Debug, Deserialize)]
struct S3Event {
    detail: S3Detail,
}

#[derive(Debug, Deserialize)]
struct S3Detail {
    object: S3Object,
}

#[derive(Debug, Deserialize)]
struct S3Object {
    key: String,
}

/// Shared, cheaply-cloneable deps for asset-event processing.
#[derive(Clone)]
pub struct AssetConsumer {
    sqs: SqsClient,
    dao: Dao,
    config: Arc<Config>,
}

impl AssetConsumer {
    pub fn new(sqs: SqsClient, dao: Dao, config: Arc<Config>) -> Self {
        Self { sqs, dao, config }
    }

    /// Run the poll loop until `shutdown` resolves.
    pub async fn run(&self, mut shutdown: impl std::future::Future<Output = ()> + Unpin) {
        tracing::info!(
            queue = %self.config.asset_events_queue_url,
            "asset consumer started"
        );
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received; stopping asset consumer");
                    return;
                }
                batch = self.poll_once() => {
                    if let Err(e) = batch {
                        tracing::error!(error = %e, "asset poll failed; backing off");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }

    async fn poll_once(&self) -> WorkerResult<()> {
        let out = self
            .sqs
            .receive_message()
            .queue_url(&self.config.asset_events_queue_url)
            .max_number_of_messages(self.config.batch_size)
            .wait_time_seconds(self.config.wait_time_seconds)
            .visibility_timeout(self.config.visibility_timeout_seconds)
            .send()
            .await
            .map_err(|e| WorkerError::Sqs(e.to_string()))?;

        let messages = out.messages.unwrap_or_default();
        if messages.is_empty() {
            return Ok(());
        }
        tracing::debug!(count = messages.len(), "received asset batch");

        let futures = messages.iter().map(|m| self.process_and_ack(m));
        join_all(futures).await;
        Ok(())
    }

    async fn process_and_ack(&self, msg: &Message) {
        let receipt = match msg.receipt_handle() {
            Some(r) => r,
            None => {
                tracing::warn!("asset message has no receipt handle; skipping");
                return;
            }
        };
        match self.process(msg).await {
            Ok(()) => self.delete(receipt).await,
            Err(e) if e.is_permanent() => {
                tracing::error!(error = %e, body = ?msg.body(), "permanent asset failure; routing to DLQ via redelivery");
            }
            Err(e) => {
                tracing::warn!(error = %e, "transient asset failure; will redeliver");
            }
        }
    }

    async fn process(&self, msg: &Message) -> WorkerResult<()> {
        let body = msg
            .body()
            .ok_or_else(|| WorkerError::Malformed("asset message has no body".into()))?;

        let event: S3Event = serde_json::from_str(body)
            .map_err(|e| WorkerError::Malformed(format!("bad S3 event: {e}")))?;
        let storage_key = event.detail.object.key;

        // The asset id is the last path segment of the object key
        // (`{purpose}/{id}`); the key is the DAO's `storage_key`.
        let asset_id = asset_id_from_key(&storage_key).ok_or_else(|| {
            WorkerError::Malformed(format!("unexpected object key: {storage_key}"))
        })?;

        let url = format!("{}/{}", self.config.assets_cdn_url, storage_key);
        // Idempotent: marking an already-uploaded asset uploaded is a harmless
        // re-write. A missing asset (id not found) is a DaoError::NotFound →
        // transient, so it redelivers — the asset item may not be visible yet if
        // this event races the create write. It lands in the DLQ if it never
        // appears.
        self.dao.mark_asset_uploaded(asset_id, &url).await?;
        tracing::info!(asset = %asset_id, "asset marked uploaded");
        Ok(())
    }

    async fn delete(&self, receipt_handle: &str) {
        if let Err(e) = self
            .sqs
            .delete_message()
            .queue_url(&self.config.asset_events_queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
        {
            tracing::error!(error = %e, "failed to delete asset message; it will redeliver");
        }
    }
}

/// Derive the asset id from an S3 object key of the form `{purpose}/{id}`.
/// Returns `None` if the key has no `/` or an empty id segment.
fn asset_id_from_key(key: &str) -> Option<&str> {
    let (_purpose, id) = key.rsplit_once('/')?;
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_asset_id_from_key() {
        assert_eq!(asset_id_from_key("profile_image/abc123"), Some("abc123"));
        assert_eq!(asset_id_from_key("match_header/xy_-Z"), Some("xy_-Z"));
    }

    #[test]
    fn rejects_keys_without_id() {
        assert_eq!(asset_id_from_key("noslash"), None);
        assert_eq!(asset_id_from_key("profile_image/"), None);
    }

    #[test]
    fn parses_s3_eventbridge_event() {
        let body = r#"{
            "detail": {
                "bucket": { "name": "agon-assets-staging" },
                "object": { "key": "match_header/m1abc" }
            }
        }"#;
        let event: S3Event = serde_json::from_str(body).unwrap();
        assert_eq!(event.detail.object.key, "match_header/m1abc");
        assert_eq!(asset_id_from_key(&event.detail.object.key), Some("m1abc"));
    }
}
