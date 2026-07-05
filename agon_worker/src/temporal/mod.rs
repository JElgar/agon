//! Temporal integration: durable orchestration for the multi-step async work
//! (feed fan-out, the accept-invitation saga).
//!
//! ⚠️ ENTIRE MODULE IS UNVERIFIED and gated behind the `temporal` cargo feature
//! (off by default). It targets the Temporal Rust SDK **Public Preview**
//! (`temporalio-sdk` / `temporalio-client`), a churning git dependency with no
//! crates.io release. It cannot be compiled in this environment (no SDK, no
//! Temporal server), so the exact SDK surface (types, method names) must be
//! reconciled against the revision you pin before enabling the feature. The
//! feature-gate keeps the verified inline worker (indexing + notifications)
//! building and shipping regardless.
//!
//! Split of responsibility (see docs/async-design.md §2/§4):
//! - The **SQS consumer** owns *capture* — every committed write arrives via the
//!   stream → pipe → queue and is processed at-least-once. For multi-step work
//!   it *starts* the relevant workflow (via [`client::TemporalClient`]) and ACKs
//!   the message once Temporal has durably accepted the start.
//! - **Temporal** owns *orchestration* — running the workflow to completion with
//!   per-step ret/checkpointing, independent of the worker process lifetime.

pub mod activities;
pub mod client;
pub mod worker;
pub mod workflows;

/// The task queue both the worker and the client use. A single queue is fine —
/// the two workflow types are distinguished by name, not queue.
pub const TASK_QUEUE: &str = "agon-async";

/// Deterministic workflow id for a match fan-out. A duplicate start (e.g. a
/// redelivered stream event) attaches to the existing run instead of
/// double-fanning-out.
pub fn fanout_workflow_id(match_id: &str) -> String {
    format!("fanout-{match_id}")
}

/// Deterministic workflow id for an invitation acceptance saga.
pub fn accept_workflow_id(invitation_id: &str) -> String {
    format!("accept-{invitation_id}")
}
