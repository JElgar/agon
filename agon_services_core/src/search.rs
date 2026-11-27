//! Meilisearch client, built on the official `meilisearch-sdk` crate.
//!
//! Shared by the worker (which keeps the indexes in sync via idempotent
//! upsert/delete off the DynamoDB stream) and the API (which queries the indexes
//! to serve discovery endpoints). This module exposes only the slice of the SDK
//! we actually use — upsert a document, delete one by id, run a filtered
//! search, and declare index settings — so callers don't reach into the SDK
//! directly.
//!
//! Write operations are idempotent (a replayed upsert/delete has no visible
//! effect), which is exactly what the worker's at-least-once delivery needs.
//! Search returns only document ids: the API hydrates full entities from
//! DynamoDB, since the indexes store only what's needed to match and rank.

use meilisearch_sdk::client::Client;
use meilisearch_sdk::search::Selectors;
use meilisearch_sdk::settings::Settings;
use serde::{Deserialize, Serialize};

use crate::error::SearchError;

pub type SearchResult<T> = Result<T, SearchError>;

/// The Meilisearch indexes maintained by the worker (see docs/async-design.md §7).
#[derive(Debug, Clone, Copy)]
pub enum Index {
    Teams,
}

impl Index {
    fn name(self) -> &'static str {
        match self {
            Index::Teams => "teams",
        }
    }

    /// Every index the worker maintains, for bootstrap iteration.
    pub const ALL: [Index; 1] = [Index::Teams];
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

/// A Meilisearch client wrapping the official SDK. Cheap to clone (the SDK's
/// `Client` wraps its `reqwest::Client` internally).
#[derive(Clone)]
pub struct SearchClient {
    client: Client,
    // Kept alongside `client` so `count` below can issue its own request
    // instead of going through the SDK's `get_stats` — see that method's
    // doc comment for why.
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

/// Only the `id` field is read back from search hits; everything else is
/// hydrated from DynamoDB, so documents must carry a string `id`.
#[derive(Deserialize)]
struct IdOnly {
    id: String,
}

impl SearchClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        // Trim a trailing slash so URL joins are predictable.
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let api_key = api_key.into();
        // Only fails if the api key can't be encoded as an HTTP header value,
        // which never happens for the keys this service is configured with.
        let client = Client::new(base_url.clone(), Some(api_key.clone()))
            .expect("invalid meilisearch client configuration");
        Self {
            client,
            base_url,
            api_key,
            http: reqwest::Client::new(),
        }
    }

    /// Upsert one document into an index. Meilisearch replaces any existing
    /// document with the same primary key (`id`), so this is idempotent.
    pub async fn upsert<T: Serialize + Send + Sync>(
        &self,
        index: Index,
        doc: &T,
    ) -> SearchResult<()> {
        self.client
            .index(index.name())
            .add_documents(std::slice::from_ref(doc), Some("id"))
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Ok(())
    }

    /// Delete one document by id from an index. Deleting a missing document is a
    /// no-op in Meilisearch, so this is idempotent.
    pub async fn delete(&self, index: Index, id: &str) -> SearchResult<()> {
        self.client
            .index(index.name())
            .delete_document(id)
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Ok(())
    }

    /// Exact count of documents in an index (Meilisearch's own stats, not an
    /// estimate) — e.g. for an admin-panel "total users" figure.
    ///
    /// Issues the stats request directly rather than through the SDK's
    /// `Index::get_stats`, which deserializes into a struct that also
    /// requires several newer, embedding-related fields
    /// (`numberOfEmbeddedDocuments` etc) added in later Meilisearch releases.
    /// Meilisearch's own compatibility note ("guarantees compatibility with
    /// v1.x... but some features may not be present") means an older v1.x
    /// server's `/indexes/:uid/stats` response can omit those fields, which
    /// fails that struct's deserialization outright — losing the one field
    /// (`numberOfDocuments`) this actually needs. Decoding only that field
    /// here is forward- and backward-compatible with the stats shape across
    /// any v1.x server.
    pub async fn count(&self, index: Index) -> SearchResult<u64> {
        #[derive(Deserialize)]
        struct DocumentCount {
            #[serde(rename = "numberOfDocuments")]
            number_of_documents: u64,
        }

        let url = format!("{}/indexes/{}/stats", self.base_url, index.name());
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| SearchError(e.to_string()))?
            .error_for_status()
            .map_err(|e| SearchError(e.to_string()))?;
        let stats: DocumentCount = response
            .json()
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Ok(stats.number_of_documents)
    }

    /// Run a search against an index, returning ranked document ids and a
    /// next-page offset. The caller hydrates full entities from DynamoDB.
    pub async fn search(&self, index: Index, query: &SearchQuery) -> SearchResult<SearchHits> {
        let idx = self.client.index(index.name());
        let mut builder = idx.search();
        builder
            .with_query(&query.q)
            .with_offset(query.offset as usize)
            .with_limit(query.limit as usize)
            // Only the primary key comes back; entities are hydrated from Dynamo.
            .with_attributes_to_retrieve(Selectors::Some(&["id"]));
        if let Some(filter) = &query.filter {
            builder.with_filter(filter);
        }
        let sort_refs: Vec<&str> = query.sort.iter().map(String::as_str).collect();
        if !sort_refs.is_empty() {
            builder.with_sort(&sort_refs);
        }

        let results = builder
            .execute::<IdOnly>()
            .await
            .map_err(|e| SearchError(e.to_string()))?;

        let ids: Vec<String> = results.hits.into_iter().map(|h| h.result.id).collect();
        // If this page reached the estimated total, there is no next page.
        let consumed = query.offset.saturating_add(ids.len() as u32);
        let total = results.estimated_total_hits.unwrap_or(consumed as usize) as u32;
        let next_offset = (consumed < total).then_some(consumed);

        Ok(SearchHits { ids, next_offset })
    }

    /// Configure one index's settings (creating the index if absent). Meilisearch
    /// treats the settings update as idempotent — re-applying the same settings
    /// is a no-op — so this is safe to run on every worker start.
    ///
    /// Uses `id` as the primary key implicitly: documents are always upserted
    /// with `id` as the explicit primary key (see `upsert`).
    pub async fn configure_index(&self, index: Index) -> SearchResult<()> {
        self.client
            .index(index.name())
            .set_settings(&Settings::new())
            .await
            .map_err(|e| SearchError(e.to_string()))?;
        Ok(())
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
}
