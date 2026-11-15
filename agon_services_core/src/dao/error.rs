use thiserror::Error;

use super::keys::KeyError;

/// Errors returned by the data access layer.
#[derive(Debug, Error)]
pub enum DaoError {
    /// A conditional write failed because the item already exists (e.g. an email
    /// already taken, a duplicate follow). Maps to 409 Conflict at the API layer.
    #[error("conflict: {0}")]
    Conflict(String),

    /// The requested item does not exist. Maps to 404 at the API layer.
    #[error("not found: {0}")]
    NotFound(String),

    /// A stored item's keys or attributes could not be parsed into the expected
    /// shape — indicates data corruption or a schema mismatch, not user error.
    #[error("malformed stored item: {0}")]
    Malformed(String),

    /// Key parse failure (see `KeyError`).
    #[error(transparent)]
    Key(#[from] KeyError),

    /// (De)serialization not tied to a specific stored item (e.g. a pagination
    /// cursor's key map) failed. Reads of an actual item go through
    /// [`SerdeItem`](DaoError::SerdeItem) instead, which carries enough to
    /// find the offending row without a table scan.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_dynamo::Error),

    /// A stored item failed to deserialize into its expected record type.
    /// Carries the item's key and target type alongside the underlying
    /// serde error, so the log line alone is enough to go find and fix the
    /// offending item — no separate diagnostic scan needed. See
    /// `dao::item::from_item`, the sole place this is constructed.
    #[error("serialization error deserializing {type_name} from {pk}/{sk}: {source}")]
    SerdeItem {
        type_name: &'static str,
        pk: String,
        sk: String,
        #[source]
        source: serde_dynamo::Error,
    },

    /// Any underlying DynamoDB error not otherwise classified.
    #[error("dynamodb error: {0}")]
    Dynamo(String),
}

pub type DaoResult<T> = Result<T, DaoError>;
