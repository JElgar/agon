//! Per-sport DAO-side surface. One module per fully-modeled sport, holding
//! everything a DAO-only context needs for it: its score/format/live-event
//! record types, its lifetime stats record, and its `SportRecord` impl.
//!
//! `core_sports!` below is the single source of truth for which sports those
//! are. Every sport-shaped aggregate on this side is generated from it —
//! `ScoreRecord`/`MatchFormatRecord`/`LiveEventPayloadRecord`/
//! `UserStatsRecord` in `crate::dao::records`, and `contribution` below — so
//! adding a sport is a new module here plus one line in the list, nothing
//! else in this crate or in `agon_worker`.
//!
//! `agon_service` has its own counterpart (`agon_sports!`) for the API-side
//! enums. The two can't share one macro across the crate boundary, so the
//! list is declared twice — keep them in sync by hand; nothing enforces it
//! automatically.

use crate::dao::records::{MatchFormatRecord, ScoreRecord};
use crate::sport::{SportContribution, SportRecord};

pub mod cricket;
pub mod football;
pub mod netball;

/// x-macro: calls `$callback!` with the sport list. `tag` is the stored sport
/// string (`MatchRecord::match_type`), which is also the sport's key under
/// `UserRecord::stats` — the stats write path addresses `stats.<tag>` — so
/// `module` doubles as the `UserStatsRecord` field name and must equal it.
macro_rules! core_sports {
    ($callback:ident) => {
        $callback! {
            Football {
                tag: "football",
                module: football,
                score: $crate::sports::football::FootballScoreRecord,
                format: $crate::sports::football::FootballFormatRecord,
                live_event: $crate::sports::football::FootballLiveEventRecord,
                stats: $crate::sports::football::FootballStatsRecord,
                record: $crate::sports::football::FootballRecord,
            },
            Cricket {
                tag: "cricket",
                module: cricket,
                score: $crate::sports::cricket::CricketScoreRecord,
                format: $crate::sports::cricket::CricketFormatRecord,
                live_event: $crate::sports::cricket::CricketLiveEventRecord,
                stats: $crate::sports::cricket::CricketStatsRecord,
                record: $crate::sports::cricket::CricketRecord,
            },
            Netball {
                tag: "netball",
                module: netball,
                score: $crate::sports::netball::NetballScoreRecord,
                format: $crate::sports::netball::NetballFormatRecord,
                live_event: $crate::sports::netball::NetballLiveEventRecord,
                stats: $crate::sports::netball::NetballStatsRecord,
                record: $crate::sports::netball::NetballRecord,
            },
        }
    };
}
pub(crate) use core_sports;

macro_rules! define_contribution {
    ($( $variant:ident {
        tag: $tag:literal,
        module: $module:ident,
        score: $score:ty,
        format: $format:ty,
        live_event: $live_event:ty,
        stats: $stats:ty,
        record: $record:ty $(,)?
    } ),+ $(,)?) => {
        /// What a confirmed `score` contributes to `player_id`'s lifetime
        /// stats in `sport` (the stored sport tag). Empty for a sport with no
        /// per-player box score (tennis, badminton, ...) — those still get
        /// played/won/drawn/lost from the match outcome, just nothing more.
        pub fn contribution(
            sport: &str,
            score: &ScoreRecord,
            player_id: &str,
            format: Option<&MatchFormatRecord>,
        ) -> SportContribution {
            match sport {
                $( $tag => <$record as SportRecord>::contribution(score, player_id, format), )+
                _ => SportContribution::default(),
            }
        }
    };
}
core_sports!(define_contribution);
