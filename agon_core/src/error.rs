//! Crate-level error types shared across modules.
//!
//! The `dao` module has its own `DaoError`; this holds errors for the other
//! shared clients (currently search and push).

use thiserror::Error;

/// A Meilisearch operation failed (network, non-2xx response, or a malformed
/// response body). Callers decide how to surface it (the worker treats it as
/// transient and retries; the API maps it to a 500).
#[derive(Debug, Error)]
#[error("meilisearch error: {0}")]
pub struct SearchError(pub String);

/// An FCM operation failed — network, credential/token exchange, or a non-2xx
/// response that wasn't a recognised "stale token" error. Callers treat this
/// as transient and retry (the worker leaves the SQS message on the queue).
#[derive(Debug, Error)]
#[error("push error: {0}")]
pub struct PushError(pub String);
