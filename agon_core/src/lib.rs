//! Agon shared domain crate.
//!
//! Holds the DynamoDB single-table data access layer (`dao`) and the Meilisearch
//! client (`search`), both used by the API service and the async worker. No
//! web-framework dependencies.

pub mod dao;
pub mod error;
pub mod search;
