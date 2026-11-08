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
//! Scoped to the `users` resource for now — matches/series/teams get their
//! own admin endpoints (and their own `models` types) in follow-up PRs.

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
