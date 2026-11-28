//! OpenSearch: the match and per-player stats store.
//!
//! DynamoDB stays the source of truth. OpenSearch holds documents derived from
//! it, written by `agon_worker` off the DynamoDB stream and rebuilt in full by
//! the `agon_worker backfill-opensearch` command. A sync failure never fails
//! a user-facing write. The team summary, leaderboard and head-to-head
//! endpoints read the match data through [`team_stats`]; a player's own
//! summary reads it through [`user_stats`]; the admin panel's
//! `GET /admin/stats`,
//! `GET /admin/users` and `GET /admin/matches` read the `users`/`matches`
//! indices through [`client::OpenSearchClient::count`],
//! [`users::search_users`] and [`matches::search_matches`]. OpenSearch is required: both binaries
//! refuse to start while `OPENSEARCH_URL` is unset.
//!
//! Four indices (see [`schema`]):
//!
//! - **`matches`**: one document per match, with each side nested (team,
//!   roster, outcome, score, conceded). Team records, form, head-to-head and
//!   next-match lookups come from here.
//! - **`match_stats`**: one flat row per player per confirmed match, carrying
//!   everything a leaderboard filters on (team, opponent, kind, tournament,
//!   year, sport) plus that player's counters. Leaderboards, a player's own
//!   record/form, and every other per-player slice come from here.
//! - **`users`**: one document per user (id, name, email, creation time,
//!   admin flag). Backs the admin panel's "total users" count,
//!   `GET /admin/users` and `GET /users/search`.
//! - **`teams`**: one document per searchable team (id, name, creation time;
//!   tournament squads are never indexed). Backs `GET /teams/search`.
//!
//! Each index is versioned. The worker writes to the physical index for the
//! version compiled into it (`matches_v1`); readers use the alias (`matches`),
//! which the backfill points at a version once that version is complete. A
//! schema change is: bump the version, deploy, run the backfill, which fills
//! the new index and swaps the alias.
//!
//! Match/leaderboard reads go through [`team_stats`]; a player's own summary
//! through [`user_stats`]; `users` reads go through [`users`] (search/browse)
//! and a direct `_count` (the total figure) — all querying the alias.

pub mod backfill;
pub mod backfill_workflow;
pub mod client;
pub mod docs;
pub mod matches;
pub mod schema;
pub mod sync;
pub mod team_stats;
pub mod teams;
pub mod text;
pub mod user_stats;
pub mod users;

pub use client::{BulkOp, OpenSearchClient, OpenSearchError};
pub use docs::{MatchDocs, build_match_docs, build_team_doc, build_user_doc};
pub use matches::{MatchFilters, search_matches};
pub use schema::{IndexDef, MATCH_STATS, MATCHES, TEAMS, USERS, all_indices};
pub use sync::{
    SyncError, SyncOutcome, TeamSyncOutcome, UserSyncOutcome, sync_match, sync_team, sync_user,
};
pub use teams::search_teams;
pub use users::{UserSearchFields, search_users};
