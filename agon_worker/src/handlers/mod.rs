//! Inline handlers for stream events (single-step, idempotent work).
//!
//! Each handler is total over the event space: it inspects the event's keys and
//! does nothing for events it doesn't care about. The [`route`] dispatcher runs
//! every applicable handler for one event; a failure in any handler fails the
//! whole event so SQS redelivers (and idempotent handlers make the replay safe).
//!
//! Multi-step work (feed fan-out, the accept-invitation saga) is **not** here —
//! it will be delegated to Temporal in a later pass (see docs/async-design.md
//! §5). This module is the inline slice only, with one deliberate exception:
//! [`rating`] both *starts* the `RepairRatings` workflow and supplies its
//! body, because the replay has to share the incremental path's eligibility
//! gate or the two would drift apart.

pub mod index;
pub mod notify;
pub mod push;
pub mod rating;
pub mod stats;

use agon_core::dao::Dao;

use crate::error::WorkerResult;
use crate::event::ChangeEvent;
use crate::temporal::client::TemporalClient;
use agon_core::push::PushClient;
use agon_core::search::SearchClient;

/// Run every inline handler applicable to one event. `now` is the processing
/// timestamp (RFC3339), used where an event carries no timestamp of its own.
///
/// Ordering: indexing, notifications, push, stats, then ratings. All are
/// independent and idempotent, so if a later one fails after an earlier
/// succeeded, redelivery re-runs them all harmlessly. `push` runs after
/// `notify` deliberately: a `NotificationRecord` write from `notify::handle`
/// produces its own stream event, which `push::handle` reacts to on a later
/// call to `route` — see `handlers/push.rs`'s module docs.
///
/// `rating` runs last, and unlike the rest of the ordering that is not
/// arbitrary: it is the only handler that can fail *permanently* (a match the
/// engine refuses to rate is a `WorkerError::Invariant`, which parks the
/// message in the DLQ rather than redelivering it). Running it last means a
/// corrupt match still gets indexed, notified and counted before the message
/// stops being retried.
///
/// `temporal` is threaded in for the same reason `push` is: the rating handler
/// starts a `RepairRatings` workflow when it finds a rating it cannot bring up
/// to date incrementally, and `None` (unit tests, a local run without the
/// `full` compose profile) degrades that to a log line. The alternative —
/// returning the repairs and having the consumer start them next to the
/// fan-out — was rejected because it would split "detect" from "start" across
/// two files and let a future trigger be added that only does the first half.
pub async fn route(
    dao: &Dao,
    search: &SearchClient,
    push: Option<&PushClient>,
    temporal: Option<&TemporalClient>,
    ui_base_url: Option<&str>,
    ev: &ChangeEvent,
    now: &str,
) -> WorkerResult<()> {
    index::handle(dao, search, ev).await?;
    notify::handle(dao, ev, now).await?;
    push::handle(dao, push, ui_base_url, ev).await?;
    stats::handle(dao, ev).await?;
    rating::handle(dao, temporal, ev, now).await?;
    Ok(())
}
