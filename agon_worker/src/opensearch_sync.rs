//! Sync one match or user to OpenSearch, via a dedicated SQS queue — the
//! *only* place an OpenSearch write happens.
//!
//! The main consumer never syncs inline any more: for every event it calls
//! `handlers::opensearch::target_for`, and if that returns a target, sends
//! it here via [`SyncProducer`] instead of running `sync_match`/`sync_user`
//! itself. [`SyncConsumer`] is the independent poll loop that actually does
//! the read-from-Dynamo-write-to-OpenSearch work.
//!
//! This queue is kept separate from the shared `agon-actions` queue the other
//! six handlers fan out onto (see `action_consumer`) because OpenSearch needs
//! a much longer retry window: `agon_infra` gives `agon-opensearch-sync` a
//! 10-minute visibility timeout and `maxReceiveCount: 3` (≈30 minutes of
//! tolerance for a blip) instead of the 60s/5 the other actions use — mixing
//! the two into one queue would mean either every action waits 10 minutes to
//! redeliver, or OpenSearch gets no more than a 1-minute window. Each attempt
//! rebuilds the documents from DynamoDB's *current* state, so a late or
//! duplicate attempt never writes stale data. See docs/opensearch.md.

use std::sync::Arc;

use agon_services_core::dao::Dao;
use agon_services_core::opensearch::{OpenSearchClient, sync_match, sync_user};
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sqs::types::Message;
use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{WorkerError, WorkerResult};

/// What a sync re-syncs: one match's documents or one user's `users` doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenSearchSyncTarget {
    Match(String),
    User(String),
}

/// Sends a sync target to the queue. Cheap to clone (just an SQS client
/// handle and a queue URL).
#[derive(Clone)]
pub struct SyncProducer {
    sqs: SqsClient,
    queue_url: String,
}

impl SyncProducer {
    pub fn new(sqs: SqsClient, queue_url: String) -> Self {
        Self { sqs, queue_url }
    }

    /// Best-effort: a send failure is logged, not propagated. Failing the
    /// caller would fail the whole stream message and, since every other
    /// handler is now a separately-fanned-out action, buy nothing — the
    /// backfill (`agon_worker backfill-opensearch`) is the fallback if a
    /// send and every later sync attempt somehow both fail.
    pub async fn send(&self, target: &OpenSearchSyncTarget) {
        let body = match serde_json::to_string(target) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(?target, error = %e, "failed to serialize opensearch sync target");
                return;
            }
        };
        if let Err(e) = self
            .sqs
            .send_message()
            .queue_url(&self.queue_url)
            .message_body(body)
            .send()
            .await
        {
            tracing::error!(?target, error = %e, "failed to schedule opensearch sync; the backfill is the fallback");
        }
    }
}

/// Long-polls the sync queue and re-syncs each target. ACK discipline
/// mirrors the other consumers: delete only on success, so a transient
/// failure redelivers after the queue's visibility timeout, and SQS's own
/// redrive policy moves it to the DLQ after `maxReceiveCount` attempts (see
/// `agon_infra`'s `agon-opensearch-sync` queue).
#[derive(Clone)]
pub struct SyncConsumer {
    sqs: SqsClient,
    dao: Dao,
    opensearch: OpenSearchClient,
    queue_url: String,
    config: Arc<Config>,
}

impl SyncConsumer {
    pub fn new(
        sqs: SqsClient,
        dao: Dao,
        opensearch: OpenSearchClient,
        queue_url: String,
        config: Arc<Config>,
    ) -> Self {
        Self {
            sqs,
            dao,
            opensearch,
            queue_url,
            config,
        }
    }

    pub async fn run(&self, mut shutdown: impl std::future::Future<Output = ()> + Unpin) {
        tracing::info!(queue = %self.queue_url, "opensearch sync consumer started");
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received; stopping opensearch sync consumer");
                    return;
                }
                batch = self.poll_once() => {
                    if let Err(e) = batch {
                        tracing::error!(error = %e, "opensearch sync poll failed; backing off");
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
            .queue_url(&self.queue_url)
            .max_number_of_messages(self.config.batch_size)
            .wait_time_seconds(self.config.wait_time_seconds)
            .send()
            .await
            .map_err(|e| WorkerError::Sqs(e.to_string()))?;

        let messages = out.messages.unwrap_or_default();
        if messages.is_empty() {
            return Ok(());
        }
        tracing::debug!(count = messages.len(), "received opensearch sync batch");

        let futures = messages.iter().map(|m| self.process_and_ack(m));
        join_all(futures).await;
        Ok(())
    }

    async fn process_and_ack(&self, msg: &Message) {
        let receipt = match msg.receipt_handle() {
            Some(r) => r,
            None => {
                tracing::warn!("opensearch sync message has no receipt handle; skipping");
                return;
            }
        };
        match self.process(msg).await {
            Ok(()) => self.delete(receipt).await,
            Err(e) => {
                tracing::warn!(error = %e, "opensearch sync attempt failed; will retry");
            }
        }
    }

    async fn process(&self, msg: &Message) -> WorkerResult<()> {
        let body = msg
            .body()
            .ok_or_else(|| WorkerError::Malformed("opensearch sync message has no body".into()))?;
        let target: OpenSearchSyncTarget = serde_json::from_str(body)
            .map_err(|e| WorkerError::Malformed(format!("bad opensearch sync target: {e}")))?;

        let now = chrono::Utc::now().to_rfc3339();
        match &target {
            OpenSearchSyncTarget::Match(id) => {
                sync_match(&self.opensearch, &self.dao, id, &now, true).await?;
            }
            OpenSearchSyncTarget::User(id) => {
                sync_user(&self.opensearch, &self.dao, id, &now, true).await?;
            }
        }
        tracing::debug!(?target, "opensearch synced");
        Ok(())
    }

    async fn delete(&self, receipt_handle: &str) {
        if let Err(e) = self
            .sqs
            .delete_message()
            .queue_url(&self.queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
        {
            tracing::error!(error = %e, "failed to delete opensearch sync message; it will redeliver");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_round_trips_through_json() {
        let t = OpenSearchSyncTarget::User("u9".into());
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<OpenSearchSyncTarget>(&s).unwrap(), t);

        let t = OpenSearchSyncTarget::Match("m1".into());
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<OpenSearchSyncTarget>(&s).unwrap(), t);
    }
}
