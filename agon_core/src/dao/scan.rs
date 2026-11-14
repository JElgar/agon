//! Full-table scans, for offline jobs only (the OpenSearch backfill). Nothing
//! on a request path scans.

use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, item_pk, s};
use super::keys::{Pk, Sk};
use super::page::{Page, decode_cursor, encode_cursor};

impl Dao {
    /// One page of match ids, read with a filtered `Scan` over every
    /// `MATCH#<id>` / `#META` item. Only the key is projected, so a page is
    /// cheap; callers re-read each match as they process it.
    ///
    /// `segment` is `(segment, total_segments)` for a parallel scan, or `None`.
    /// `page_size` caps items *examined* per call (DynamoDB applies the filter
    /// after reading), so a page can hold fewer ids than that, or none, while
    /// `next_cursor` is still `Some`. Keep paging until it's `None`.
    #[tracing::instrument(skip(self))]
    pub async fn scan_match_ids(
        &self,
        segment: Option<(i32, i32)>,
        cursor: Option<&str>,
        page_size: i32,
    ) -> DaoResult<Page<String>> {
        let start_key = cursor.map(decode_cursor).transpose()?;
        let mut req = self
            .client
            .scan()
            .table_name(self.table())
            .filter_expression("#sk = :meta AND begins_with(#pk, :prefix)")
            .projection_expression("#pk, #sk")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#sk", ATTR_SK)
            .expression_attribute_values(":meta", s(Sk::Meta.to_string()))
            .expression_attribute_values(":prefix", s(Pk::Match(String::new()).to_string()))
            .limit(page_size)
            .set_exclusive_start_key(start_key);
        if let Some((seg, total)) = segment {
            req = req.segment(seg).total_segments(total);
        }
        let out = req
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut items = Vec::new();
        for item in out.items.unwrap_or_default() {
            if let Pk::Match(id) = item_pk(&item)? {
                items.push(id);
            }
        }
        let next_cursor = out
            .last_evaluated_key
            .map(|k: std::collections::HashMap<String, AttributeValue>| encode_cursor(k))
            .transpose()?;
        Ok(Page { items, next_cursor })
    }
}
