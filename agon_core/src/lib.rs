//! Agon shared domain crate.
//!
//! Holds the DynamoDB single-table data access layer (`dao`), the Meilisearch
//! client (`search`), and the FCM push client (`push`), all used by the API
//! service and the async worker. No web-framework dependencies.

pub mod dao;
pub mod error;
pub mod push;
pub mod search;
pub mod telemetry;
