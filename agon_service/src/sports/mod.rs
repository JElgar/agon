//! Per-sport API↔DAO mapping. One module per sport, holding the score/
//! format/live-event conversions between the API's poem-openapi types and
//! the DAO's plain-serde records — see `crate::sports::netball`'s doc
//! comment for why these are plain functions rather than a shared trait.

pub mod cricket;
pub mod football;
pub mod netball;
