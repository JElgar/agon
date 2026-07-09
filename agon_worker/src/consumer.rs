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
    /// Client for starting multi-step workflows. Attached in `main` after the
    /// Temporal connection succeeds; when absent (e.g. in unit tests), multi-step
    /// work is not started here.
    temporal: Option<crate::temporal::client::TemporalClient>,
}

impl Consumer {
    pub fn new(sqs: SqsClient, dao: Dao, search: SearchClient, config: Config) -> Self {
        Self {
            sqs,
            dao,
            search,
            config: Arc::new(config),
            temporal: None,
        }
    }

    /// Attach a Temporal client so multi-step events start workflows.
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

    /// Process one message; delete it (ACK) only on success. Any failure —
    /// permanent or transient — is left on the queue, so it redelivers and, after
    /// `maxReceiveCount` attempts, SQS moves it to the DLQ.
    ///
    /// We deliberately do NOT delete "permanent" (unparseable) messages: a parse
    /// bug that classifies every message as permanent would otherwise silently
    /// destroy every event (that exact incident happened — a Pipe rendering quirk
    /// made every envelope look malformed). Routing them to the DLQ keeps them
    /// visible and replayable instead of lost. The only cost is a permanently-bad
    /// message is retried a few times before landing in the DLQ, which is cheap.
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
                // Don't delete — let it redeliver into the DLQ for inspection.
                tracing::error!(error = %e, body = ?msg.body(), "permanent failure; routing to DLQ via redelivery");
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
        // deterministic ids). A start failure is transient, so the message is left
        // on the queue and redelivers.
        self.maybe_start_workflow(&event).await?;

        Ok(())
    }

    /// Start the Temporal workflow this event calls for, if a client is attached.
    /// The routing decision is factored out into [`workflow_for`] (pure, so it's
    /// unit-tested); this method just performs the resulting start.
    async fn maybe_start_workflow(&self, event: &ChangeEvent) -> WorkerResult<()> {
        let Some(temporal) = &self.temporal else {
            return Ok(());
        };

        match workflow_for(event) {
            Some(WorkflowStart::FanOut { match_id }) => temporal
                .start_fanout(&match_id)
                .await
                .map_err(|e| WorkerError::Sqs(format!("start fanout: {e}")))?,
            Some(WorkflowStart::Accept(input)) => temporal
                .start_accept(input)
                .await
                .map_err(|e| WorkerError::Sqs(format!("start accept saga: {e}")))?,
            None => {}
        }
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

/// Which Temporal workflow a stream event should start, if any. Kept separate
/// from [`Consumer`] so the routing decision is a pure function of the event —
/// no live Temporal client — and can be asserted directly in unit tests.
#[derive(Debug, Clone, PartialEq)]
enum WorkflowStart {
    /// (Re)fan a match into feeds.
    FanOut { match_id: String },
    /// Run the invitation-acceptance saga.
    Accept(crate::temporal::workflows::AcceptInvitationInput),
}

/// Decide which workflow (if any) a change event should start:
/// - a match meta write (not a remove) → fan-out;
/// - an invitation that just transitioned *into* "accepted" → the accept saga.
///
/// For the invitation case both images deserialize straight into
/// `InvitationRecord`, so we detect the transition and read the full accepted
/// record with no extra DynamoDB read. Firing on the *transition* means we start
/// once, not on every subsequent modify of an already-accepted invitation.
fn workflow_for(event: &ChangeEvent) -> Option<WorkflowStart> {
    use agon_core::dao::keys::{Pk, Sk};
    use agon_core::dao::records::{InvitationContextRecord, InvitationRecord};

    if event.kind.is_remove() {
        return None;
    }

    match (&event.pk, &event.sk) {
        // A match was created or updated → (re)fan it into feeds.
        (Pk::Match(match_id), Sk::Meta) => Some(WorkflowStart::FanOut {
            match_id: match_id.clone(),
        }),
        // An invitation meta write → start the accept saga iff this is a
        // pending → accepted transition.
        (Pk::Invitation(_), Sk::Meta) => {
            let new_inv = event.new_record::<InvitationRecord>()?;
            // Only act on the transition into "accepted", not every modify of an
            // already-accepted invitation.
            let was_accepted = event
                .old_record::<InvitationRecord>()
                .map(|old| old.status == "accepted")
                .unwrap_or(false);
            if new_inv.status != "accepted" || was_accepted {
                return None;
            }
            // The accepter is the invited user. Skip if unidentified (unresolved
            // token).
            let accepting_user_id = new_inv.invited_user_id.clone()?;
            // Only match invites drive a re-fan-out; team invites have no feed
            // impact.
            let match_id = match &new_inv.context {
                InvitationContextRecord::Match { match_id, .. } => Some(match_id.clone()),
                InvitationContextRecord::Team { .. } => None,
            };
            Some(WorkflowStart::Accept(
                crate::temporal::workflows::AcceptInvitationInput {
                    invitation_id: new_inv.id.clone(),
                    accepting_user_id,
                    responded_at: new_inv.responded_at.clone().unwrap_or_default(),
                    match_id,
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ChangeKind, Image};
    use agon_core::dao::records::{
        InvitationContextRecord, InvitationKindRecord, InvitationRecord,
    };

    /// Build a `ChangeEvent` from typed keys and optional old/new records, going
    /// through the same attribute-value (de)serialization the stream uses.
    fn event(
        kind: ChangeKind,
        pk: &str,
        sk: &str,
        old: Option<&InvitationRecord>,
        new: Option<&InvitationRecord>,
    ) -> ChangeEvent {
        let to_image = |r: &InvitationRecord| -> Image { serde_dynamo::to_item(r).unwrap() };
        ChangeEvent::from_envelope(&Envelope {
            event: kind,
            pk: pk.into(),
            sk: sk.into(),
            old_image: old.map(to_image),
            new_image: new.map(to_image),
        })
        .unwrap()
    }

    fn invitation(status: &str, context: InvitationContextRecord) -> InvitationRecord {
        InvitationRecord {
            id: "inv1".into(),
            status: status.into(),
            invited_by_user_id: "u_host".into(),
            invited_user_id: Some("u_guest".into()),
            invite_token: None,
            kind: InvitationKindRecord::User {
                invited_user_id: "u_guest".into(),
            },
            context,
            invited_at: "2026-07-01T10:00:00Z".into(),
            responded_at: Some("2026-07-02T09:00:00Z".into()),
        }
    }

    fn match_ctx() -> InvitationContextRecord {
        InvitationContextRecord::Match {
            match_id: "m1".into(),
            match_name: "Tennis".into(),
        }
    }

    fn team_ctx() -> InvitationContextRecord {
        InvitationContextRecord::Team {
            team_id: "t1".into(),
            team_name: "Aces".into(),
        }
    }

    #[test]
    fn match_meta_write_starts_fanout() {
        let ev = event(ChangeKind::Insert, "MATCH#m1", "#META", None, None);
        assert_eq!(
            workflow_for(&ev),
            Some(WorkflowStart::FanOut {
                match_id: "m1".into()
            })
        );
    }

    #[test]
    fn match_meta_remove_starts_nothing() {
        let ev = event(ChangeKind::Remove, "MATCH#m1", "#META", None, None);
        assert_eq!(workflow_for(&ev), None);
    }

    #[test]
    fn non_meta_match_write_starts_nothing() {
        // A side write under the same match partition is not a fan-out trigger.
        let ev = event(ChangeKind::Modify, "MATCH#m1", "SIDE#s1", None, None);
        assert_eq!(workflow_for(&ev), None);
    }

    #[test]
    fn accept_transition_starts_accept_saga_with_match_id() {
        let old = invitation("pending", match_ctx());
        let new = invitation("accepted", match_ctx());
        let ev = event(
            ChangeKind::Modify,
            "INVITATION#inv1",
            "#META",
            Some(&old),
            Some(&new),
        );
        assert_eq!(
            workflow_for(&ev),
            Some(WorkflowStart::Accept(
                crate::temporal::workflows::AcceptInvitationInput {
                    invitation_id: "inv1".into(),
                    accepting_user_id: "u_guest".into(),
                    responded_at: "2026-07-02T09:00:00Z".into(),
                    match_id: Some("m1".into()),
                }
            ))
        );
    }

    #[test]
    fn team_invite_accept_has_no_match_fanout() {
        let old = invitation("pending", team_ctx());
        let new = invitation("accepted", team_ctx());
        let ev = event(
            ChangeKind::Modify,
            "INVITATION#inv1",
            "#META",
            Some(&old),
            Some(&new),
        );
        match workflow_for(&ev) {
            Some(WorkflowStart::Accept(input)) => assert_eq!(input.match_id, None),
            other => panic!("expected accept saga, got {other:?}"),
        }
    }

    #[test]
    fn already_accepted_modify_starts_nothing() {
        // Both images already accepted → not a transition → no saga.
        let old = invitation("accepted", match_ctx());
        let new = invitation("accepted", match_ctx());
        let ev = event(
            ChangeKind::Modify,
            "INVITATION#inv1",
            "#META",
            Some(&old),
            Some(&new),
        );
        assert_eq!(workflow_for(&ev), None);
    }

    #[test]
    fn pending_insert_starts_nothing() {
        // A freshly-created pending invitation is not yet accepted.
        let new = invitation("pending", match_ctx());
        let ev = event(ChangeKind::Insert, "INVITATION#inv1", "#META", None, Some(&new));
        assert_eq!(workflow_for(&ev), None);
    }

    #[test]
    fn accept_without_invited_user_starts_nothing() {
        // An accepted invitation with no resolved invitee (bare token) can't
        // identify the accepter, so the saga is skipped.
        let mut new = invitation("accepted", match_ctx());
        new.invited_user_id = None;
        let ev = event(ChangeKind::Modify, "INVITATION#inv1", "#META", None, Some(&new));
        assert_eq!(workflow_for(&ev), None);
    }
}
