//! Starts/queries the OpenSearch backfill Temporal workflow from the admin
//! API, instead of `agon-service` running the backfill itself and holding its
//! progress in memory. `agon_worker`'s Temporal worker is what actually runs
//! it (see `agon_worker::temporal::worker::run`); this is a plain client
//! connection only — see
//! `agon_services_core::opensearch::backfill_workflow`'s doc comment for why
//! the workflow + activities live in the shared crate rather than here or in
//! `agon_worker`.
//!
//! The workflow id is fixed (`WORKFLOW_ID`): there's only ever one backfill.
//! Starting it with `WorkflowIdConflictPolicy::Fail` while one is already
//! running is what gives `POST /admin/opensearch/backfill` its 409 for free,
//! with no job-tracking of our own.

use agon_services_core::opensearch::backfill_workflow::{
    BackfillOpenSearch, BackfillWorkflowInput, BackfillWorkflowStatus, TASK_QUEUE, WORKFLOW_ID,
};
use temporalio_client::envconfig::LoadClientConfigProfileOptions;
use temporalio_client::errors::{WorkflowInteractionError, WorkflowStartError};
use temporalio_client::{
    Client, ClientOptions, Connection, WorkflowDescribeOptions, WorkflowQueryOptions,
    WorkflowStartOptions,
};
use temporalio_common::protos::temporal::api::enums::v1::{
    WorkflowExecutionStatus, WorkflowIdConflictPolicy,
};

/// The admin API's view of a run: execution-level status plus the workflow's
/// own progress counters (from its `status` query, best-effort — a query
/// failure still reports the execution status, just with zeroed counters).
#[derive(Debug, Clone)]
pub struct BackfillRun {
    pub status: RunStatus,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub progress: BackfillWorkflowStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
}

/// A connection to Temporal, scoped to starting/querying the backfill
/// workflow. Cheap to clone (the underlying client is itself a thin,
/// cloneable gRPC channel wrapper), so one connection built at startup is
/// shared across requests the same way the DAO/OpenSearch clients are.
#[derive(Clone)]
pub struct BackfillClient {
    client: Client,
}

impl BackfillClient {
    /// Connect using the standard `TEMPORAL_*` env / config profile — same as
    /// `agon_worker::temporal::client::TemporalClient::connect`.
    pub async fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let (conn_options, client_options) =
            ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())?;
        let connection = Connection::connect(conn_options).await?;
        Ok(Self {
            client: Client::new(connection, client_options)?,
        })
    }

    /// Start a run, or report that one is already in flight. `Err` is an
    /// unexpected Temporal-side failure (connection, serialization), not an
    /// ordinary "already running" — callers map that to a 500, not a 409.
    pub async fn start(
        &self,
        delete_old: bool,
    ) -> Result<StartOutcome, Box<dyn std::error::Error>> {
        let input = BackfillWorkflowInput {
            delete_old,
            ..Default::default()
        };
        let result = self
            .client
            .start_workflow(
                BackfillOpenSearch::run,
                input,
                WorkflowStartOptions::new(TASK_QUEUE, WORKFLOW_ID)
                    .id_conflict_policy(WorkflowIdConflictPolicy::Fail)
                    .build(),
            )
            .await;
        match result {
            Ok(_handle) => Ok(StartOutcome::Started),
            Err(WorkflowStartError::AlreadyStarted { .. }) => Ok(StartOutcome::AlreadyRunning),
            Err(e) => Err(Box::new(e)),
        }
    }

    /// The current (or most recent) run, or `None` if a backfill has never
    /// been triggered via this path. `Err` is an unexpected Temporal-side
    /// failure, not "never run" — that's `Ok(None)`.
    pub async fn current_run(&self) -> Result<Option<BackfillRun>, Box<dyn std::error::Error>> {
        let handle = self
            .client
            .get_workflow_handle::<BackfillOpenSearch>(WORKFLOW_ID);

        let description = match handle.describe(WorkflowDescribeOptions::default()).await {
            Ok(d) => d,
            Err(WorkflowInteractionError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let info = description.raw_description.workflow_execution_info;
        let exec_status = info
            .as_ref()
            .and_then(|i| WorkflowExecutionStatus::try_from(i.status).ok())
            .unwrap_or(WorkflowExecutionStatus::Unspecified);
        let status = match exec_status {
            WorkflowExecutionStatus::Running | WorkflowExecutionStatus::Unspecified => {
                RunStatus::Running
            }
            WorkflowExecutionStatus::Completed => RunStatus::Succeeded,
            WorkflowExecutionStatus::Failed
            | WorkflowExecutionStatus::Canceled
            | WorkflowExecutionStatus::Terminated
            | WorkflowExecutionStatus::TimedOut
            | WorkflowExecutionStatus::ContinuedAsNew
            | WorkflowExecutionStatus::Paused => RunStatus::Failed,
        };
        let started_at = info
            .as_ref()
            .and_then(|i| i.start_time.as_ref())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32));
        let finished_at = info
            .as_ref()
            .and_then(|i| i.close_time.as_ref())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32));

        // Temporal answers queries against closed workflow executions too
        // (by replaying their history), so this works whether the run is
        // still going or already finished — a query failure (e.g. a very
        // old run whose history has been retention-deleted) just means the
        // counters come back zeroed rather than failing the whole request.
        let progress = handle
            .query(
                BackfillOpenSearch::status,
                (),
                WorkflowQueryOptions::default(),
            )
            .await
            .unwrap_or_default();

        Ok(Some(BackfillRun {
            status,
            started_at,
            finished_at,
            progress,
        }))
    }
}

pub enum StartOutcome {
    Started,
    AlreadyRunning,
}
