//! Cursor pagination for Query operations.
//!
//! A cursor is DynamoDB's `LastEvaluatedKey` (an attribute map) serialized to
//! JSON and base64url-encoded — opaque to clients, decoded back into the
//! `ExclusiveStartKey` on the next call.

use std::collections::HashMap;

use aws_sdk_dynamodb::operation::query::builders::QueryFluentBuilder;
use aws_sdk_dynamodb::types::AttributeValue;
use base64::{Engine, prelude::BASE64_URL_SAFE};
use serde::de::DeserializeOwned;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::from_item;

/// A page of results plus the opaque cursor for the next page (`None` = end).
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl Dao {
    /// Run a prepared query with cursor pagination, deserializing each returned
    /// item into `T`. The caller supplies everything except `limit` and the
    /// start key. Results honour the query's own sort direction (set
    /// `scan_index_forward(false)` on the builder for newest-first).
    pub(super) async fn query_page<T: DeserializeOwned>(
        &self,
        query: QueryFluentBuilder,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<T>> {
        let start_key = match cursor {
            Some(c) => Some(decode_cursor(c)?),
            None => None,
        };

        let out = query
            .limit(limit as i32)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let items = out
            .items
            .unwrap_or_default()
            .into_iter()
            .map(from_item)
            .collect::<DaoResult<Vec<T>>>()?;

        let next_cursor = out.last_evaluated_key.map(encode_cursor).transpose()?;

        Ok(Page { items, next_cursor })
    }
}

/// Encode a `LastEvaluatedKey` map into an opaque cursor string.
fn encode_cursor(key: HashMap<String, AttributeValue>) -> DaoResult<String> {
    // serde_dynamo round-trips the key map through serde_json::Value.
    let value: serde_json::Value = serde_dynamo::from_item(key)?;
    let json =
        serde_json::to_vec(&value).map_err(|e| DaoError::Dynamo(format!("cursor encode: {e}")))?;
    Ok(BASE64_URL_SAFE.encode(json))
}

/// Decode an opaque cursor string back into an `ExclusiveStartKey` map.
fn decode_cursor(cursor: &str) -> DaoResult<HashMap<String, AttributeValue>> {
    let bytes = BASE64_URL_SAFE
        .decode(cursor)
        .map_err(|_| DaoError::Malformed("invalid cursor".into()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| DaoError::Malformed("invalid cursor".into()))?;
    Ok(serde_dynamo::to_item(value)?)
}
