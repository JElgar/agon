//! Per-sport `SportRecord` implementations. One module per sport, holding
//! everything a DAO-only context needs for that sport — see
//! `agon_core::sport::SportRecord`'s doc comment.
//!
//! Only sports migrated onto the trait live here; the rest (cricket,
//! football) still have their derived-stats logic inline in
//! `agon_worker::handlers::stats` until they're migrated too.

pub mod netball;
