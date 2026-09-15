//! Per-sport `SportApi` implementations. One module per sport, holding the
//! API↔DAO mapping and (eventually) live-scoring fold for that sport — see
//! `crate::sport::SportApi`'s doc comment.
//!
//! Only sports migrated onto the trait live here; the rest (cricket,
//! football) still have their conversion functions inline in `mapping.rs`
//! until they're migrated too.

pub mod netball;
