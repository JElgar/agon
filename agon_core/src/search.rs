//! Minimal Meilisearch client over its REST API.
//!
//! Shared by the worker (which keeps the indexes in sync via idempotent
//! upsert/delete off the DynamoDB stream) and the API (which queries the indexes
//! to serve discovery endpoints). We talk to Meilisearch directly with `reqwest`
//! rather than pulling in the official SDK: the surface we need is small —
//! upsert a document, delete one by id, and run a filtered search — so a thin
//! client keeps the dependency footprint (and version churn) down.
//!
//! Write operations are idempotent (a replayed upsert/delete has no visible
//! effect), which is exactly what the worker's at-least-once delivery needs.
//! Search returns only document ids: the API hydrates full entities from
//! DynamoDB, since the indexes store only what's needed to match and rank.

use serde::{Deserialize, Serialize};

use crate::error::SearchError;

pub type SearchResult<T> = Result<T, SearchError>;

/// The Meilisearch indexes maintained by the worker (see docs/async-design.md §7).
#[derive(Debug, Clone, Copy)]
pub enum Index {
    Users,
    Teams,
    Matches,
}

impl Index {
    fn name(self) -> &'static str {
        match self {
            Index::Users => "users",
            Index::Teams => "teams",
            Index::Matches => "matches",
        }
    }

    /// Every index the worker maintains, for bootstrap iteration.
    pub const ALL: [Index; 3] = [Index::Users, Index::Teams, Index::Matches];

    /// Attributes that must be declared *filterable* before they can be used in
    /// a search `filter` expression. Only the matches index is filtered (by
    /// sport / participant / date range in `GET /matches`).
    fn filterable_attributes(self) -> &'static [&'static str] {
        match self {
            Index::Matches => &["sport", "participant_ids", "starts_at", "status"],
            Index::Users | Index::Teams => &[],
        }
    }

    /// Attributes that must be declared *sortable* before they can be used in a
    /// search `sort`. Matches are sorted by start time (most recent first).
    fn sortable_attributes(self) -> &'static [&'static str] {
        match self {
            Index::Matches => &["starts_at"],
            Index::Users | Index::Teams => &[],
        }
    }
}

/// A page of matching document ids, with an offset to fetch the next page.
#[derive(Debug, Clone)]
pub struct SearchHits {
    /// The `id` of each matching document, in ranked order.
    pub ids: Vec<String>,
    /// Offset for the next page, or `None` if this was the last page.
    pub next_offset: Option<u32>,
}

/// Parameters for a search query.
#[derive(Debug, Default, Clone)]
pub struct SearchQuery {
    /// Free-text query. Empty matches everything (filtered browse).
    pub q: String,
    /// Meilisearch filter expression (e.g. `sport = tennis AND starts_at >= ...`).
    pub filter: Option<String>,
    /// Sort expressions (e.g. `["starts_at:desc"]`).
    pub sort: Vec<String>,
    /// Zero-based offset into the result set.
    pub offset: u32,
    /// Max hits to return.
    pub limit: u32,
}

/// A thin Meilisearch REST client. Cheap to clone (wraps an `Arc` internally).
#[derive(Clone)]
pub struct SearchClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

/// Only the `id` field is read back from search hits; everything else is
/// hydrated from DynamoDB, so documents must carry a string `id`.
#[derive(Deserialize)]
struct IdOnly {
    id: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<IdOnly>,
    #[serde(rename = "estimatedTotalHits")]
    estimated_total_hits: u32,
}

impl SearchClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            // Trim a trailing slash so URL joins are predictable.
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    /// Upsert one document into an index. Meilisearch replaces any existing
    /// document with the same primary key (`id`), so this is idempotent.
    pub async fn upsert<T: Serialize>(&self, index: Index, doc: &T) -> SearchResult<()> {
        let url = format!("{}/indexes/{}/documents", self.base_url, index.name());
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&[doc])
            .send()
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Self::check(resp, "upsert").await
    }

    /// Delete one document by id from an index. Deleting a missing document is a
    /// no-op in Meilisearch, so this is idempotent.
    pub async fn delete(&self, index: Index, id: &str) -> SearchResult<()> {
        let url = format!(
            "{}/indexes/{}/documents/{}",
            self.base_url,
            index.name(),
            id
        );
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Self::check(resp, "delete").await
    }

    /// Run a search against an index, returning ranked document ids and a
    /// next-page offset. The caller hydrates full entities from DynamoDB.
    pub async fn search(&self, index: Index, query: &SearchQuery) -> SearchResult<SearchHits> {
        let url = format!("{}/indexes/{}/search", self.base_url, index.name());

        let mut body = serde_json::json!({
            "q": query.q,
            "offset": query.offset,
            "limit": query.limit,
            // Only the primary key comes back; entities are hydrated from Dynamo.
            "attributesToRetrieve": ["id"],
        });
        if let Some(filter) = &query.filter {
            body["filter"] = serde_json::json!(filter);
        }
        if !query.sort.is_empty() {
            body["sort"] = serde_json::json!(query.sort);
        }

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| SearchError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(SearchError(format!("search returned {status}: {text}")));
        }

        let parsed: SearchResponse = resp
            .json()
            .await
            .map_err(|e| SearchError(format!("bad search response: {e}")))?;

        let ids: Vec<String> = parsed.hits.into_iter().map(|h| h.id).collect();
        // If this page reached the estimated total, there is no next page.
        let consumed = query.offset.saturating_add(ids.len() as u32);
        let next_offset = (consumed < parsed.estimated_total_hits).then_some(consumed);

        Ok(SearchHits { ids, next_offset })
    }

    /// Configure one index's settings (creating the index if absent) so its
    /// filterable / sortable attributes match what the API queries. Meilisearch
    /// treats the settings update as idempotent — re-applying the same settings
    /// is a no-op — so this is safe to run on every worker start.
    ///
    /// Uses `id` as the primary key to match how documents are upserted.
    pub async fn configure_index(&self, index: Index) -> SearchResult<()> {
        let url = format!("{}/indexes/{}/settings", self.base_url, index.name());
        let body = serde_json::json!({
            "filterableAttributes": index.filterable_attributes(),
            "sortableAttributes": index.sortable_attributes(),
        });
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Self::check(resp, "configure_index").await
    }

    /// Configure every index. Called once on worker startup so a fresh
    /// Meilisearch instance has the filterable / sortable attributes declared
    /// before any documents are indexed or queried.
    pub async fn bootstrap(&self) -> SearchResult<()> {
        for index in Index::ALL {
            self.configure_index(index).await?;
        }
        Ok(())
    }

    async fn check(resp: reqwest::Response, op: &str) -> SearchResult<()> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(SearchError(format!("{op} returned {status}: {body}")))
    }
}
