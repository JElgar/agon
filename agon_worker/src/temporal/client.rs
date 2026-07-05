//! Temporal client wrapper the SQS consumer uses to *start* workflows on the
//! relevant stream events, with deterministic (idempotent) workflow ids.
//!
//! ⚠️ UNVERIFIED — see `temporal/mod.rs`. Gated behind the `temporal` feature.
//!
//! Starting a workflow whose id already exists is treated as success (the point
//! of deterministic ids): a redelivered stream event attaches to the existing
//! run rather than erroring or double-processing.

use temporalio_client::{
    Client, ClientOptions, Connection, WorkflowOptions, envconfig::LoadClientConfigProfileOptions,
};

use super::workflows::{AcceptInvitation, AcceptInvitationInput, FanOutMatch};
use super::{TASK_QUEUE, accept_workflow_id, fanout_workflow_id};

/// Thin wrapper over a Temporal client for starting Agon workflows.
#[derive(Clone)]
pub struct TemporalClient {
    client: Client,
}

impl TemporalClient {
    /// Connect using the standard Temporal env / config profile.
    pub async fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let (conn_options, client_options) =
            ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())?;
        let connection = Connection::connect(conn_options).await?;
        Ok(Self {
            client: Client::new(connection, client_options),
        })
    }

    /// Start (or attach to) the fan-out workflow for a match. Idempotent via the
    /// deterministic `fanout-<match_id>` id.
    pub async fn start_fanout(&self, match_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .start_workflow(
                FanOutMatch::run,
                match_id.to_string(),
                WorkflowOptions::new(TASK_QUEUE, fanout_workflow_id(match_id)).build(),
            )
            .await?;
        Ok(())
    }

    /// Start (or attach to) the accept-invitation saga. Idempotent via the
    /// deterministic `accept-<invitation_id>` id.
    pub async fn start_accept(
        &self,
        input: AcceptInvitationInput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = accept_workflow_id(&input.invitation_id);
        self.client
            .start_workflow(
                AcceptInvitation::run,
                input,
                WorkflowOptions::new(TASK_QUEUE, id).build(),
            )
            .await?;
        Ok(())
    }
}
