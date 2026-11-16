//! OpenSearch: the match and per-player stats store.
//!
//! DynamoDB stays the source of truth. OpenSearch holds documents derived from
//! it, written by `agon_worker` off the DynamoDB stream and rebuilt in full by
//! the `agon_worker backfill-opensearch` command. A write failure here never
//! fails a user-facing write — `matches`/`match_stats` are still shadow-only
//! (nothing reads them yet); `users` is the first index with a real reader
//! (`GET /admin/stats`), which reports 503 while `OPENSEARCH_URL` is unset,
//! same convention as the other optional integrations.
//!
//! Three indices (see [`schema`]):
//!
//! - **`matches`**: one document per match, with each side nested (team,
//!   roster, outcome, score, conceded). Team records, form and head-to-head
//!   come from here.
//! - **`match_stats`**: one flat row per player per confirmed match, carrying
//!   everything a leaderboard filters on (team, opponent, kind, tournament,
//!   year, sport) plus that player's counters. Leaderboards and every
//!   per-player slice come from here.
//! - **`users`**: one minimal document per user. Backs the admin panel's
//!   "total users" count (`GET /admin/stats`) — the first thing that reads
//!   OpenSearch on a request path. Not a user search index; `GET
//!   /admin/users` still searches Meilisearch.
//!
//! Each index is versioned. The worker writes to the physical index for the
//! version compiled into it (`matches_v1`); readers use the alias (`matches`),
//! which the backfill points at a version once that version is complete. A
//! schema change is: bump the version, deploy, run the backfill, which fills
//! the new index and swaps the alias.

pub mod client;
pub mod docs;
pub mod schema;
pub mod sync;

pub use client::{BulkOp, OpenSearchClient, OpenSearchError};
pub use docs::{MatchDocs, build_match_docs, build_user_doc};
pub use schema::{IndexDef, MATCH_STATS, MATCHES, USERS, all_indices};
pub use sync::{SyncError, SyncOutcome, UserSyncOutcome, sync_match, sync_user};
