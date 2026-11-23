//! Sports: the registry of every sport Agon knows, and the DAO-side contract
//! a fully modeled sport implements.
//!
//! # Adding a sport
//!
//! 1. Add `agon_services_core::sports::<sport>`: the sport's record types (score,
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

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::dao::records::{MatchFormatRecord, ScoreRecord};

pub mod cricket;
pub mod football;
pub mod netball;

/// The sport registry. Calls `$callback!` with every sport, in two groups:
///
/// - `modeled`: sports with their own score, format, live-event and stats
///   types. `Variant` names the marker type at
///   `agon_services_core::sports::<key>::<Variant>`, which implements [`SportRecord`]
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

    /// One match's counters in this sport, for one player or summed for a
    /// side: what the stats store (OpenSearch) keeps so team and player
    /// totals can be summed. Counters only: played/won/drawn/lost come from
    /// the outcome, and personal bests aren't summable.
    type MatchStatsRecord: RecordType + Default + Eq;

    /// Every field of [`SportRecord::MatchStatsRecord`], as serialized. The
    /// stats store maps and sums exactly these.
    const MATCH_STATS_FIELDS: &'static [&'static str];

    /// One player's counters, from their [`SportRecord::contribution`].
    /// `side` is their side's result in the match.
    fn match_stats(
        contribution: &Self::StatsRecord,
        side: &Self::SideResultRecord,
    ) -> Self::MatchStatsRecord;

    /// Adds `other` into `totals`.
    fn add_match_stats(totals: &mut Self::MatchStatsRecord, other: &Self::MatchStatsRecord);

    /// A side's counters, from the sum of its players'. Override for a
    /// counter that belongs to the side as a whole rather than to the sum
    /// of its players.
    fn side_match_stats(
        players: Self::MatchStatsRecord,
        _side: &Self::SideResultRecord,
    ) -> Self::MatchStatsRecord {
        players
    }

    /// One side's scoreline in a match, read from the confirmed score
    /// (e.g. goals for and against): what a team's record sums.
    type SideResultRecord: RecordType + Default + Eq;

    /// Every field of [`SportRecord::SideResultRecord`], as serialized.
    const SIDE_RESULT_FIELDS: &'static [&'static str];

    /// `side_id`'s scoreline in `score`.
    fn side_result(score: &Self::ScoreRecord, side_id: &str) -> Self::SideResultRecord;

    /// Adds `other` into `totals`.
    fn add_side_result(totals: &mut Self::SideResultRecord, other: &Self::SideResultRecord);
}

/// A stored per-sport record keyed by sport tag (`{"football": {...}}`),
/// summed field by field.
fn from_sums<T: DeserializeOwned>(fields: &[&str], sum: impl Fn(&str) -> u64) -> Option<T> {
    let fields: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .map(|f| (f.to_string(), sum(f).into()))
        .collect();
    serde_json::from_value(fields.into()).ok()
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

        /// One match's counters in its sport, as the stats store keeps them:
        /// one field per modeled sport, only the match's own sport set
        /// (the same layout as `UserStatsRecord`). Empty for a generic sport.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub struct MatchStatsRecord {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $key: Option<<$key::$variant as SportRecord>::MatchStatsRecord>,
            )+
        }

        impl MatchStatsRecord {
            /// The counters the user holding `player_ids` on `side_id` earned
            /// from a confirmed `score` in `sport`.
            pub fn for_player(
                sport: &str,
                score: &ScoreRecord,
                side_id: &str,
                player_ids: &[&str],
                format: Option<&MatchFormatRecord>,
                match_id: &str,
            ) -> Self {
                let mut out = Self::default();
                match (sport, score) {
                    $(
                        ($tag, ScoreRecord::$variant(score)) => {
                            #[allow(unreachable_patterns)]
                            let format = match format {
                                Some(MatchFormatRecord::$variant(f)) => Some(f),
                                _ => None,
                            };
                            let c = <$key::$variant as SportRecord>::contribution(
                                score, player_ids, format, match_id,
                            );
                            let side = <$key::$variant as SportRecord>::side_result(score, side_id);
                            out.$key = Some(<$key::$variant as SportRecord>::match_stats(&c, &side));
                        }
                    )+
                    _ => {}
                }
                out
            }

            /// Adds `other` into this, sport by sport.
            pub fn add(&mut self, other: &Self) {
                $(
                    if let Some(o) = &other.$key {
                        <$key::$variant as SportRecord>::add_match_stats(
                            self.$key.get_or_insert_with(Default::default),
                            o,
                        );
                    }
                )+
            }

            /// `side_id`'s counters from a confirmed `score` in `sport`,
            /// given its players' summed. Empty for a generic sport.
            pub fn for_side(sport: &str, score: &ScoreRecord, side_id: &str, players: Self) -> Self {
                let mut out = Self::default();
                match (sport, score) {
                    $(
                        ($tag, ScoreRecord::$variant(score)) => {
                            let side = <$key::$variant as SportRecord>::side_result(score, side_id);
                            let totals = players.$key.unwrap_or_default();
                            out.$key = Some(<$key::$variant as SportRecord>::side_match_stats(totals, &side));
                        }
                    )+
                    _ => {}
                }
                out
            }

            /// The counter fields `sport` keeps, under `stats.<sport>`.
            /// Empty for a generic sport.
            pub fn fields(sport: &str) -> &'static [&'static str] {
                match sport {
                    $( $tag => <$key::$variant as SportRecord>::MATCH_STATS_FIELDS, )+
                    _ => &[],
                }
            }

            /// Every modeled sport's tag and its counter fields.
            pub fn all_fields() -> Vec<(&'static str, &'static [&'static str])> {
                vec![$( ($tag, <$key::$variant as SportRecord>::MATCH_STATS_FIELDS), )+]
            }

            /// `sport`'s counters, from the summed value of each of its
            /// [`MatchStatsRecord::fields`]. Empty for a generic sport.
            pub fn from_sums(sport: &str, sum: impl Fn(&str) -> u64) -> Self {
                let mut out = Self::default();
                match sport {
                    $( $tag => out.$key = from_sums(<$key::$variant as SportRecord>::MATCH_STATS_FIELDS, sum), )+
                    _ => {}
                }
                out
            }
        }

        /// One side's scoreline in its sport, as the stats store keeps it:
        /// one field per modeled sport, only the match's own sport set.
        /// Empty for a generic sport.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub struct SideResultRecord {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $key: Option<<$key::$variant as SportRecord>::SideResultRecord>,
            )+
        }

        impl SideResultRecord {
            /// `side_id`'s scoreline in a confirmed `score` in `sport`.
            pub fn for_side(sport: &str, score: &ScoreRecord, side_id: &str) -> Self {
                let mut out = Self::default();
                match (sport, score) {
                    $(
                        ($tag, ScoreRecord::$variant(score)) => {
                            out.$key = Some(<$key::$variant as SportRecord>::side_result(score, side_id));
                        }
                    )+
                    _ => {}
                }
                out
            }

            /// Adds `other` into this, sport by sport.
            pub fn add(&mut self, other: &Self) {
                $(
                    if let Some(o) = &other.$key {
                        <$key::$variant as SportRecord>::add_side_result(
                            self.$key.get_or_insert_with(Default::default),
                            o,
                        );
                    }
                )+
            }

            /// The fields `sport` keeps, under `result.<sport>`. Empty for a
            /// generic sport.
            pub fn fields(sport: &str) -> &'static [&'static str] {
                match sport {
                    $( $tag => <$key::$variant as SportRecord>::SIDE_RESULT_FIELDS, )+
                    _ => &[],
                }
            }

            /// Every modeled sport's tag and its fields.
            pub fn all_fields() -> Vec<(&'static str, &'static [&'static str])> {
                vec![$( ($tag, <$key::$variant as SportRecord>::SIDE_RESULT_FIELDS), )+]
            }

            /// `sport`'s scoreline, from the summed value of each of its
            /// [`SideResultRecord::fields`]. Empty for a generic sport.
            pub fn from_sums(sport: &str, sum: impl Fn(&str) -> u64) -> Self {
                let mut out = Self::default();
                match sport {
                    $( $tag => out.$key = from_sums(<$key::$variant as SportRecord>::SIDE_RESULT_FIELDS, sum), )+
                    _ => {}
                }
                out
            }
        }
    };
}
for_each_sport!(define_contribution);

#[cfg(test)]
mod tests {
    use super::*;

    /// The `*_FIELDS` lists are what the stats store maps and sums, so each
    /// must name every field its type serializes, no more and no fewer.
    #[test]
    fn declared_fields_name_every_serialized_field() {
        let stats = MatchStatsRecord::all_fields()
            .into_iter()
            .map(|(sport, fields)| {
                let one = serde_json::to_value(MatchStatsRecord::from_sums(sport, |_| 1));
                (sport, fields, one.unwrap())
            });
        let results = SideResultRecord::all_fields()
            .into_iter()
            .map(|(sport, fields)| {
                let one = serde_json::to_value(SideResultRecord::from_sums(sport, |_| 1));
                (sport, fields, one.unwrap())
            });
        for (sport, fields, value) in stats.chain(results) {
            let mut serialized: Vec<&str> = value[sport]
                .as_object()
                .unwrap_or_else(|| panic!("{sport} parses from its own fields"))
                .keys()
                .map(String::as_str)
                .collect();
            serialized.sort();
            let mut declared = fields.to_vec();
            declared.sort();
            assert_eq!(serialized, declared, "{sport}");
            assert!(
                value[sport].as_object().unwrap().values().all(|v| v == 1),
                "{sport}: every declared field reads back"
            );
        }
    }

    #[test]
    fn a_generic_sport_has_no_match_stats() {
        assert!(MatchStatsRecord::fields("tennis").is_empty());
        assert!(SideResultRecord::fields("tennis").is_empty());
        let sets = ScoreRecord::Sets {
            entries: [("a".to_string(), vec![6, 6])].into_iter().collect(),
        };
        assert_eq!(
            MatchStatsRecord::for_side("tennis", &sets, "a", MatchStatsRecord::default()),
            MatchStatsRecord::default()
        );
        assert_eq!(
            SideResultRecord::for_side("tennis", &sets, "a"),
            SideResultRecord::default()
        );
    }
}
