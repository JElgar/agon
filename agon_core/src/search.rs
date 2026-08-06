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
            // `starts_at_ts` (numeric epoch) — not `starts_at` (ISO string) —
            // because Meilisearch range filters are numeric-only.
            Index::Matches => &["sport", "participant_ids", "starts_at_ts", "status"],
            Index::Users | Index::Teams => &[],
        }
    }

    /// Attributes that must be declared *sortable* before they can be used in a
    /// search `sort`. Matches are sorted by start time (most recent first).
    fn sortable_attributes(self) -> &'static [&'static str] {
        match self {
            // Numeric epoch: sorting an ISO string works lexically but the same
            // field must be numeric for range filters, so use one numeric field.
            Index::Matches => &["starts_at_ts"],
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

/// A queried participant's result in a match, resolved from the match search
/// document's outcome buckets (see [`search_matches`](SearchClient::search_matches)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    Won,
    Lost,
    Draw,
}

/// One match search hit, with the outcome of whichever participant the
/// caller asked about (see [`SearchClient::search_matches`]).
#[derive(Debug, Clone)]
pub struct MatchSearchHit {
    pub id: String,
    /// `None` when the search wasn't scoped to a participant, or when it was
    /// but the match has no confirmed result yet — not a draw, just unknown.
    pub outcome: Option<MatchOutcome>,
}

/// A page of match search hits, with an offset to fetch the next page.
#[derive(Debug, Clone)]
pub struct MatchSearchHits {
    pub items: Vec<MatchSearchHit>,
    pub next_offset: Option<u32>,
}

/// A Meilisearch client wrapping the official SDK. Cheap to clone (the SDK's
/// `Client` wraps its `reqwest::Client` internally).
#[derive(Clone)]
pub struct SearchClient {
    client: Client,
}

/// Only the `id` field is read back from search hits; everything else is
/// hydrated from DynamoDB, so documents must carry a string `id`.
#[derive(Deserialize)]
struct IdOnly {
    id: String,
}

/// What [`SearchClient::search_matches`] retrieves from a match hit — enough
/// to resolve a participant's outcome without a second lookup.
#[derive(Deserialize)]
struct MatchOutcomeDoc {
    id: String,
    #[serde(default)]
    winning_participant_ids: Vec<String>,
    #[serde(default)]
    losing_participant_ids: Vec<String>,
    #[serde(default)]
    drawing_participant_ids: Vec<String>,
}

impl SearchClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        // Trim a trailing slash so URL joins are predictable.
        let base_url = base_url.into().trim_end_matches('/').to_string();
        // Only fails if the api key can't be encoded as an HTTP header value,
        // which never happens for the keys this service is configured with.
        let client = Client::new(base_url, Some(api_key.into()))
            .expect("invalid meilisearch client configuration");
        Self { client }
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

    /// Search the matches index, resolving each hit's outcome for
    /// `participant_id` (if given) from the document's outcome buckets —
    /// `winning_participant_ids`/`losing_participant_ids`/
    /// `drawing_participant_ids` (see `agon_worker`'s `MatchDoc`) — instead of
    /// a second lookup. Free: those fields ride along on the same hit a
    /// `participant` filter already retrieves.
    ///
    /// `participant_id` only makes sense paired with a `query.filter` that
    /// already scopes results to that participant (e.g. `participant_ids =
    /// "<id>"`) — this method doesn't add that filter itself, callers already
    /// building one for the `participant` discovery param should reuse it as
    /// the outcome subject.
    pub async fn search_matches(
        &self,
        query: &SearchQuery,
        participant_id: Option<&str>,
    ) -> SearchResult<MatchSearchHits> {
        let idx = self.client.index(Index::Matches.name());
        let mut builder = idx.search();
        builder
            .with_query(&query.q)
            .with_offset(query.offset as usize)
            .with_limit(query.limit as usize)
            .with_attributes_to_retrieve(Selectors::Some(&[
                "id",
                "winning_participant_ids",
                "losing_participant_ids",
                "drawing_participant_ids",
            ]));
        if let Some(filter) = &query.filter {
            builder.with_filter(filter);
        }
        let sort_refs: Vec<&str> = query.sort.iter().map(String::as_str).collect();
        if !sort_refs.is_empty() {
            builder.with_sort(&sort_refs);
        }

        let results = builder
            .execute::<MatchOutcomeDoc>()
            .await
            .map_err(|e| SearchError(e.to_string()))?;

        let items: Vec<MatchSearchHit> = results
            .hits
            .into_iter()
            .map(|h| {
                let doc = h.result;
                let outcome = participant_id.and_then(|p| {
                    if doc.winning_participant_ids.iter().any(|id| id == p) {
                        Some(MatchOutcome::Won)
                    } else if doc.losing_participant_ids.iter().any(|id| id == p) {
                        Some(MatchOutcome::Lost)
                    } else if doc.drawing_participant_ids.iter().any(|id| id == p) {
                        Some(MatchOutcome::Draw)
                    } else {
                        None
                    }
                });
                MatchSearchHit {
                    id: doc.id,
                    outcome,
                }
            })
            .collect();

        let consumed = query.offset.saturating_add(items.len() as u32);
        let total = results.estimated_total_hits.unwrap_or(consumed as usize) as u32;
        let next_offset = (consumed < total).then_some(consumed);

        Ok(MatchSearchHits { items, next_offset })
    }

    /// Configure one index's settings (creating the index if absent) so its
    /// filterable / sortable attributes match what the API queries. Meilisearch
    /// treats the settings update as idempotent — re-applying the same settings
    /// is a no-op — so this is safe to run on every worker start.
    ///
    /// Uses `id` as the primary key implicitly: documents are always upserted
    /// with `id` as the explicit primary key (see `upsert`).
    pub async fn configure_index(&self, index: Index) -> SearchResult<()> {
        let settings = Settings::new()
            .with_filterable_attributes(index.filterable_attributes().iter().copied())
            .with_sortable_attributes(index.sortable_attributes().iter().copied());
        self.client
            .index(index.name())
            .set_settings(&settings)
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
