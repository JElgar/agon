//! Worker error type.
//!
//! A handler returning `Err` means the SQS message is **not** deleted, so SQS
//! redelivers it (at-least-once; see docs/async-design.md §3). Handlers must
//! therefore be idempotent. A message whose body can't even be parsed is a
//! permanent poison message: we log and drop it (delete) rather than loop
//! forever — the DLQ redrive policy is the backstop for repeated failures.

use agon_core::dao::error::DaoError;
use agon_core::dao::keys::KeyError;
use agon_core::error::SearchError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("configuration error: {0}")]
    Config(String),

    /// The SQS message body could not be parsed into an envelope. Permanent —
    /// the message is dropped rather than retried.
    #[error("malformed message: {0}")]
    Malformed(String),

    /// A state the write path is supposed to make impossible was observed (e.g.
    /// a self-follow edge, which the DAO rejects). This is a bug or data
    /// corruption, not user error or a transient fault — fail loudly rather than
    /// silently proceed. Permanent: the message goes to the DLQ for inspection.
    #[error("invariant violated: {0}")]
    Invariant(String),

    /// A key in the envelope could not be parsed. Permanent (bad data).
    #[error("key parse error: {0}")]
    Key(#[from] KeyError),

    /// A DAO operation failed. Usually transient → retry (redeliver).
    #[error(transparent)]
    Dao(#[from] DaoError),

    /// A Meilisearch call failed. Usually transient → retry.
    #[error(transparent)]
    Search(#[from] SearchError),

    /// An SQS operation failed.
    #[error("sqs error: {0}")]
    Sqs(String),
}

impl WorkerError {
    /// Whether this failure is permanent (bad data) vs. transient (retryable).
    /// Permanent failures are dropped (deleted) so they don't loop; transient
    /// failures are left on the queue to redeliver.
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            WorkerError::Malformed(_) | WorkerError::Key(_) | WorkerError::Invariant(_)
        )
    }
}

pub type WorkerResult<T> = Result<T, WorkerError>;
