//! Data access layer for the DynamoDB single-table design.
//!
//! Not yet wired into the API — this is being built out independently. See
//! `docs/dynamodb-design.md` for the table/key/access-pattern design.
//!
//! Layout:
//! - `keys`    — typed PK/SK enums (format/parse).
//! - `error`   — `DaoError` / `DaoResult`.
//! - `item`    — struct ↔ DynamoDB item map bridging (+ GSI stamping).
//! - `page`    — cursor pagination over Query.
//! - `records` — the persisted record structs (DAO-owned, distinct from API).
//! - `client`  — the `Dao` handle wrapping the SDK client.
//! - `batch`   — shared `BatchGetItem` retry + backoff plumbing.
//! - `user`, `follow`, … — per-entity operations (impl blocks on `Dao`).

pub mod client;
pub mod error;
pub mod item;
pub mod keys;
pub mod page;
pub mod records;

pub mod accept;
pub mod asset;
pub mod audience;
pub mod batch;
pub mod feed;
pub mod follow;
pub mod invitation;
pub mod match_ops;
pub mod match_social;
pub mod notification;
pub mod stats;
pub mod team;
pub mod user;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;

/// True if a `TransactWriteItems` failure was caused by a condition check (one
/// of our `attribute_(not_)exists` guards) rather than a transient error.
/// Shared by every op that runs a guarded transaction.
pub(crate) fn is_transaction_conditional_failure(err: &SdkError<TransactWriteItemsError>) -> bool {
    match err {
        SdkError::ServiceError(se) => match se.err() {
            TransactWriteItemsError::TransactionCanceledException(e) => e
                .cancellation_reasons()
                .iter()
                .any(|r| r.code() == Some("ConditionalCheckFailed")),
            _ => false,
        },
        _ => false,
    }
}

// Re-exported for the API layer once wired in; unused within the crate for now.
#[allow(unused_imports)]
pub use client::Dao;
#[allow(unused_imports)]
pub use error::{DaoError, DaoResult};
#[allow(unused_imports)]
pub use page::Page;
