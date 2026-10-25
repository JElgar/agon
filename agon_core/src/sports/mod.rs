//! Sports: the registry of every sport Agon knows, and the DAO-side contract
//! a fully modeled sport implements.
//!
//! # Adding a sport
//!
//! 1. Add `agon_core::sports::<sport>`: the sport's record types (score,
//!    format, live event, lifetime stats), a unit marker type named after the
//!    sport, and `impl SportRecord` for it.
//! 2. Add `agon_service::sports::<sport>`: the API types and `impl Sport`
//!    (`agon_service::sports::Sport`) for the same marker type.
//! 3. Add one line to `modeled` in [`for_each_sport!`] below.
//!
//! Every sport-shaped enum, struct field and dispatcher in both crates
//! (`ScoreRecord`, `MatchFormatRecord`, `LiveEventPayloadRecord`,
//! `UserStatsRecord`, the rating `Sport`, the API's `MatchType`, `Score`,
//! `MatchFormat`, `LiveEventInput`, `UserStats` and the mapping between them)
//! is generated from that list, so nothing else needs editing.

use std::collections::HashMap;
use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::dao::records::{MatchFormatRecord, ScoreRecord};

pub mod cricket;
pub mod football;
pub mod netball;

/// The sport registry. Calls `$callback!` with every sport, in two groups:
///
/// - `modeled`: sports with their own score, format, live-event and stats
///   types. `Variant` names the marker type at
///   `agon_core::sports::<key>::<Variant>`, which implements [`SportRecord`]
///   here and `agon_service::sports::Sport` in the API crate.
/// - `generic`: sports scored with the shared set-by-set shape and common
///   stats only. They have no live scoring.
///
/// `tag` is the stored sport string (`MatchRecord::match_type`) and the serde
/// tag of the sport's variant in every generated enum. `key` is the same
/// string as an identifier: the module name, and the sport's field name in
/// `UserStatsRecord`/`UserStats` (the stats write path addresses
/// `stats.<tag>`, so the two must match).
#[macro_export]
macro_rules! for_each_sport {
    ($callback:ident) => {
        $callback! {
            modeled: [
                Football { tag: "football", key: football },
                Cricket { tag: "cricket", key: cricket },
                Netball { tag: "netball", key: netball },
            ],
            generic: [
                Tennis { tag: "tennis", key: tennis, label: "Tennis" },
                Badminton { tag: "badminton", key: badminton, label: "Badminton" },
                Squash { tag: "squash", key: squash, label: "Squash" },
                TableTennis { tag: "table_tennis", key: table_tennis, label: "Table Tennis" },
            ],
        }
    };
}

/// What one user contributed to a single match's confirmed score: every
/// counter that feeds their lifetime totals, and the single-match figures
/// worth keeping a personal best for. Empty when they didn't feature in the
/// box score.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SportContribution {
    /// Added to `stats.<tag>.<name>`. Each name must be a field on the
    /// sport's stats record.
    pub counters: HashMap<String, u64>,
    /// Stored at `stats.<tag>.best_<name>` when this match beats the current
    /// best. The stats record needs a matching `best_<name>` field.
    pub bests: HashMap<String, BestCandidate>,
}

/// A single-match figure that replaces the stored personal best when its
/// `rank` is higher. Stored as `detail` plus the rank under `rank_field` and
/// the match's id.
#[derive(Debug, Clone, PartialEq)]
pub struct BestCandidate {
    pub rank_field: &'static str,
    pub rank: u64,
    pub detail: serde_json::Map<String, serde_json::Value>,
}

impl BestCandidate {
    /// A plain figure, stored as `{ value, match_id }` (`BestFigureRecord`).
    pub fn value(value: u64) -> Self {
        Self {
            rank_field: "value",
            rank: value,
            detail: serde_json::Map::new(),
        }
    }

    /// A figure ranked by `rank_field`, with `detail` stored alongside it
    /// (e.g. a bowling spell's runs conceded and overs).
    pub fn ranked_by(
        rank_field: &'static str,
        rank: u64,
        detail: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        Self {
            rank_field,
            rank,
            detail,
        }
    }
}

/// Bounds every stored sport type shares.
pub trait RecordType: Debug + Clone + PartialEq + Serialize + DeserializeOwned {}
impl<T: Debug + Clone + PartialEq + Serialize + DeserializeOwned> RecordType for T {}

/// What the DAO and `agon_worker` need to know about a modeled sport, in
/// terms of its stored record shapes only (this crate has no `poem-openapi`).
pub trait SportRecord {
    /// How the sport is named to people, e.g. on share cards.
    const LABEL: &'static str;

    /// The stored score: the `ScoreRecord` variant for this sport.
    type ScoreRecord: RecordType;
    /// The stored match format: the `MatchFormatRecord` variant.
    type FormatRecord: RecordType;
    /// One stored live-scoring event: the `LiveEventPayloadRecord` variant.
    type LiveEventRecord: RecordType;
    /// The user's lifetime stats, stored at `stats.<tag>` on their profile.
    /// The stats write path adds counters by name, so every counter
    /// [`SportRecord::contribution`] returns must be a field here (missing
    /// fields default when read back).
    type StatsRecord: RecordType + Default;

    /// What a confirmed score contributes to one user's lifetime stats,
    /// beyond the played/won/drawn/lost every sport gets from the outcome.
    /// `player_ids` is every roster entry that user holds in the match
    /// (almost always one).
    fn contribution(
        score: &Self::ScoreRecord,
        player_ids: &[&str],
        format: Option<&Self::FormatRecord>,
    ) -> SportContribution;
}

macro_rules! define_contribution {
    (
        modeled: [$( $variant:ident { tag: $tag:literal, key: $key:ident } ),+ $(,)?],
        generic: [$( $gvariant:ident { tag: $gtag:literal, key: $gkey:ident, label: $glabel:literal } ),* $(,)?] $(,)?
    ) => {
        /// What a confirmed `score` contributes to the lifetime stats of the
        /// user holding `player_ids`, in `sport` (the stored sport tag). Empty
        /// for a generic sport, which only gets played/won/drawn/lost from the
        /// match outcome.
        pub fn contribution(
            sport: &str,
            score: &ScoreRecord,
            player_ids: &[&str],
            format: Option<&MatchFormatRecord>,
        ) -> SportContribution {
            match (sport, score) {
                $(
                    ($tag, ScoreRecord::$variant(score)) => {
                        #[allow(unreachable_patterns)]
                        let format = match format {
                            Some(MatchFormatRecord::$variant(f)) => Some(f),
                            _ => None,
                        };
                        <$key::$variant as SportRecord>::contribution(score, player_ids, format)
                    }
                )+
                _ => SportContribution::default(),
            }
        }
    };
}
for_each_sport!(define_contribution);
