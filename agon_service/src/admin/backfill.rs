//! Tracks the OpenSearch backfill job triggered by `POST
//! /admin/opensearch/backfill`, so `GET` on the same path can report
//! progress while it runs.
//!
//! `agon-service` runs as a single replica (see `agon_infra`), so this
//! in-memory, process-local state is all a poller needs — nothing durable is
//! worth adding for an admin action that's run rarely and by hand, same as
//! the `kubectl exec ... agon_worker backfill-opensearch` it replaces. A
//! service restart loses the record of the last run, same as losing a
//! terminal's scrollback would.
//!
//! One run at a time: [`BackfillJob::try_start`] refuses a second trigger
//! while one is already in flight rather than queuing it, since the
//! operation is idempotent and cheap to re-trigger once the first finishes.

use std::sync::Mutex;

use agon_services_core::opensearch::backfill::BackfillProgress;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct JobState {
    pub status: Status,
    pub delete_old: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub progress: BackfillProgress,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct BackfillJob(Mutex<Option<JobState>>);

impl BackfillJob {
    /// The last (or currently running) job, if one has ever been triggered
    /// on this process.
    pub fn snapshot(&self) -> Option<JobState> {
        self.0.lock().unwrap().clone()
    }

    /// Starts a run, unless one is already in flight. Returns `false` (and
    /// leaves the existing job's state untouched) when it is.
    pub fn try_start(&self, delete_old: bool) -> bool {
        let mut guard = self.0.lock().unwrap();
        if matches!(guard.as_ref(), Some(s) if s.status == Status::Running) {
            return false;
        }
        *guard = Some(JobState {
            status: Status::Running,
            delete_old,
            started_at: Utc::now(),
            finished_at: None,
            progress: BackfillProgress::default(),
            error: None,
        });
        true
    }

    /// Called from the spawned task after each page.
    pub fn update_progress(&self, progress: BackfillProgress) {
        if let Some(state) = self.0.lock().unwrap().as_mut() {
            state.progress = progress;
        }
    }

    /// Called once the spawned task's `run_against` call returns.
    pub fn finish(&self, exit_code: i32, error: Option<String>) {
        let mut guard = self.0.lock().unwrap();
        if let Some(state) = guard.as_mut() {
            state.status = if exit_code == 0 {
                Status::Succeeded
            } else {
                Status::Failed
            };
            state.finished_at = Some(Utc::now());
            state.error = error;
        }
    }
}
