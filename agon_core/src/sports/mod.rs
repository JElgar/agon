//! Sports: the registry of every sport Agon knows, and the DAO-side contract
//! a fully modeled sport implements.
//!
//! # Adding a sport
//!
//! 1. Add `agon_core::sports::<sport>`: the sport's record types (score,
//!    format, live event, lifetime stats), a unit marker type named after the
//!    sport, and `impl SportRecord` for it. List the stats record's personal
//!    bests with [`personal_bests!`].
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

/// What one user contributed to a single match, flattened from the sport's
/// per-match stats record (see [`SportRecord::contribution`]) into the shape
/// the stats write path applies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SportContribution {
    /// Every nonzero counter, added to `stats.<tag>.<name>`.
    pub counters: HashMap<String, u64>,
    /// This match's personal-best figures, each stored at
    /// `stats.<tag>.<field>` when it beats the stored one.
    pub bests: Vec<PersonalBestUpdate>,
}

/// One `best_*` field of a per-match stats record, ready for the stats write
/// path: the stored record and what it's ranked by.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonalBestUpdate {
    /// The stats record field, e.g. `best_bowling`.
    pub field: &'static str,
    /// The record field the stored best is compared on.
    pub rank_field: &'static str,
    pub rank: u64,
    pub record: serde_json::Value,
}

impl PersonalBestUpdate {
    pub fn new<B: PersonalBest>(field: &'static str, best: &B) -> Self {
        Self {
            field,
            rank_field: B::RANK_FIELD,
            rank: best.rank(),
            record: serde_json::to_value(best).expect("a personal-best record serializes to JSON"),
        }
    }
}

/// A personal-best record type, stored in a `best_*` field of a sport's
/// stats record. A new best replaces the stored one when its rank is higher.
pub trait PersonalBest: Serialize {
    /// The serialized field holding [`PersonalBest::rank`].
    const RANK_FIELD: &'static str;
    fn rank(&self) -> u64;
}

/// Lists the personal-best fields of a sport's stats record.
pub trait SportStats: Serialize {
    fn personal_bests(&self) -> Vec<PersonalBestUpdate>;
}

/// Implements [`SportStats`] for a stats record from its `best_*` fields,
/// each an `Option` of a [`PersonalBest`] type:
/// `personal_bests!(CricketStatsRecord { best_runs, best_bowling });`
#[macro_export]
macro_rules! personal_bests {
    ($record:ty { $($field:ident),* $(,)? }) => {
        impl $crate::sports::SportStats for $record {
            fn personal_bests(&self) -> Vec<$crate::sports::PersonalBestUpdate> {
                #[allow(unused_mut)]
                let mut bests = Vec::new();
                $(
                    if let Some(best) = &self.$field {
                        bests.push($crate::sports::PersonalBestUpdate::new(
                            stringify!($field),
                            best,
                        ));
                    }
                )*
                bests
            }
        }
    };
}

impl SportContribution {
    /// Flattens one match's stats record: every nonzero integer field is a
    /// counter, and its personal bests come from [`SportStats`].
    pub fn from_stats<S: SportStats>(stats: &S) -> Self {
        let counters = match serde_json::to_value(stats) {
            Ok(serde_json::Value::Object(fields)) => fields
                .into_iter()
                .filter_map(|(name, value)| Some((name, value.as_u64()?)))
                .filter(|(_, value)| *value > 0)
                .collect(),
            _ => HashMap::new(),
        };
        Self {
            counters,
            bests: stats.personal_bests(),
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
    /// The user's lifetime stats, stored at `stats.<tag>` on their profile:
    /// integer counters plus `best_*` personal bests (listed with
    /// [`personal_bests!`]). Every field defaults when missing, since the
    /// stored map only holds what has been written.
    type StatsRecord: RecordType + Default + SportStats;

    /// This match's stats for one user, from its confirmed score: the
    /// counters they earned here (added to their lifetime totals) and this
    /// match's figures in each `best_*` field (kept if they beat the stored
    /// best). Leave the common played/won/drawn/lost counters at zero, since
    /// they come from the match outcome. `player_ids` is every roster entry
    /// that user holds in the match (almost always one).
    fn contribution(
        score: &Self::ScoreRecord,
        player_ids: &[&str],
        format: Option<&Self::FormatRecord>,
        match_id: &str,
    ) -> Self::StatsRecord;
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
            match_id: &str,
        ) -> SportContribution {
            match (sport, score) {
                $(
                    ($tag, ScoreRecord::$variant(score)) => {
                        #[allow(unreachable_patterns)]
                        let format = match format {
                            Some(MatchFormatRecord::$variant(f)) => Some(f),
                            _ => None,
                        };
                        SportContribution::from_stats(&<$key::$variant as SportRecord>::contribution(
                            score, player_ids, format, match_id,
                        ))
                    }
                )+
                _ => SportContribution::default(),
            }
        }
    };
}
for_each_sport!(define_contribution);
