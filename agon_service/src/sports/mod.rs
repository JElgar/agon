//! Per-sport API-side surface. One module per fully-modeled sport, holding
//! *everything* specific to that sport on the API side: its score/format/
//! live-event/stats poem-openapi types, the event-log fold that computes a
//! score from a live event log (`XScore::from_events`/`apply_event`), the
//! per-score-shape helpers the generic `Score` dispatchers call into
//! (`side_ids`/`resolve_ids`/`set_players`/`player_ids`/`winner`/
//! `apply_new_events`), and the conversions between those API types and the
//! DAO's records (`score_to_record`/`score_from_record`/`format_to_record`/
//! `format_from_record`/`live_event_to_record`/`live_event_from_record`/
//! `from_events`/`stats_from_record`). Plain functions with conventional
//! names, not a shared trait — see `crate::sports::netball`'s doc comment for
//! why. `netball.rs` is the smallest complete example of the full set.
//!
//! `agon_sports!` below is the single source of truth for which sports those
//! are. Every sport-shaped aggregate and dispatcher on this side is generated
//! from it: `Score`/`MatchType`/`UserStats` (`main.rs`), `MatchFormat`
//! (`match_format.rs`), `LiveEventInput` (`live_score`), and every
//! conversion/dispatch function in `mapping.rs` — so no other file names a
//! specific sport.
//!
//! Adding a fully-modeled sport is therefore:
//! 1. `agon_core::sports::<sport>` — its DAO record types (score/format/
//!    live-event/stats) and `SportRecord` impl, plus one line in
//!    `agon_core::sports::core_sports!`.
//! 2. `agon_service::sports::<sport>` (this module) — the functions above,
//!    plus one line in `agon_sports!` below.
//!
//! Each generated type stays defined in its own file rather than centralized
//! here, so existing call sites (`crate::Score`, `crate::match_format::
//! MatchFormat`, `crate::live_score::LiveEventInput`, ...) don't change
//! paths — `agon_sports!` is invoked once per file, each time with a
//! different "template" macro (defined right next to that invocation) that
//! says what to build from the list for that file.
//!
//! The two sport lists (this one and `agon_core::sports::core_sports!`) can't
//! be one macro across the crate boundary — keep them in sync by hand;
//! nothing enforces it automatically.
//!
//! Tennis/badminton/squash/table-tennis are deliberately *not* here: they're
//! the generic, unmodeled sports (`Score::Sets`, no dedicated format/
//! live-event/stats), and stay as fixed arms hand-written alongside each
//! macro invocation. Making one of them fully modeled would mean giving it
//! the same treatment as any new sport — a module here plus a line in the
//! list — not touching this macro.
#[macro_export]
macro_rules! agon_sports {
    ($callback:ident) => {
        $callback! {
            Football {
                tag: "football",
                module: football,
                score: $crate::sports::football::FootballScore,
                format: $crate::sports::football::FootballFormat,
                live_event: $crate::sports::football::FootballLiveEvent,
                stats: $crate::sports::football::FootballPlayerStats,
            },
            Cricket {
                tag: "cricket",
                module: cricket,
                score: $crate::sports::cricket::CricketScore,
                format: $crate::sports::cricket::CricketFormat,
                live_event: $crate::sports::cricket::CricketLiveEvent,
                stats: $crate::sports::cricket::CricketPlayerStats,
            },
            Netball {
                tag: "netball",
                module: netball,
                score: $crate::sports::netball::NetballScore,
                format: $crate::sports::netball::NetballFormat,
                live_event: $crate::sports::netball::NetballLiveEvent,
                stats: $crate::sports::netball::NetballPlayerStats,
            },
        }
    };
}

pub mod cricket;
pub mod football;
pub mod netball;
