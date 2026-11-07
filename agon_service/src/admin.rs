//! Request/response types for the admin panel. Handlers live in `main.rs`'s
//! single `impl Api` block (same convention as `team.rs`/`membership.rs`),
//! guarded by `Api::require_admin` on every route.
//!
//! Scoped to the `users` resource for now — matches/series/teams get their
//! own admin endpoints in follow-up PRs, reusing the same `require_admin`
//! guard and the same list/get/update shape.

use poem_openapi::Object;

/// A user row as seen in the admin panel — the full account record, not the
/// public-facing `UserProfile` (no `is_followed_by_me`, includes `email` and
/// `is_admin`).
#[derive(Object)]
pub struct AdminUser {
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
pub struct AdminUserPage {
    pub items: Vec<AdminUser>,
    pub next_cursor: Option<String>,
}

/// Editable fields on `PATCH /admin/users/{user_id}`. All optional — only
/// supplied fields change.
#[derive(Object)]
pub struct UpdateAdminUserInput {
    pub name: Option<String>,
    /// Grant (`true`) or revoke (`false`) admin-panel access.
    pub is_admin: Option<bool>,
}
