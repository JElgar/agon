//! The SQS consumer loop.
//!
//! Long-polls the events queue, parses each message into a [`ChangeEvent`], runs
//! the inline handlers, and **deletes the message only after the work succeeds**
//! — this is the ACK discipline that gives at-least-once processing (see
//! docs/async-design.md §3). On a transient failure the message is left on the
//! queue and redelivered after its visibility timeout; on a permanent failure
//! (unparseable body / bad keys) the message is deleted so it can't loop, with
//! the DLQ redrive policy as the ultimate backstop.
//!
//! Messages within a single receive batch are processed concurrently and each
//! is independently ACKed, so one poison message never blocks its neighbours.

use std::sync::Arc;

use agon_core::dao::Dao;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sqs::types::Message;
use chrono::Utc;
use futures::future::join_all;

use crate::config::Config;
use crate::error::{WorkerError, WorkerResult};
use crate::event::{ChangeEvent, Envelope};
use crate::handlers;
use agon_core::search::SearchClient;

/// Shared, cheaply-cloneable dependencies for message processing.
#[derive(Clone)]
pub struct Consumer {
    sqs: SqsClient,
    dao: Dao,
    search: SearchClient,
    config: Arc<Config>,
    /// Client for starting multi-step workflows. Only present with the
    /// `temporal` feature; when absent, multi-step work is not started here.
    #[cfg(feature = "temporal")]
    temporal: Option<crate::temporal::client::TemporalClient>,
}

impl Consumer {
    pub fn new(sqs: SqsClient, dao: Dao, search: SearchClient, config: Config) -> Self {
        Self {
            sqs,
            dao,
            search,
            config: Arc::new(config),
            #[cfg(feature = "temporal")]
            temporal: None,
        }
    }

    /// Attach a Temporal client so multi-step events start workflows.
    #[cfg(feature = "temporal")]
    pub fn with_temporal(mut self, client: crate::temporal::client::TemporalClient) -> Self {
        self.temporal = Some(client);
        self
    }

    /// Run the poll loop until `shutdown` resolves (e.g. SIGTERM). Each iteration
    /// long-polls for a batch, processes it concurrently, and ACKs successes.
    pub async fn run(&self, mut shutdown: impl std::future::Future<Output = ()> + Unpin) {
        tracing::info!(queue = %self.config.queue_url, "worker consumer started");
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received; stopping consumer");
                    return;
                }
                batch = self.poll_once() => {
                    if let Err(e) = batch {
                        // A receive failure is transient; back off briefly and retry.
                        tracing::error!(error = %e, "poll failed; backing off");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }

    /// One receive → process → delete cycle. Returns `Ok` even if individual
    /// messages fail (those are simply not deleted); only a receive-level error
    /// bubbles up.
    async fn poll_once(&self) -> WorkerResult<()> {
        let out = self
            .sqs
            .receive_message()
            .queue_url(&self.config.queue_url)
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
        tracing::debug!(count = messages.len(), "received batch");

        // Process the batch concurrently; each message ACKs itself on success.
        let futures = messages.iter().map(|m| self.process_and_ack(m));
        join_all(futures).await;
        Ok(())
    }

    /// Process one message and, on success (or permanent failure), delete it.
    /// Transient failures leave the message on the queue for redelivery.
    async fn process_and_ack(&self, msg: &Message) {
        let receipt = match msg.receipt_handle() {
            Some(r) => r,
            None => {
                tracing::warn!("message has no receipt handle; skipping");
                return;
            }
        };

        match self.process(msg).await {
            Ok(()) => self.delete(receipt).await,
            Err(e) if e.is_permanent() => {
                // Bad data will never succeed — drop it (DLQ is the backstop for
                // anything we misclassify).
                tracing::error!(error = %e, body = ?msg.body(), "permanent failure; dropping message");
                self.delete(receipt).await;
            }
            Err(e) => {
                // Transient — leave it for redelivery after the visibility timeout.
                tracing::warn!(error = %e, "transient failure; will redeliver");
            }
        }
    }

    /// Parse and handle one message. Errors classify as permanent (bad data) or
    /// transient (retryable) via [`WorkerError::is_permanent`].
    async fn process(&self, msg: &Message) -> WorkerResult<()> {
        let body = msg
            .body()
            .ok_or_else(|| WorkerError::Malformed("message has no body".into()))?;

        let envelope: Envelope = serde_json::from_str(body)
            .map_err(|e| WorkerError::Malformed(format!("bad envelope: {e}")))?;
        let event = ChangeEvent::from_envelope(&envelope)?;

        let now = Utc::now().to_rfc3339();
        handlers::route(&self.dao, &self.search, &event, &now).await?;

        // Multi-step work: start the relevant Temporal workflow (idempotent via
        // deterministic ids). Only compiled/attempted with the `temporal`
        // feature; a start failure is transient so the message redelivers.
        #[cfg(feature = "temporal")]
        self.maybe_start_workflow(&event).await?;

        Ok(())
    }

    /// Start a Temporal workflow for a multi-step event, if a client is attached:
    /// a match meta write → fan-out; an invitation that just transitioned to
    /// accepted → the accept saga.
    #[cfg(feature = "temporal")]
    async fn maybe_start_workflow(&self, event: &ChangeEvent) -> WorkerResult<()> {
        use agon_core::dao::keys::{Pk, Sk};

        let Some(temporal) = &self.temporal else {
            return Ok(());
        };

        match (&event.pk, &event.sk) {
            // A match was created or updated → (re)fan it into feeds.
            (Pk::Match(match_id), Sk::Meta) if !event.kind.is_remove() => {
                temporal
                    .start_fanout(match_id)
                    .await
                    .map_err(|e| WorkerError::Sqs(format!("start fanout: {e}")))?;
            }
            // An invitation whose status just became "accepted" → run the accept
            // saga. Both images deserialize straight into InvitationRecord, so we
            // detect the transition and read the full accepted record with no
            // extra DynamoDB read. Firing on the *transition* means we start once,
            // not on every subsequent modify of an already-accepted invitation.
            (Pk::Invitation(_), Sk::Meta) if !event.kind.is_remove() => {
                self.maybe_start_accept_saga(temporal, event).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Start the accept saga if this invitation event is a pending → accepted
    /// transition. Uses the old/new images directly (no re-read): the new image
    /// is the full accepted `InvitationRecord`.
    #[cfg(feature = "temporal")]
    async fn maybe_start_accept_saga(
        &self,
        temporal: &crate::temporal::client::TemporalClient,
        event: &ChangeEvent,
    ) -> WorkerResult<()> {
        use agon_core::dao::records::{InvitationContextRecord, InvitationRecord};

        let Some(new_inv) = event.new_record::<InvitationRecord>() else {
            return Ok(());
        };
        // Only act on the transition into "accepted", not every modify of an
        // already-accepted invitation.
        let was_accepted = event
            .old_record::<InvitationRecord>()
            .map(|old| old.status == "accepted")
            .unwrap_or(false);
        if new_inv.status != "accepted" || was_accepted {
            return Ok(());
        }
        // The accepter is the invited user. Skip if unidentified (unresolved token).
        let Some(accepting_user_id) = new_inv.invited_user_id.clone() else {
            return Ok(());
        };
        // Only match invites drive a re-fan-out; team invites have no feed impact.
        let match_id = match &new_inv.context {
            InvitationContextRecord::Match { match_id, .. } => Some(match_id.clone()),
            InvitationContextRecord::Team { .. } => None,
        };

        let input = crate::temporal::workflows::AcceptInvitationInput {
            invitation_id: new_inv.id.clone(),
            accepting_user_id,
            responded_at: new_inv.responded_at.clone().unwrap_or_default(),
            match_id,
        };
        temporal
            .start_accept(input)
            .await
            .map_err(|e| WorkerError::Sqs(format!("start accept saga: {e}")))?;
        Ok(())
    }

    async fn delete(&self, receipt_handle: &str) {
        if let Err(e) = self
            .sqs
            .delete_message()
            .queue_url(&self.config.queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
        {
            // A failed delete means the message redelivers; handlers are
            // idempotent, so this is safe — just log it.
            tracing::error!(error = %e, "failed to delete message; it will redeliver");
        }
    }
}
