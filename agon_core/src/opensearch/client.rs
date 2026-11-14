//! A small OpenSearch HTTP client: just the calls the worker and the backfill
//! need (create index, point alias, bulk write, delete by query, search).
//!
//! Plain `reqwest` + JSON rather than the `opensearch` crate: the surface we use
//! is tiny and stable, and this keeps the dependency tree unchanged (`reqwest`
//! is already in the workspace).

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::{Value, json};

use super::schema::IndexDef;

#[derive(Debug, thiserror::Error)]
pub enum OpenSearchError {
    #[error("opensearch request failed: {0}")]
    Http(#[from] reqwest::Error),
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
    http: reqwest::Client,
    base_url: String,
    auth: Option<(String, String)>,
    prefix: String,
}

impl OpenSearchClient {
    pub fn new(
        base_url: impl Into<String>,
        auth: Option<(String, String)>,
        prefix: impl Into<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds with static config");
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth,
            prefix: prefix.into(),
        }
    }

    /// Build from the environment, or `None` when `OPENSEARCH_URL` is unset
    /// (OpenSearch is opt-in, like push notifications and telemetry).
    ///
    /// - `OPENSEARCH_URL`: e.g. `http://opensearch:9200`
    /// - `OPENSEARCH_USERNAME` / `OPENSEARCH_PASSWORD`: optional basic auth
    /// - `OPENSEARCH_INDEX_PREFIX`: optional, prepended to every index name
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("OPENSEARCH_URL")
            .ok()
            .filter(|u| !u.is_empty())?;
        let auth = match (
            std::env::var("OPENSEARCH_USERNAME"),
            std::env::var("OPENSEARCH_PASSWORD"),
        ) {
            (Ok(u), Ok(p)) if !u.is_empty() => Some((u, p)),
            _ => None,
        };
        let prefix = std::env::var("OPENSEARCH_INDEX_PREFIX").unwrap_or_default();
        Some(Self::new(url, auth, prefix))
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The physical index this build writes to for `def`.
    pub fn write_index(&self, def: &IndexDef) -> String {
        def.physical(&self.prefix)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(
            method,
            format!("{}/{}", self.base_url, path.trim_start_matches('/')),
        );
        match &self.auth {
            Some((u, p)) => req.basic_auth(u, Some(p)),
            None => req,
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> OpenSearchResult<(StatusCode, Value)> {
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, body))
    }

    fn check(status: StatusCode, body: &Value) -> OpenSearchResult<()> {
        if status.is_success() {
            Ok(())
        } else {
            Err(OpenSearchError::Status {
                status: status.as_u16(),
                body: body.to_string(),
            })
        }
    }

    /// Cluster health, for start-up logging and readiness checks.
    pub async fn health(&self) -> OpenSearchResult<Value> {
        let (status, body) = self
            .send(self.request(Method::GET, "_cluster/health"))
            .await?;
        Self::check(status, &body)?;
        Ok(body)
    }

    pub async fn index_exists(&self, index: &str) -> OpenSearchResult<bool> {
        let (status, body) = self.send(self.request(Method::HEAD, index)).await?;
        match status {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(OpenSearchError::Status {
                status: status.as_u16(),
                body: body.to_string(),
            }),
        }
    }

    /// The physical indices the alias currently points at (empty if the alias
    /// doesn't exist yet).
    pub async fn alias_targets(&self, alias: &str) -> OpenSearchResult<Vec<String>> {
        let (status, body) = self
            .send(self.request(Method::GET, &format!("_alias/{alias}")))
            .await?;
        if status == StatusCode::NOT_FOUND {
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
            let (status, body) = self
                .send(self.request(Method::PUT, &index).json(&(def.body)()))
                .await?;
            // Another process may have created it between the check and the put.
            let already = status == StatusCode::BAD_REQUEST
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
        let (status, body) = self
            .send(
                self.request(Method::POST, "_aliases")
                    .json(&json!({ "actions": actions })),
            )
            .await?;
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
        let mut ndjson = String::new();
        for op in ops {
            match op {
                BulkOp::Index { index, id, doc } => {
                    ndjson
                        .push_str(&json!({ "index": { "_index": index, "_id": id } }).to_string());
                    ndjson.push('\n');
                    ndjson.push_str(&doc.to_string());
                    ndjson.push('\n');
                }
                BulkOp::Delete { index, id } => {
                    ndjson
                        .push_str(&json!({ "delete": { "_index": index, "_id": id } }).to_string());
                    ndjson.push('\n');
                }
            }
        }
        let (status, body) = self
            .send(
                self.request(
                    Method::POST,
                    if wait_for_refresh {
                        "_bulk?refresh=wait_for"
                    } else {
                        "_bulk"
                    },
                )
                .header("Content-Type", "application/x-ndjson")
                .body(ndjson),
            )
            .await?;
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
        let (status, body) = self
            .send(
                self.request(Method::POST, &format!("{index}/_search"))
                    .json(&json!({ "query": query, "size": max, "_source": false })),
            )
            .await?;
        if status == StatusCode::NOT_FOUND {
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
    /// verification and by tests; no request path reads OpenSearch yet.
    pub async fn search(&self, index: &str, body: Value) -> OpenSearchResult<Value> {
        let (status, resp) = self
            .send(
                self.request(Method::POST, &format!("{index}/_search"))
                    .json(&body),
            )
            .await?;
        Self::check(status, &resp)?;
        Ok(resp)
    }

    /// Make recent writes visible to search (tests and the backfill's final
    /// verification only; normal operation relies on the 1s refresh interval).
    pub async fn refresh(&self, index: &str) -> OpenSearchResult<()> {
        let (status, body) = self
            .send(self.request(Method::POST, &format!("{index}/_refresh")))
            .await?;
        Self::check(status, &body)
    }

    /// Delete an index (backfill clean-up of superseded versions).
    pub async fn delete_index(&self, index: &str) -> OpenSearchResult<()> {
        let (status, body) = self.send(self.request(Method::DELETE, index)).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Self::check(status, &body)
    }
}
