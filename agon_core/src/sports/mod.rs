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

use cricket::BowlingSpell;

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

/// One player's box-score contribution to a single match's confirmed score:
/// every counter that feeds their lifetime totals, the subset worth a
/// personal-best record, and (cricket only) their bowling figures in this
/// match. Empty when the player didn't feature in the box score.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SportContribution {
    pub counters: HashMap<String, u64>,
    pub best_candidates: HashMap<String, u64>,
    /// Bowling figures need runs conceded and balls bowled alongside the
    /// wicket count, not just a scalar (see `Dao::update_best_bowling_figures`).
    pub bowling_spell: Option<BowlingSpell>,
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

    /// What a confirmed score contributes to `player_id`'s lifetime stats,
    /// beyond the played/won/drawn/lost every sport gets from the outcome.
    fn contribution(
        score: &Self::ScoreRecord,
        player_id: &str,
        format: Option<&Self::FormatRecord>,
    ) -> SportContribution;
}

macro_rules! define_contribution {
    (
        modeled: [$( $variant:ident { tag: $tag:literal, key: $key:ident } ),+ $(,)?],
        generic: [$( $gvariant:ident { tag: $gtag:literal, key: $gkey:ident, label: $glabel:literal } ),* $(,)?] $(,)?
    ) => {
        /// What a confirmed `score` contributes to `player_id`'s lifetime
        /// stats in `sport` (the stored sport tag). Empty for a generic sport,
        /// which only gets played/won/drawn/lost from the match outcome.
        pub fn contribution(
            sport: &str,
            score: &ScoreRecord,
            player_id: &str,
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
                        <$key::$variant as SportRecord>::contribution(score, player_id, format)
                    }
                )+
                _ => SportContribution::default(),
            }
        }
    };
}
for_each_sport!(define_contribution);
