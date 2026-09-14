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
pub mod device;
pub mod device_pairing;
pub mod feed;
pub mod follow;
pub mod invitation;
pub mod join_link;
pub mod live_score_ops;
pub mod match_ops;
pub mod match_social;
pub mod notification;
pub mod paired_device;
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

/// Whether the transact-item at `index` (0-based, matching the order items
/// were added via `transact_items`) failed its own `ConditionExpression`, for
/// a cancelled `TransactWriteItems`. `false` for any other kind of failure
/// (network, throttling, ...) — those should propagate as real errors, not
/// get misread as "the condition failed". For a transaction with more than
/// one guard, this is how a caller tells *which* fact changed without acting
/// on a separate, possibly stale read of it.
pub(crate) fn item_condition_failed(err: &SdkError<TransactWriteItemsError>, index: usize) -> bool {
    match err {
        SdkError::ServiceError(se) => match se.err() {
            TransactWriteItemsError::TransactionCanceledException(e) => {
                e.cancellation_reasons().get(index).and_then(|r| r.code())
                    == Some("ConditionalCheckFailed")
            }
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
