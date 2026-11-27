//! Retry a failed OpenSearch sync via a dedicated SQS queue, isolated from the
//! main events queue so a sync failure never replays the other inline
//! handlers (indexing, notifications, push, stats, tournament, rating).
//!
//! `handlers::opensearch` hands a failed target to [`RepairProducer`], which
//! sends it to the `agon-opensearch-repair` queue. [`RepairConsumer`] is a
//! second, independent poll loop over that queue: it re-syncs from
//! DynamoDB's *current* state, so a late or duplicate attempt never writes
//! stale data. Retry/backoff is SQS's own redrive policy on that queue (not
//! this process's concern) — see docs/opensearch.md.

use std::sync::Arc;

use agon_services_core::dao::Dao;
use agon_services_core::opensearch::{OpenSearchClient, sync_match, sync_user};
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sqs::types::Message;
use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{WorkerError, WorkerResult};

/// What a repair re-syncs: one match's documents or one user's `users` doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenSearchSyncTarget {
    Match(String),
    User(String),
}

/// Sends a failed sync's target to the repair queue. Cheap to clone (just an
/// SQS client handle and a queue URL).
#[derive(Clone)]
pub struct RepairProducer {
    sqs: SqsClient,
    queue_url: String,
}

impl RepairProducer {
    pub fn new(sqs: SqsClient, queue_url: String) -> Self {
        Self { sqs, queue_url }
    }

    /// Best-effort: a failure here is logged, not propagated. Failing the
    /// caller would fail the whole stream message and replay every other
    /// handler for this event, which is exactly what routing OpenSearch
    /// through its own queue is meant to avoid. The backfill
    /// (`agon_worker backfill-opensearch`) is the fallback if both this send
    /// and every repair attempt somehow fail.
    pub async fn schedule(&self, target: &OpenSearchSyncTarget) {
        let body = match serde_json::to_string(target) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(?target, error = %e, "failed to serialize opensearch repair target");
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
            tracing::error!(?target, error = %e, "failed to schedule opensearch repair; the backfill is the fallback");
        } else {
            tracing::warn!(?target, "opensearch: sync failed; scheduled a repair");
        }
    }
}

/// Long-polls the repair queue and re-syncs each target. ACK discipline
/// mirrors the other consumers: delete only on success, so a transient
/// failure redelivers after the queue's visibility timeout, and SQS's own
/// redrive policy moves it to the DLQ after `maxReceiveCount` attempts (see
/// `agon_infra`'s `agon-opensearch-repair` queue).
#[derive(Clone)]
pub struct RepairConsumer {
    sqs: SqsClient,
    dao: Dao,
    opensearch: OpenSearchClient,
    queue_url: String,
    config: Arc<Config>,
}

impl RepairConsumer {
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
        tracing::info!(queue = %self.queue_url, "opensearch repair consumer started");
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received; stopping opensearch repair consumer");
                    return;
                }
                batch = self.poll_once() => {
                    if let Err(e) = batch {
                        tracing::error!(error = %e, "opensearch repair poll failed; backing off");
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
        tracing::debug!(count = messages.len(), "received opensearch repair batch");

        let futures = messages.iter().map(|m| self.process_and_ack(m));
        join_all(futures).await;
        Ok(())
    }

    async fn process_and_ack(&self, msg: &Message) {
        let receipt = match msg.receipt_handle() {
            Some(r) => r,
            None => {
                tracing::warn!("opensearch repair message has no receipt handle; skipping");
                return;
            }
        };
        match self.process(msg).await {
            Ok(()) => self.delete(receipt).await,
            Err(e) => {
                tracing::warn!(error = %e, "opensearch repair attempt failed; will retry");
            }
        }
    }

    async fn process(&self, msg: &Message) -> WorkerResult<()> {
        let body = msg.body().ok_or_else(|| {
            WorkerError::Malformed("opensearch repair message has no body".into())
        })?;
        let target: OpenSearchSyncTarget = serde_json::from_str(body)
            .map_err(|e| WorkerError::Malformed(format!("bad opensearch repair target: {e}")))?;

        let now = chrono::Utc::now().to_rfc3339();
        match &target {
            OpenSearchSyncTarget::Match(id) => {
                sync_match(&self.opensearch, &self.dao, id, &now, true).await?;
            }
            OpenSearchSyncTarget::User(id) => {
                sync_user(&self.opensearch, &self.dao, id, &now, true).await?;
            }
        }
        tracing::info!(?target, "opensearch repair succeeded");
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
            tracing::error!(error = %e, "failed to delete opensearch repair message; it will redeliver");
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
