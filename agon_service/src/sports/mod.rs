//! Per-sport API-side surface. One module per sport, holding *everything*
//! specific to that sport on the API side: its detailed-score/live-event/
//! format poem-openapi types (formerly split across `detailed_score`/
//! `live_score`/`match_format`), the event-log fold that computes a score
//! from a live event log (`XScore::from_events`/`apply_event`), the small
//! per-score-shape helpers `main.rs`'s generic `Score` dispatchers call into
//! (`side_ids`/`resolve_ids`/`set_players`/`player_ids`/`winner`/
//! `apply_new_events`), and the score/format/live-event conversions between
//! those API types and the DAO's plain-serde records. Plain functions with
//! conventional names, not a shared trait — see `crate::sports::netball`'s
//! doc comment for why.
//!
//! The result: adding a fully-modeled sport (or extending an existing one)
//! touches exactly one file in this module plus one file in
//! `agon_core::sports` (its DAO record types and `SportRecord` impl) — not
//! a scatter across `main.rs`, `mapping.rs`, `match_format.rs`,
//! `detailed_score`, and `live_score` the way it used to.
//!
//! `agon_sports!` below is the single source of truth for which sports are
//! fully modeled — the x-macro every generated enum (`Score`/`MatchType` in
//! `main.rs`, `MatchFormat` in `match_format.rs`, `LiveEventInput` in
//! `live_score.rs`) is generated from. Adding a sport means: write its DAO
//! record types (`agon_core::dao::records`) and `SportRecord` impl
//! (`agon_core::sports`), write a new module here with its API types, event
//! fold, dispatch helpers, and `score_to_record`/`score_from_record`/
//! `format_to_record`/`format_from_record`/`live_event_to_record`/
//! `live_event_from_record`/`from_events`/`apply_new_events`/`side_ids`/
//! `resolve_ids`/`set_players`/`player_ids`/`winner` functions (see
//! `netball.rs` for the shape each one needs), then add one line to the
//! `agon_sports!` invocations below. Nothing else changes.
//!
//! Each generated enum stays defined in its own file (`main.rs`,
//! `match_format.rs`, `live_score.rs`) rather than centralized here, so
//! existing call sites (`crate::Score`, `crate::match_format::MatchFormat`,
//! `crate::live_score::LiveEventInput`, ...) don't need to change paths —
//! `agon_sports!` is invoked once per file, each time with a different
//! "template" macro (defined right next to that invocation) that says what
//! to build from the list for that file.
//!
//! `agon_core` has its own independent counterpart (`define_sport_records!`
//! in `agon_core::dao::records`) for the DAO-side `ScoreRecord`/
//! `MatchFormatRecord`/`LiveEventPayloadRecord` enums — see that macro's doc
//! comment for why the sport list can't be shared across the crate
//! boundary. Keep the two lists in sync by hand; nothing enforces it
//! automatically.
//!
//! Tennis/badminton/squash/table-tennis are deliberately *not* here: they're
//! the generic, unmodeled sports (`Score::Sets`, no dedicated format/
//! live-event/stats), and stay as fixed arms hand-written alongside each
//! macro invocation. Making one of them fully modeled the way netball is
//! would mean giving it the same treatment as any new sport — a module here
//! plus a line in each `agon_sports!` invocation — not touching this macro.
#[macro_export]
macro_rules! agon_sports {
    ($callback:ident) => {
        $callback! {
            Football { tag: "football", module: football, score: FootballScore, format: FootballFormat, live_event: FootballLiveEvent },
            Cricket { tag: "cricket", module: cricket, score: CricketScore, format: CricketFormat, live_event: CricketLiveEvent },
            Netball { tag: "netball", module: netball, score: NetballScore, format: NetballFormat, live_event: NetballLiveEvent },
        }
    };
}

pub mod cricket;
pub mod football;
pub mod netball;
