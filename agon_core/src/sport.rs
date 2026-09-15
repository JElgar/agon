//! The `SportRecord` trait: everything a DAO-only context (the DAO itself,
//! `agon_worker`) needs to know about one sport, purely in terms of its
//! DynamoDB record shapes. No dependency on `poem-openapi` — `agon_worker`
//! depends only on this crate, not `agon_service`.
//!
//! Pairs with `agon_service::sport::SportApi`, which bridges a sport's API
//! (poem-openapi) types to the DAO shapes named here. See the project's
//! sport-setup refactor design notes for the split rationale and the
//! `define_sports!` macro this is expected to grow into once every sport is
//! migrated.

use std::collections::HashMap;

use crate::dao::stats::BowlingSpell;

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

/// A sport's DAO-facing contract. `agon_core::dao::records::ScoreRecord`
/// stays one enum covering every sport (so a match's score is still one
/// value regardless of sport) — implementations pattern-match out their own
/// variant and return `SportContribution::default()` for any other, same as
/// the free functions this replaces.
pub trait SportRecord {
    /// The DAO's string sport tag, e.g. `"netball"` — matches
    /// `MatchRecord.sport` / `MatchScoreRecord.sport`.
    const NAME: &'static str;

    /// This player's box-score contribution to a confirmed score. Cricket
    /// additionally needs the match's `balls_per_over` (from its
    /// `CricketFormatRecord`, standard 6 if unconfigured) to convert overs to
    /// an exact ball count — threaded through as-is rather than a full
    /// `Option<&MatchFormatRecord>`, since no other sport needs format
    /// context here yet.
    fn contribution(
        score: &crate::dao::records::ScoreRecord,
        player_id: &str,
        balls_per_over: u32,
    ) -> SportContribution;
}
