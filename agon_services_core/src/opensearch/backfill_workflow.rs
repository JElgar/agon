//! The OpenSearch backfill as a Temporal workflow, so a long-running backfill
//! survives a restart of whichever `agon_worker` pod was running it and
//! exposes its progress to a client via a query — rather than `agon_service`
//! holding that state itself (see `agon_service::admin`'s `POST`/`GET
//! /admin/opensearch/backfill`, the only caller that starts/queries this
//! workflow outside `agon_worker`'s own worker).
//!
//! Lives here, not in `agon_worker`, so `agon_service` can start and query it
//! directly with its own plain [`temporalio_client::Client`] — `agon_service`
//! doesn't depend on `agon_worker` and runs no Temporal worker of its own.
//! `agon_worker` registers it on its existing worker/task queue alongside its
//! other workflows (see `agon_worker::temporal::worker::run`).
//!
//! Each page (of matches, then users, then teams) is its own activity, same as
//! `FanOutMatch`'s feed chunks in `agon_worker` — a crash resumes from the
//! last completed page's cursor, not from the top. The workflow id is the
//! fixed [`WORKFLOW_ID`] (there's only ever one backfill at a time); starting
//! it with [`temporalio_common::protos::temporal::api::enums::v1::WorkflowIdConflictPolicy::Fail`]
//! gives the admin API's "already running" 409 for free.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::activities::{ActivityContext, ActivityError};
use temporalio_sdk::{ActivityOptions, WorkflowContext, WorkflowResult};
use temporalio_workflow::WorkflowContextView;

use crate::dao::Dao;
use crate::opensearch::OpenSearchClient;
use crate::opensearch::backfill::{self, PageOutcome};

/// The task queue this workflow is started on. Must match
/// `agon_worker::temporal::TASK_QUEUE` — the queue `agon_worker`'s Temporal
/// worker (the only thing that runs this workflow) actually polls.
pub const TASK_QUEUE: &str = "agon-async";

/// Fixed workflow id: there's only ever one backfill running. Starting with
/// `WorkflowIdConflictPolicy::Fail` while one is already running surfaces as
/// `WorkflowStartError::AlreadyStarted`, which the admin API maps to 409.
pub const WORKFLOW_ID: &str = "opensearch-backfill";

/// Default activity timeout. A page's work is bounded by its own page size
/// and concurrency, but a cold OpenSearch connection can be slow, so this is
/// generous rather than tight.
fn activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(300))
}

/// Inputs to [`BackfillOpenSearch`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillWorkflowInput {
    pub delete_old: bool,
    pub page_size: u32,
    pub concurrency: usize,
}

impl Default for BackfillWorkflowInput {
    fn default() -> Self {
        let defaults = backfill::BackfillArgs::default();
        Self {
            delete_old: false,
            page_size: defaults.page_size,
            concurrency: defaults.concurrency,
        }
    }
}

/// Snapshot of a run's progress — the [`BackfillOpenSearch::status`] query's
/// output, and the workflow's final result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackfillWorkflowStatus {
    /// "matches" | "users" | "teams" | "finishing" | "done".
    pub phase: String,
    pub delete_old: bool,
    pub match_pages: u64,
    pub matches_indexed: u64,
    pub matches_removed: u64,
    pub matches_failed: u64,
    pub user_pages: u64,
    pub users_indexed: u64,
    pub users_removed: u64,
    pub users_failed: u64,
    pub team_pages: u64,
    pub teams_indexed: u64,
    pub teams_removed: u64,
    pub teams_failed: u64,
}

impl BackfillWorkflowStatus {
    fn record_match_page(&mut self, outcome: &PageOutcome) {
        self.match_pages += 1;
        self.matches_indexed += outcome.indexed;
        self.matches_removed += outcome.removed;
        self.matches_failed += outcome.failed;
    }

    fn record_user_page(&mut self, outcome: &PageOutcome) {
        self.user_pages += 1;
        self.users_indexed += outcome.indexed;
        self.users_removed += outcome.removed;
        self.users_failed += outcome.failed;
    }

    fn record_team_page(&mut self, outcome: &PageOutcome) {
        self.team_pages += 1;
        self.teams_indexed += outcome.indexed;
        self.teams_removed += outcome.removed;
        self.teams_failed += outcome.failed;
    }
}

/// Backfills every OpenSearch index from DynamoDB, page by page, then swaps
/// their aliases onto this version. Workflow id: the fixed [`WORKFLOW_ID`].
#[workflow]
#[derive(Default)]
pub struct BackfillOpenSearch {
    status: BackfillWorkflowStatus,
}

#[workflow_methods]
impl BackfillOpenSearch {
    #[run]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: BackfillWorkflowInput,
    ) -> WorkflowResult<BackfillWorkflowStatus> {
        ctx.state_mut(|w| {
            w.status.phase = "matches".to_string();
            w.status.delete_old = input.delete_old;
        });

        ctx.start_activity(BackfillActivities::ensure_ready, (), activity_opts())
            .await?;

        let mut cursor: Option<String> = None;
        loop {
            let outcome = ctx
                .start_activity(
                    BackfillActivities::backfill_match_page,
                    PageInput {
                        cursor: cursor.clone(),
                        page_size: input.page_size,
                        concurrency: input.concurrency,
                    },
                    activity_opts(),
                )
                .await?;
            cursor = outcome.next_cursor.clone();
            ctx.state_mut(|w| w.status.record_match_page(&outcome));
            if cursor.is_none() {
                break;
            }
        }

        ctx.state_mut(|w| w.status.phase = "users".to_string());

        let mut cursor: Option<String> = None;
        loop {
            let outcome = ctx
                .start_activity(
                    BackfillActivities::backfill_user_page,
                    PageInput {
                        cursor: cursor.clone(),
                        page_size: input.page_size,
                        concurrency: input.concurrency,
                    },
                    activity_opts(),
                )
                .await?;
            cursor = outcome.next_cursor.clone();
            ctx.state_mut(|w| w.status.record_user_page(&outcome));
            if cursor.is_none() {
                break;
            }
        }

        ctx.state_mut(|w| w.status.phase = "teams".to_string());

        let mut cursor: Option<String> = None;
        loop {
            let outcome = ctx
                .start_activity(
                    BackfillActivities::backfill_team_page,
                    PageInput {
                        cursor: cursor.clone(),
                        page_size: input.page_size,
                        concurrency: input.concurrency,
                    },
                    activity_opts(),
                )
                .await?;
            cursor = outcome.next_cursor.clone();
            ctx.state_mut(|w| w.status.record_team_page(&outcome));
            if cursor.is_none() {
                break;
            }
        }

        ctx.state_mut(|w| w.status.phase = "finishing".to_string());
        ctx.start_activity(
            BackfillActivities::finish,
            FinishInput {
                swap: true,
                delete_old: input.delete_old,
            },
            activity_opts(),
        )
        .await?;

        ctx.state_mut(|w| w.status.phase = "done".to_string());
        Ok(ctx.state(|w| w.status.clone()))
    }

    /// Live progress while the run is in flight — also readable, identically,
    /// from the final result once it's done.
    #[query]
    pub fn status(&self, _ctx: &WorkflowContextView) -> BackfillWorkflowStatus {
        self.status.clone()
    }
}

/// One page request, for the match, user and team phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInput {
    pub cursor: Option<String>,
    pub page_size: u32,
    pub concurrency: usize,
}

/// The finishing step's input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishInput {
    pub swap: bool,
    pub delete_old: bool,
}

/// Activities backing [`BackfillOpenSearch`] — thin wrappers over the pure
/// page-level functions in `opensearch::backfill`, which also back the
/// `agon_worker backfill-opensearch` CLI (see that module's doc comment).
pub struct BackfillActivities {
    pub dao: Dao,
    pub os: OpenSearchClient,
}

#[activities]
impl BackfillActivities {
    #[activity]
    pub async fn ensure_ready(
        self: Arc<Self>,
        _ctx: ActivityContext,
        _input: (),
    ) -> Result<(), ActivityError> {
        backfill::ensure_ready(&self.os)
            .await
            .map_err(ActivityError::from)
    }

    #[activity]
    pub async fn backfill_match_page(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: PageInput,
    ) -> Result<PageOutcome, ActivityError> {
        backfill::backfill_match_page(
            &self.os,
            &self.dao,
            input.cursor,
            input.page_size,
            input.concurrency,
        )
        .await
        .map_err(ActivityError::from)
    }

    #[activity]
    pub async fn backfill_user_page(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: PageInput,
    ) -> Result<PageOutcome, ActivityError> {
        backfill::backfill_user_page(
            &self.os,
            &self.dao,
            input.cursor,
            input.page_size,
            input.concurrency,
        )
        .await
        .map_err(ActivityError::from)
    }

    #[activity]
    pub async fn backfill_team_page(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: PageInput,
    ) -> Result<PageOutcome, ActivityError> {
        backfill::backfill_team_page(
            &self.os,
            &self.dao,
            input.cursor,
            input.page_size,
            input.concurrency,
        )
        .await
        .map_err(ActivityError::from)
    }

    #[activity]
    pub async fn finish(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: FinishInput,
    ) -> Result<(), ActivityError> {
        backfill::finish(&self.os, input.swap, input.delete_old)
            .await
            .map_err(ActivityError::from)
    }
}
