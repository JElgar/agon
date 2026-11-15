//! Our OpenSearch client: the official `opensearch` crate underneath, wrapped
//! in just the calls the worker and the backfill need (create index, point
//! alias, bulk write, id lookup, search).
//!
//! **Where the typing lives.** The official crate is untyped for request and
//! response bodies (`serde_json::Value` in and out). Rather than let JSON leak
//! into callers, this module is the boundary: documents are typed structs
//! (`docs.rs`), index shapes are defined once (`schema.rs`), and the read
//! queries, when they arrive, will be typed functions returning typed rows in
//! this module. Callers never build query JSON themselves.

use std::time::Duration;

use opensearch::auth::Credentials;
use opensearch::cluster::ClusterHealthParts;
use opensearch::http::Url;
use opensearch::http::request::JsonBody;
use opensearch::http::response::Response;
use opensearch::http::transport::{SingleNodeConnectionPool, TransportBuilder};
use opensearch::indices::{
    IndicesCreateParts, IndicesDeleteParts, IndicesExistsParts, IndicesGetAliasParts,
    IndicesRefreshParts,
};
use opensearch::params::Refresh;
use opensearch::{BulkParts, OpenSearch, SearchParts};
use serde::Serialize;
use serde_json::{Value, json};

use super::schema::IndexDef;

#[derive(Debug, thiserror::Error)]
pub enum OpenSearchError {
    #[error("opensearch client error: {0}")]
    Client(#[from] opensearch::Error),
    #[error("opensearch: invalid OPENSEARCH_URL `{url}`: {reason}")]
    Config { url: String, reason: String },
    #[error("opensearch returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("opensearch bulk request had {failed} failed item(s), first: {first}")]
    Bulk { failed: usize, first: String },
    #[error("opensearch: could not encode document: {0}")]
    Encode(#[from] serde_json::Error),
}

pub type OpenSearchResult<T> = Result<T, OpenSearchError>;

/// One operation in a `_bulk` request.
pub enum BulkOp {
    /// Create or fully replace the document `id` in `index`.
    Index {
        index: String,
        id: String,
        doc: Value,
    },
    /// Delete the document `id` from `index` (a missing document is fine).
    Delete { index: String, id: String },
}

impl BulkOp {
    pub fn index<T: Serialize>(index: &str, id: &str, doc: &T) -> OpenSearchResult<Self> {
        Ok(BulkOp::Index {
            index: index.to_string(),
            id: id.to_string(),
            doc: serde_json::to_value(doc)?,
        })
    }
}

/// Connection to one OpenSearch cluster, plus the deployment's index-name
/// prefix (empty by default; set it if two environments share a cluster).
#[derive(Clone)]
pub struct OpenSearchClient {
    client: OpenSearch,
    prefix: String,
}

impl OpenSearchClient {
    pub fn new(
        base_url: &str,
        auth: Option<(String, String)>,
        prefix: impl Into<String>,
    ) -> OpenSearchResult<Self> {
        let config_err = |reason: String| OpenSearchError::Config {
            url: base_url.to_string(),
            reason,
        };
        let url = Url::parse(base_url).map_err(|e| config_err(e.to_string()))?;
        let mut builder = TransportBuilder::new(SingleNodeConnectionPool::new(url))
            .timeout(Duration::from_secs(30));
        if let Some((user, pass)) = auth {
            builder = builder.auth(Credentials::Basic(user, pass));
        }
        let transport = builder.build().map_err(|e| config_err(e.to_string()))?;
        Ok(Self {
            client: OpenSearch::new(transport),
            prefix: prefix.into(),
        })
    }

    /// Build from the environment. `Ok(None)` when `OPENSEARCH_URL` is unset
    /// (OpenSearch is opt-in, like push notifications and telemetry); `Err` when
    /// it's set but invalid.
    ///
    /// - `OPENSEARCH_URL`: e.g. `http://opensearch:9200`
    /// - `OPENSEARCH_USERNAME` / `OPENSEARCH_PASSWORD`: optional basic auth
    /// - `OPENSEARCH_INDEX_PREFIX`: optional, prepended to every index name
    pub fn from_env() -> OpenSearchResult<Option<Self>> {
        let Some(url) = std::env::var("OPENSEARCH_URL")
            .ok()
            .filter(|u| !u.is_empty())
        else {
            return Ok(None);
        };
        let auth = match (
            std::env::var("OPENSEARCH_USERNAME"),
            std::env::var("OPENSEARCH_PASSWORD"),
        ) {
            (Ok(u), Ok(p)) if !u.is_empty() => Some((u, p)),
            _ => None,
        };
        let prefix = std::env::var("OPENSEARCH_INDEX_PREFIX").unwrap_or_default();
        Self::new(&url, auth, prefix).map(Some)
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The physical index this build writes to for `def`.
    pub fn write_index(&self, def: &IndexDef) -> String {
        def.physical(&self.prefix)
    }

    /// Read a response as (status, JSON body). An empty body (e.g. a HEAD)
    /// becomes `Null`; a non-JSON body becomes a JSON string.
    async fn read(resp: Response) -> OpenSearchResult<(u16, Value)> {
        let status = resp.status_code().as_u16();
        let text = resp.text().await?;
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, body))
    }

    fn check(status: u16, body: &Value) -> OpenSearchResult<()> {
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(OpenSearchError::Status {
                status,
                body: body.to_string(),
            })
        }
    }

    /// Cluster health, for start-up logging and readiness checks.
    pub async fn health(&self) -> OpenSearchResult<Value> {
        let resp = self
            .client
            .cluster()
            .health(ClusterHealthParts::None)
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        Self::check(status, &body)?;
        Ok(body)
    }

    pub async fn index_exists(&self, index: &str) -> OpenSearchResult<bool> {
        let resp = self
            .client
            .indices()
            .exists(IndicesExistsParts::Index(&[index]))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        match status {
            200 => Ok(true),
            404 => Ok(false),
            _ => Err(OpenSearchError::Status {
                status,
                body: body.to_string(),
            }),
        }
    }

    /// The physical indices the alias currently points at (empty if the alias
    /// doesn't exist yet).
    pub async fn alias_targets(&self, alias: &str) -> OpenSearchResult<Vec<String>> {
        let resp = self
            .client
            .indices()
            .get_alias(IndicesGetAliasParts::Name(&[alias]))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        if status == 404 {
            return Ok(Vec::new());
        }
        Self::check(status, &body)?;
        Ok(body
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default())
    }

    /// Make sure this build's physical index for `def` exists, creating it with
    /// its mapping if not. If the read alias doesn't exist at all yet (a brand-new
    /// cluster), point it here too, so the very first version needs no swap.
    /// Returns whether the index was created.
    ///
    /// Safe to call on every start; never changes an existing index or alias.
    pub async fn ensure_index(&self, def: &IndexDef) -> OpenSearchResult<bool> {
        let index = def.physical(&self.prefix);
        let created = if self.index_exists(&index).await? {
            false
        } else {
            let resp = self
                .client
                .indices()
                .create(IndicesCreateParts::Index(&index))
                .body((def.body)())
                .send()
                .await?;
            let (status, body) = Self::read(resp).await?;
            // Another process may have created it between the check and the put.
            let already = status == 400
                && body
                    .to_string()
                    .contains("resource_already_exists_exception");
            if !already {
                Self::check(status, &body)?;
            }
            !already
        };
        let alias = def.alias(&self.prefix);
        if self.alias_targets(&alias).await?.is_empty() {
            self.point_alias(def).await?;
        }
        Ok(created)
    }

    /// Atomically point the read alias for `def` at this build's physical index,
    /// removing it from any other version.
    pub async fn point_alias(&self, def: &IndexDef) -> OpenSearchResult<()> {
        let alias = def.alias(&self.prefix);
        let index = def.physical(&self.prefix);
        let mut actions = Vec::new();
        for old in self.alias_targets(&alias).await? {
            if old != index {
                actions.push(json!({ "remove": { "index": old, "alias": alias } }));
            }
        }
        actions.push(json!({ "add": { "index": index, "alias": alias } }));
        let resp = self
            .client
            .indices()
            .update_aliases()
            .body(json!({ "actions": actions }))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        Self::check(status, &body)
    }

    /// Run a `_bulk` request. Fails if any item failed, except deletes of
    /// documents that were already gone.
    pub async fn bulk(&self, ops: &[BulkOp]) -> OpenSearchResult<()> {
        self.bulk_with(ops, false).await
    }

    /// [`Self::bulk`], optionally waiting (`refresh=wait_for`, up to the 1s
    /// refresh interval) until the writes are visible to search. The live sync
    /// waits so that a second change to the same match, a moment later, can see
    /// the first change's rows when it looks for stale ones.
    pub async fn bulk_with(&self, ops: &[BulkOp], wait_for_refresh: bool) -> OpenSearchResult<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut lines: Vec<JsonBody<Value>> = Vec::with_capacity(ops.len() * 2);
        for op in ops {
            match op {
                BulkOp::Index { index, id, doc } => {
                    lines.push(json!({ "index": { "_index": index, "_id": id } }).into());
                    lines.push(doc.clone().into());
                }
                BulkOp::Delete { index, id } => {
                    lines.push(json!({ "delete": { "_index": index, "_id": id } }).into());
                }
            }
        }
        let mut req = self.client.bulk(BulkParts::None).body(lines);
        if wait_for_refresh {
            req = req.refresh(Refresh::WaitFor);
        }
        let (status, body) = Self::read(req.send().await?).await?;
        Self::check(status, &body)?;
        if body["errors"].as_bool() == Some(true) {
            let failures: Vec<&Value> = body["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_object().and_then(|o| o.values().next()))
                .filter(|res| {
                    let s = res["status"].as_u64().unwrap_or(500);
                    // A delete of a missing doc reports 404: that's the outcome we wanted.
                    !(200..300).contains(&s) && s != 404
                })
                .collect();
            if !failures.is_empty() {
                return Err(OpenSearchError::Bulk {
                    failed: failures.len(),
                    first: failures[0].to_string(),
                });
            }
        }
        Ok(())
    }

    /// Ids of the documents in `index` matching `query` (a query DSL object), up
    /// to `max`. Used to find a match's stale player rows. Deliberately a plain
    /// search rather than `_delete_by_query`, which needs the reindex module and
    /// runs asynchronously; a match has tens of rows at most.
    pub async fn ids_matching(
        &self,
        index: &str,
        query: Value,
        max: usize,
    ) -> OpenSearchResult<Vec<String>> {
        let resp = self
            .client
            .search(SearchParts::Index(&[index]))
            .body(json!({ "query": query, "size": max, "_source": false }))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        if status == 404 {
            return Ok(Vec::new());
        }
        Self::check(status, &body)?;
        Ok(body["hits"]["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| h["_id"].as_str().map(str::to_string))
            .collect())
    }

    /// Run a raw `_search` against an index or alias. Used by the backfill's
    /// verification and by tests; no request path reads OpenSearch yet. Typed
    /// read queries will wrap this in this module.
    pub async fn search(&self, index: &str, body: Value) -> OpenSearchResult<Value> {
        let resp = self
            .client
            .search(SearchParts::Index(&[index]))
            .body(body)
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        Self::check(status, &body)?;
        Ok(body)
    }

    /// Make recent writes visible to search (tests and the backfill's final
    /// verification only; normal operation relies on the 1s refresh interval).
    pub async fn refresh(&self, index: &str) -> OpenSearchResult<()> {
        let resp = self
            .client
            .indices()
            .refresh(IndicesRefreshParts::Index(&[index]))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        Self::check(status, &body)
    }

    /// Delete an index (backfill clean-up of superseded versions).
    pub async fn delete_index(&self, index: &str) -> OpenSearchResult<()> {
        let resp = self
            .client
            .indices()
            .delete(IndicesDeleteParts::Index(&[index]))
            .send()
            .await?;
        let (status, body) = Self::read(resp).await?;
        if status == 404 {
            return Ok(());
        }
        Self::check(status, &body)
    }
}
