//! The individual inline handlers for stream events (single-step, idempotent
//! work), each run by exactly one fanned-out message now — see
//! `action_consumer` for the dispatch and `opensearch_sync` for the one
//! handler (OpenSearch) that isn't a plain [`crate::action::Action`].
//!
//! Each handler is total over the event space: it inspects the event's keys and
//! does nothing for events it doesn't care about.
//!
//! Multi-step work (feed fan-out, the accept-invitation saga) is **not** here —
//! it's delegated to Temporal (see `crate::temporal`). This module is the
//! inline slice only.

pub mod index;
pub mod notify;
pub mod opensearch;
pub mod push;
pub mod rating;
pub mod stats;
pub mod tournament;
