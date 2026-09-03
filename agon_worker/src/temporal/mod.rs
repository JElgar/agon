//! Temporal integration: durable orchestration for the multi-step async work
//! (feed fan-out, the accept-invitation saga). Built against the Temporal Rust
//! SDK (`temporalio-sdk` / `temporalio-client`, crates.io 0.5).
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

/// Deterministic workflow id for a rating repair — one per (owner, ladder),
/// which is the unit a replay actually works on (`Dao::list_rating_history`
/// takes exactly an owner and a ladder).
///
/// Keyed on the owner rather than on the match that triggered it, even though
/// a re-scored match invalidates every participant at once and this therefore
/// starts N workflows for one event. A match-keyed repair would have to fan
/// back out to one replay per participant anyway — the checkpointed state is a
/// cursor into *one* owner's history — and it would dedupe on the wrong thing:
/// two different changes to the same match are one workflow id, while the same
/// change delivered twice for two different owners are two. Per-owner gets
/// both right, and the N runs are independent (disjoint partitions, no shared
/// writes), so they proceed in parallel.
///
/// The kind is in the id because a user and a team can hold the same id string
/// in principle, and their ratings are different pools entirely (see the plan's
/// Part 2.7). Segments are joined with `-`; ladders never contain one
/// (`rating::Ladder` uses `:` for sub-ladders), so the tail is unambiguous even
/// though an owner id may contain `-` (ids are base64url).
pub fn repair_workflow_id(owner_kind: &str, owner_id: &str, ladder: &str) -> String {
    format!("repair-{owner_kind}-{owner_id}-{ladder}")
}
