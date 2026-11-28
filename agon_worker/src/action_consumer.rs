//! Fan out one stream event into one SQS message per [`Action`], and
//! consume them.
//!
//! The main consumer (`consumer.rs`) no longer runs any handler inline for a
//! stream event: it hands the same envelope to [`ActionProducer`], which
//! sends it once per [`Action`] (including OpenSearch syncing — see
//! `opensearch_sync`) to the shared `agon-actions` queue. [`ActionConsumer`]
//! is the independent poll loop over that queue: it runs exactly one
//! handler per message. A failure in one handler only redelivers that
//! handler's own message — it can no longer replay its neighbours, which
//! running all of them inline in one SQS message used to mean.
//!
//! ACK discipline mirrors the other consumers: delete only on success, so a
//! transient failure redelivers after the queue's visibility timeout, and
//! the queue's own redrive policy (`agon_infra`'s `agon-actions` queue) is
//! the retry — the same settings the old single events queue used
//! (`maxReceiveCount: 5`, 60s visibility), now scoped to one handler's
//! failure instead of all six.
//!
//! `rating` can still fail *permanently* (`WorkerError::Invariant`): as
//! before, that message is left to redeliver into the DLQ rather than
//! deleted, so a bad classification doesn't silently destroy the message —
//! see `consumer.rs`'s module docs for the same reasoning.

use std::sync::Arc;

use agon_services_core::dao::Dao;
use agon_services_core::opensearch::OpenSearchClient;
use agon_services_core::push::PushClient;
use agon_services_core::search::SearchClient;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sqs::types::Message;
use chrono::Utc;
use futures::future::{join_all, try_join_all};

use crate::action::{Action, ActionMessage};
use crate::config::Config;
use crate::error::{WorkerError, WorkerResult};
use crate::event::{ChangeEvent, Envelope};
use crate::handlers::{self, index, notify, push, rating, stats, tournament};
use crate::opensearch_sync;

/// Sends one message per [`Action::ALL`] to the shared actions queue.
#[derive(Clone)]
pub struct ActionProducer {
    sqs: SqsClient,
    queue_url: String,
}

impl ActionProducer {
    pub fn new(sqs: SqsClient, queue_url: String) -> Self {
        Self { sqs, queue_url }
    }

    /// Fan `envelope` out into one message per action, concurrently. All-or-
    /// nothing: if any send fails, the caller leaves the original stream
    /// message undeleted so it redelivers — re-sending every action is
    /// harmless (SQS has no dedup here, but every handler is idempotent).
    pub async fn fan_out(&self, envelope: &Envelope) -> WorkerResult<()> {
        let sends = Action::ALL.iter().map(|&action| {
            let body = serde_json::to_string(&ActionMessage {
                action,
                envelope: envelope.clone(),
            })
            .expect("ActionMessage serializes: no non-UTF8 map keys, no NaN floats");
            async move {
                self.sqs
                    .send_message()
                    .queue_url(&self.queue_url)
                    .message_body(body)
                    .send()
                    .await
                    .map_err(|e| WorkerError::Sqs(e.to_string()))
            }
        });
        try_join_all(sends).await?;
        Ok(())
    }
}

/// Long-polls `agon-actions` and dispatches each message to exactly one
/// handler.
#[derive(Clone)]
pub struct ActionConsumer {
    sqs: SqsClient,
    dao: Dao,
    search: SearchClient,
    push: Option<PushClient>,
    /// `None` when OpenSearch syncing isn't configured (e.g. local dev) —
    /// then `Action::OpenSearchSync` messages are simply acked as no-ops.
    opensearch: Option<OpenSearchClient>,
    queue_url: String,
    config: Arc<Config>,
}

impl ActionConsumer {
    pub fn new(
        sqs: SqsClient,
        dao: Dao,
        search: SearchClient,
        push: Option<PushClient>,
        opensearch: Option<OpenSearchClient>,
        queue_url: String,
        config: Arc<Config>,
    ) -> Self {
        Self {
            sqs,
            dao,
            search,
            push,
            opensearch,
            queue_url,
            config,
        }
    }

    pub async fn run(&self, mut shutdown: impl std::future::Future<Output = ()> + Unpin) {
        tracing::info!(queue = %self.queue_url, "action consumer started");
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received; stopping action consumer");
                    return;
                }
                batch = self.poll_once() => {
                    if let Err(e) = batch {
                        tracing::error!(error = %e, "action poll failed; backing off");
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
        tracing::debug!(count = messages.len(), "received action batch");

        let futures = messages.iter().map(|m| self.process_and_ack(m));
        join_all(futures).await;
        Ok(())
    }

    async fn process_and_ack(&self, msg: &Message) {
        let receipt = match msg.receipt_handle() {
            Some(r) => r,
            None => {
                tracing::warn!("action message has no receipt handle; skipping");
                return;
            }
        };
        match self.process(msg).await {
            Ok(()) => self.delete(receipt).await,
            Err(e) if e.is_permanent() => {
                tracing::error!(error = %e, body = ?msg.body(), "permanent action failure; routing to DLQ via redelivery");
            }
            Err(e) => {
                tracing::warn!(error = %e, "transient action failure; will redeliver");
            }
        }
    }

    async fn process(&self, msg: &Message) -> WorkerResult<()> {
        let body = msg
            .body()
            .ok_or_else(|| WorkerError::Malformed("action message has no body".into()))?;
        let ActionMessage { action, envelope } = serde_json::from_str(body)
            .map_err(|e| WorkerError::Malformed(format!("bad action message: {e}")))?;
        let ev = ChangeEvent::from_envelope(&envelope)?;
        let now = Utc::now().to_rfc3339();

        match action {
            Action::Index => index::handle(&self.dao, &self.search, &ev).await,
            Action::Notify => notify::handle(&self.dao, &ev, &now).await,
            Action::Push => {
                push::handle(
                    &self.dao,
                    self.push.as_ref(),
                    self.config.ui_base_url.as_deref(),
                    &ev,
                )
                .await
            }
            Action::Stats => stats::handle(&self.dao, &ev).await,
            Action::Tournament => tournament::handle(&self.dao, &ev).await,
            Action::Rating => rating::handle(&self.dao, &ev, &now).await,
            Action::OpenSearchSync => {
                match (handlers::opensearch::target_for(&ev), &self.opensearch) {
                    (Some(target), Some(os)) => opensearch_sync::sync(&target, &self.dao, os).await,
                    _ => Ok(()),
                }
            }
        }
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
            tracing::error!(error = %e, "failed to delete action message; it will redeliver");
        }
    }
}
