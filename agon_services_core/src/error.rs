//! Crate-level error types shared across modules.
//!
//! The `dao` module has its own `DaoError`; this holds errors for the other
//! shared clients (currently push).

use thiserror::Error;

/// An FCM operation failed — network, credential/token exchange, or a non-2xx
/// response that wasn't a recognised "stale token" error. Callers treat this
/// as transient and retry (the worker leaves the SQS message on the queue).
#[derive(Debug, Error)]
#[error("push error: {0}")]
pub struct PushError(pub String);
