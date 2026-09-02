//! Agon shared domain crate.
//!
//! Holds the DynamoDB single-table data access layer (`dao`), the Meilisearch
//! client (`search`), the FCM push client (`push`) and the rating engine
//! (`rating`), all used by the API service and the async worker. No
//! web-framework dependencies.

pub mod dao;
pub mod error;
pub mod push;
pub mod rating;
pub mod search;
pub mod telemetry;
