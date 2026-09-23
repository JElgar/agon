//! The `SportRecord` trait: what a DAO-only context (the DAO itself,
//! `agon_worker`) needs to know about one sport, purely in terms of its
//! DynamoDB record shapes. No dependency on `poem-openapi` — `agon_worker`
//! depends only on this crate, not `agon_service`.
//!
//! Each fully-modeled sport implements it in its own `crate::sports::<sport>`
//! module; `crate::sports::contribution` dispatches to the right one by sport
//! tag (generated from the sport list — see `crate::sports`).

use std::collections::HashMap;

use crate::dao::records::{MatchFormatRecord, ScoreRecord};
use crate::sports::cricket::BowlingSpell;

/// One player's box-score contribution to a single match's *confirmed*
/// score — every counter that feeds their lifetime totals, the subset worth
/// a personal-best record, and (cricket-only, today) their bowling figures
/// in this match. Empty for a sport with no per-player box score to derive
/// any of this from yet, or when the player didn't feature at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SportContribution {
    pub counters: HashMap<String, u64>,
    pub best_candidates: HashMap<String, u64>,
    /// Cricket-only today — bowling figures need runs-conceded/balls-bowled
    /// alongside the wicket count, not just a bare scalar (see
    /// `Dao::update_best_bowling_figures`). Generalize to an enum of "rich"
    /// candidates only once a second sport actually needs one.
    pub bowling_spell: Option<BowlingSpell>,
}

/// A sport's DAO-facing contract. `ScoreRecord` stays one enum covering every
/// sport (so a match's score is still one value regardless of sport) —
/// implementations pattern-match out their own variant and return
/// `SportContribution::default()` for any other.
pub trait SportRecord {
    /// This player's box-score contribution to a confirmed score. `format` is
    /// the match's configured format, for a sport whose counting depends on
    /// it (cricket's `balls_per_over`); most sports ignore it.
    fn contribution(
        score: &ScoreRecord,
        player_id: &str,
        format: Option<&MatchFormatRecord>,
    ) -> SportContribution;
}
