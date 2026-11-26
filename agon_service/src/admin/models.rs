//! Types for the admin panel. Named plainly (`User`, not `AdminUser`) since
//! that bare name reads as "a user who is an admin", when it's really just
//! an account record modeled for the admin panel — any account can have one,
//! `is_admin` or not. Always referenced qualified, as `admin_models::User`
//! (main.rs imports this module `as admin_models`), never pulled in bare —
//! bare `User` would also collide with the unrelated `/users/me` response
//! type of the same name in `main.rs`.
//!
//! `#[oai(rename = ...)]` keeps the OpenAPI schema names (`AdminUser`, etc)
//! as they were — this rename is Rust-side only, so the wire contract and
//! generated clients are unaffected.
//!
//! Covers the `users` and `matches` resources — series/teams get their own
//! admin endpoints (and their own `models` types) in follow-up PRs.

use poem_openapi::Object;

/// A user row as seen in the admin panel — the full account record, not the
/// public-facing `UserProfile` (no `is_followed_by_me`, includes `email` and
/// `is_admin`).
#[derive(Object)]
#[oai(rename = "AdminUser")]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
    pub follower_count: u32,
    pub following_count: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One page of `GET /admin/users`. `next_cursor` absent => end — same
/// offset-cursor convention as every other search-backed list endpoint.
#[derive(Object)]
#[oai(rename = "AdminUserPage")]
pub struct UserPage {
    pub items: Vec<User>,
    pub next_cursor: Option<String>,
}

/// Editable fields on `PATCH /admin/users/{user_id}`. All optional — only
/// supplied fields change.
#[derive(Object)]
#[oai(rename = "UpdateAdminUserInput")]
pub struct UpdateUserInput {
    pub name: Option<String>,
    /// Grant (`true`) or revoke (`false`) admin-panel access.
    pub is_admin: Option<bool>,
}

/// A match row as seen in the admin panel's list — just what that list
/// shows (name, sport, status, kick-off); the full match detail is a click
/// through to the match's own page, not duplicated here.
#[derive(Object)]
#[oai(rename = "AdminMatch")]
pub struct Match {
    pub id: String,
    pub name: String,
    pub sport: String,
    pub status: String,
    pub starts_at: chrono::DateTime<chrono::Utc>,
}

/// One page of `GET /admin/matches`. Same offset-cursor convention as
/// [`UserPage`].
#[derive(Object)]
#[oai(rename = "AdminMatchPage")]
pub struct MatchPage {
    pub items: Vec<Match>,
    pub next_cursor: Option<String>,
}

/// Usage aggregates shown at the top of the admin panel's resource pages.
/// One field per resource the panel covers — grows as series/teams get
/// their own admin pages.
#[derive(Object)]
#[oai(rename = "AdminStats")]
pub struct Stats {
    pub total_users: u64,
    pub total_matches: u64,
}

/// Body of `POST /admin/opensearch/backfill`.
#[derive(Object)]
#[oai(rename = "StartAdminBackfillInput")]
pub struct StartBackfillInput {
    /// Delete the superseded index version once the alias swap completes —
    /// same as the CLI's `--delete-old`. Defaults to `false` (keep it, in
    /// case a rollback is needed).
    pub delete_old: Option<bool>,
}

/// The OpenSearch backfill job this `agon-service` process last ran (or is
/// running), as reported by `GET`/`POST /admin/opensearch/backfill`. Absent
/// `started_at` means nothing has been triggered on this process yet.
#[derive(Object)]
#[oai(rename = "AdminBackfillStatus")]
pub struct BackfillStatus {
    /// `"idle"`, `"running"`, `"succeeded"` or `"failed"`.
    pub status: String,
    pub delete_old: Option<bool>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub match_pages: u64,
    pub matches_indexed: u64,
    pub matches_removed: u64,
    pub matches_failed: u64,
    pub user_pages: u64,
    pub users_indexed: u64,
    pub users_removed: u64,
    pub users_failed: u64,
    pub error: Option<String>,
}
