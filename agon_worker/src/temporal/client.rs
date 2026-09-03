//! Temporal client wrapper the SQS consumer uses to *start* workflows on the
//! relevant stream events, with deterministic (idempotent) workflow ids.
//!
//! Starting a workflow whose id already exists is treated as success (the point
//! of deterministic ids): a redelivered stream event attaches to the existing
//! run rather than erroring or double-processing.

use temporalio_client::{
    Client, ClientOptions, Connection, WorkflowStartOptions,
    envconfig::LoadClientConfigProfileOptions,
};
use temporalio_common::protos::temporal::api::enums::v1::WorkflowIdConflictPolicy;

use super::workflows::{
    AcceptInvitation, AcceptInvitationInput, FanOutMatch, RepairRatings, RepairRatingsInput,
};
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
            client: Client::new(connection, client_options)?,
        })
    }

    /// Start (or attach to) the fan-out workflow for a match. Idempotent via the
    /// deterministic `fanout-<match_id>` id: the `UseExisting` conflict policy
    /// means a duplicate start returns a handle to the running run rather than
    /// erroring, so a redelivered stream event is a no-op.
    pub async fn start_fanout(&self, match_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .start_workflow(
                FanOutMatch::run,
                match_id.to_string(),
                WorkflowStartOptions::new(TASK_QUEUE, fanout_workflow_id(match_id))
                    .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                    .build(),
            )
            .await?;
        Ok(())
    }

    /// Start (or attach to) a rating repair for one owner and ladder.
    ///
    /// `UseExisting` is doing more work here than it does for the other two.
    /// A single re-scored match starts one of these per participant, each
    /// under its own id, and each of the three triggers can fire repeatedly
    /// for the same owner as a match's `#META` is rewritten by likes and
    /// comments — so duplicate starts are the norm, not an edge case. Attaching
    /// is sound because every run replays the owner's whole ladder, i.e. all
    /// runs do the identical job (see [`RepairRatings`], which also documents
    /// the one window this leaves open).
    pub async fn start_repair(
        &self,
        input: RepairRatingsInput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = input.workflow_id();
        self.client
            .start_workflow(
                RepairRatings::run,
                input,
                WorkflowStartOptions::new(TASK_QUEUE, id)
                    .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                    .build(),
            )
            .await?;
        Ok(())
    }

    /// Start (or attach to) the accept-invitation saga. Idempotent via the
    /// deterministic `accept-<invitation_id>` id + `UseExisting` conflict policy.
    pub async fn start_accept(
        &self,
        input: AcceptInvitationInput,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = accept_workflow_id(&input.invitation_id);
        self.client
            .start_workflow(
                AcceptInvitation::run,
                input,
                WorkflowStartOptions::new(TASK_QUEUE, id)
                    .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                    .build(),
            )
            .await?;
        Ok(())
    }
}
