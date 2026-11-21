//! OpenSearch: the match and per-player stats store.
//!
//! DynamoDB stays the source of truth. OpenSearch holds documents derived from
//! it, written by `agon_worker` off the DynamoDB stream and rebuilt in full by
//! the `agon_worker backfill-opensearch` command. A sync failure never fails
//! a user-facing write. The team summary and leaderboard endpoints read the
//! match data through [`team_stats`]; the admin panel's `GET /admin/stats`
//! and `GET /admin/users` read the `users` index through
//! [`client::OpenSearchClient::count`] and [`users::search_users`]. All
//! report 503 while `OPENSEARCH_URL` is unset, same convention as the other
//! optional integrations.
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
//! - **`users`**: one document per user (id, name, email, creation time,
//!   admin flag). Backs the admin panel's "total users" count and its
//!   `GET /admin/users` search/browse — the whole admin panel is now
//!   Meilisearch-free.
//!
//! Each index is versioned. The worker writes to the physical index for the
//! version compiled into it (`matches_v1`); readers use the alias (`matches`),
//! which the backfill points at a version once that version is complete. A
//! schema change is: bump the version, deploy, run the backfill, which fills
//! the new index and swaps the alias.
//!
//! Match/leaderboard reads go through [`team_stats`]; `users` reads go
//! through [`users`] (search/browse) and a direct `_count` (the total
//! figure), both querying the alias.

pub mod client;
pub mod docs;
pub mod schema;
pub mod sync;
pub mod team_stats;
pub mod users;

pub use client::{BulkOp, OpenSearchClient, OpenSearchError};
pub use docs::{MatchDocs, build_match_docs, build_user_doc};
pub use schema::{IndexDef, MATCH_STATS, MATCHES, USERS, all_indices};
pub use sync::{SyncError, SyncOutcome, UserSyncOutcome, sync_match, sync_user};
pub use users::search_users;
