//! Per-sport `SportRecord` implementations. One module per sport, holding
//! everything a DAO-only context needs for that sport — see
//! `agon_core::sport::SportRecord`'s doc comment.
//!
//! Only sports migrated onto the trait live here; the rest (cricket) still
//! has its derived-stats logic inline in `agon_worker::handlers::stats`
//! until it's migrated too.

pub mod football;
pub mod netball;
