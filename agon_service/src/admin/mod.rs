//! Admin panel: `Api::require_admin` guards every `/admin/...` route; the
//! handlers themselves live in `main.rs`'s single `impl Api` block (same
//! convention as `team.rs`/`membership.rs`).
//!
//! Types live in [`models`], imported into `main.rs` as `admin_models` so a
//! call site reads `admin_models::User` rather than a bare `AdminUser` —
//! see that module's doc comment for why.

pub mod models;
