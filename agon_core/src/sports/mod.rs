//! Per-sport DAO record types and `SportRecord` implementations. One module
//! per sport, holding everything a DAO-only context needs for that sport —
//! its `ScoreRecord`/`FormatRecord`/`LiveEventRecord` type tree (assembled
//! into `dao::records::ScoreRecord`/`MatchFormatRecord`/
//! `LiveEventPayloadRecord` by `define_sport_records!`) plus the
//! `SportRecord` impl — see `agon_core::sport::SportRecord`'s doc comment.

pub mod cricket;
pub mod football;
pub mod netball;
