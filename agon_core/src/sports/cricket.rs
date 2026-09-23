//! Cricket's whole DAO-side surface: its score/format/live-event record
//! types, its lifetime stats record, and its `SportRecord` impl (what a
//! confirmed score contributes to each player's stats).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dao::records::{
    BestFigureRecord, GenericSportStatsRecord, MatchFormatRecord, ScoreRecord, default_true,
};
use crate::sport::{SportContribution, SportRecord};

/// Cricket's `ScoreRecord` shape — see
/// `crate::sports::netball::NetballScoreRecord`'s doc comment for why this is
/// a standalone type rather than an inline enum-variant struct: wire-identical
/// to the inline shape it replaced, guarded by
/// `agon_core/tests/cricket_score_record_roundtrip.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketScoreRecord {
    pub innings: Vec<CricketScoreInningsRecord>,
    /// The current/most recent innings' recent-ball window — `None` once
    /// there isn't a "current" innings (between innings, or the match is
    /// over) or for a result with no ball-by-ball data behind it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_deliveries: Option<Vec<CricketDeliveryRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ball_context: Option<NextBallContextRecord>,
    /// True once the log's last innings has ended and no following one
    /// has started yet. `None` for a result with no live log behind it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_next_innings: Option<bool>,
}

/// One innings' final totals, as stored on a match's confirmed/pending
/// `Score` — mirrors the API's `CricketScoreInnings`. `batting`/`bowling`/
/// `fall_of_wickets`/`extras` are `None` for a manually-entered result with
/// no per-player detail to hand over; populated when the result came from a
/// live-scored (or backfilled) match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketScoreInningsRecord {
    pub batting_side_id: String,
    pub bowling_side_id: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: OversRecord,
    pub declared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batting: Option<Vec<CricketBattingEntryRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowling: Option<Vec<CricketBowlingEntryRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall_of_wickets: Option<Vec<CricketFallOfWicketRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<CricketExtrasRecord>,
}

/// Mirrors the API's `detailed_score::cricket::CricketBattingEntry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketBattingEntryRecord {
    pub player_id: String,
    pub runs: u32,
    pub balls_faced: u32,
    pub fours: u32,
    pub sixes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissal: Option<CricketDismissalRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batting_position: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDismissalRecord {
    pub kind: CricketDismissalKindRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fielder_player_id: Option<String>,
}

/// Mirrors the API's `detailed_score::cricket::CricketBowlingEntry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketBowlingEntryRecord {
    pub player_id: String,
    pub overs: OversRecord,
    pub maidens: u32,
    pub runs_conceded: u32,
    pub wickets: u32,
    pub wides: u32,
    pub no_balls: u32,
}

/// Mirrors the API's `detailed_score::cricket::CricketExtras`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketExtrasRecord {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

/// Mirrors the API's `detailed_score::cricket::CricketFallOfWicket`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketFallOfWicketRecord {
    pub wicket: u32,
    pub runs: u32,
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overs: Option<OversRecord>,
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// over — mirrors the API's `detailed_score::cricket::Overs`. Two integer
/// fields rather than a single float, which can't safely represent a ball
/// count that doesn't fit in one decimal digit.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OversRecord {
    pub overs: u32,
    pub balls: u32,
}

/// Mirrors `agon_service::match_format::CricketFormat`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketFormatRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overs_per_innings: Option<u32>,
    pub innings_per_side: u32,
    pub balls_per_over: u32,
    pub no_ball_penalty_runs: u32,
    pub wide_penalty_runs: u32,
    /// `#[serde(default = "default_true")]` for records written before these
    /// two fields existed — the standard rule (extra ball) is the safe
    /// default for a match that never configured otherwise.
    #[serde(default = "default_true")]
    pub wide_is_extra_ball: bool,
    #[serde(default = "default_true")]
    pub no_ball_is_extra_ball: bool,
    pub free_hit_after_no_ball: bool,
}

// ---- Cricket live events ----------------------------------------------------

/// Mirrors `agon_service::live_score::cricket::CricketLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CricketLiveEventRecord {
    Delivery(CricketDeliveryRecord),
    Retire(CricketRetireEventRecord),
    InningsStart(CricketInningsStartEventRecord),
    InningsEnd(CricketInningsEndEventRecord),
}

/// Mirrors `detailed_score::cricket::CricketDelivery` (reused verbatim as the
/// API's live delivery payload).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryRecord {
    pub over: u32,
    pub ball: u32,
    pub bowler_player_id: String,
    pub striker_player_id: String,
    pub non_striker_player_id: String,
    pub runs_off_bat: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<CricketDeliveryExtraRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wicket: Option<CricketDeliveryWicketRecord>,
    /// Mirrors `agon_service::detailed_score::cricket::CricketDelivery::occurred_at`
    /// — RFC3339, same string convention as `LiveEventRecord::occurred_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryExtraRecord {
    pub kind: CricketExtraKindRecord,
    pub runs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CricketExtraKindRecord {
    Wide,
    NoBall,
    Bye,
    LegBye,
    Penalty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketDeliveryWicketRecord {
    pub kind: CricketDismissalKindRecord,
    pub dismissed_player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fielder_player_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CricketDismissalKindRecord {
    Bowled,
    Caught,
    LegBeforeWicket,
    RunOut,
    Stumped,
    HitWicket,
    RetiredOut,
    RetiredHurt,
}

/// Mirrors `detailed_score::cricket::NextBallContext`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextBallContextRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub striker_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub non_striker_player_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bowler_player_id: Option<String>,
    pub over: u32,
    pub ball: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_over_bowler_player_id: Option<String>,
    pub runs_conceded_this_over: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketRetireEventRecord {
    pub batter_player_id: String,
    pub retired_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketInningsStartEventRecord {
    pub batting_side_id: String,
    pub bowling_side_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketInningsEndEventRecord {
    pub reason: InningsEndReasonRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InningsEndReasonRecord {
    AllOut,
    OversComplete,
    Declared,
    TargetReached,
}

// ===========================================================================
// Stats (lifetime per-user totals, stored inline on the user's profile as
// `stats.cricket` — see `crate::dao::records::UserStatsRecord`).
// ===========================================================================

/// Lifetime cricket stats: the common counters plus a batting/bowling summary
/// derived from every confirmed match's box score, and each counter's
/// personal-best single-match figure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CricketStatsRecord {
    #[serde(flatten)]
    pub common: GenericSportStatsRecord,
    // As with `GenericSportStatsRecord`, every counter here is
    // `#[serde(default)]`: only counters that were ever actually
    // incremented get written to `stats.cricket` (see `Dao::stats_delta`),
    // so e.g. a bowler who never batted has no `runs` attribute at all.
    #[serde(default)]
    pub runs: u64,
    #[serde(default)]
    pub wickets: u64,
    #[serde(default)]
    pub fours: u64,
    #[serde(default)]
    pub sixes: u64,
    /// Legal balls faced while batting (career total).
    #[serde(default)]
    pub balls_faced: u64,
    /// Times out as a batter — divisor for batting average. Not-out innings
    /// aren't counted, same convention as the sport's own "average".
    #[serde(default)]
    pub dismissals: u64,
    /// Catches taken (as the credited fielder on any dismissal, batting side
    /// or bowling side — a catch isn't tied to which side this player was
    /// fielding for in that innings).
    #[serde(default)]
    pub catches: u64,
    /// Runs conceded while bowling (career total) — divisor for economy.
    #[serde(default)]
    pub runs_conceded: u64,
    /// Legal balls bowled (career total) — divisor for economy, and the
    /// source for the displayed "overs bowled". Summed as a raw ball count
    /// rather than `Overs`, and — critically — each contributing match's
    /// legal-ball count is computed from *that match's own*
    /// `CricketFormatRecord::balls_per_over` (5-ball, 6-ball, whatever it
    /// was), not a fixed assumption, so the accumulation itself is exact
    /// regardless of how many different formats a career spans. The only
    /// approximation left is display: turning a cross-format total back into
    /// an "X overs Y balls" figure has to pick *some* over length, since a
    /// blended career total isn't really in any one format — this uses the
    /// standard 6-ball over, the same convention real-world career bowling
    /// figures are always reported in regardless of which tournaments
    /// contributed to them.
    #[serde(default)]
    pub balls_bowled: u64,
    /// Highest score in a single match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_runs: Option<BestFigureRecord>,
    /// Best single-match bowling spell — most wickets, with the runs
    /// conceded and overs bowled in that same spell so it isn't just a bare
    /// wicket count. See `Dao::update_best_bowling_figures`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_bowling: Option<BestBowlingFiguresRecord>,
}

/// A personal-best single-match bowling spell: most wickets taken, plus the
/// runs conceded and overs bowled in that same spell — e.g. "5 wickets for
/// 32 runs off 5.4 overs", not just "5 wickets". Ranked by `wickets` alone
/// (ties aren't broken by economy). See `Dao::update_best_bowling_figures`
/// for why this only ever ratchets up, same as `BestFigureRecord`.
///
/// `overs` is the exact figure from that one match (in that match's own
/// `balls_per_over`), not re-derived from a raw ball count under some
/// assumed over length — unlike the career `balls_bowled` total above, a
/// single match's own bowling figures have no cross-format ambiguity to
/// approximate away.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BestBowlingFiguresRecord {
    pub wickets: u64,
    pub runs_conceded: u64,
    pub overs: OversRecord,
    pub match_id: String,
}

/// One player's bowling figures in a single match (possibly summed across
/// more than one innings) — the raw material for `best_bowling`. Not
/// persisted as-is: `Dao::update_best_bowling_figures` writes it out as a
/// `BestBowlingFiguresRecord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BowlingSpell {
    pub wickets: u64,
    pub runs_conceded: u64,
    /// Legal balls bowled — also folded into the cumulative `balls_bowled`
    /// counter (see `CricketStatsRecord::balls_bowled`).
    pub balls_bowled: u64,
    /// Same ball count as whole overs + balls, in this match's own format —
    /// what `best_bowling` actually stores/displays.
    pub overs: OversRecord,
    /// This match's over length, kept alongside so `absorb` can re-derive
    /// `overs` exactly after summing.
    pub balls_per_over: u32,
}

impl BowlingSpell {
    /// Fold a second spell *from the same match* into this one — the rare
    /// case of one user appearing as more than one player in a single match.
    /// `overs` is re-derived from the summed raw ball count rather than
    /// added field-wise: two `Overs` values don't sum that way (balls can roll
    /// over into a whole extra over).
    pub fn absorb(&mut self, other: &BowlingSpell) {
        self.wickets += other.wickets;
        self.runs_conceded += other.runs_conceded;
        self.balls_bowled += other.balls_bowled;
        self.overs = balls_to_overs(self.balls_bowled, self.balls_per_over);
    }
}

/// Marker type for cricket's `SportRecord` impl — see
/// `agon_core::sports::netball::NetballRecord`'s doc comment for the pattern.
pub struct CricketRecord;

impl SportRecord for CricketRecord {
    /// Sums this player's batting (runs/fours/sixes/balls faced/dismissals),
    /// fielding (catches), and bowling (wickets/runs conceded/balls bowled)
    /// across every innings of a confirmed cricket score. A player can
    /// feature in more than one innings (e.g. a two-innings match), so these
    /// accumulate rather than take the last entry.
    ///
    /// Only "runs" (high score) is a best-candidate here — "wickets" is
    /// tracked as a richer `bowling_spell` (best bowling figures need
    /// runs-conceded/balls-bowled alongside the wicket count, not just a bare
    /// scalar; see `Dao::update_best_bowling_figures`), and the rest (fours,
    /// sixes, balls faced, dismissals, catches, runs conceded, balls bowled)
    /// are lifetime totals only — nobody's asked to see "most balls faced in
    /// a game" as a record, so there's no reason to pay for tracking it.
    fn contribution(
        score: &ScoreRecord,
        player_id: &str,
        format: Option<&MatchFormatRecord>,
    ) -> SportContribution {
        let ScoreRecord::Cricket(rec) = score else {
            return SportContribution::default();
        };
        let balls_per_over = balls_per_over(format);

        let mut counters = HashMap::new();
        let mut best_candidates = HashMap::new();
        let mut spell_wickets = 0;
        let mut spell_runs_conceded = 0;
        let mut spell_balls = 0;
        let mut bowled = false;

        for inning in &rec.innings {
            for entry in inning.batting.iter().flatten() {
                if entry.player_id == player_id {
                    *counters.entry("runs".to_string()).or_insert(0) += entry.runs as u64;
                    *counters.entry("fours".to_string()).or_insert(0) += entry.fours as u64;
                    *counters.entry("sixes".to_string()).or_insert(0) += entry.sixes as u64;
                    *counters.entry("balls_faced".to_string()).or_insert(0) +=
                        entry.balls_faced as u64;
                    if entry.dismissal.is_some() {
                        *counters.entry("dismissals".to_string()).or_insert(0) += 1;
                    }
                }
                // Catches: this player credited as the fielder on *any* batter's
                // dismissal in the innings, regardless of which side they
                // batted for (a catch is a fielding contribution, not tied to
                // this player's own batting entry).
                if let Some(dismissal) = &entry.dismissal
                    && matches!(dismissal.kind, CricketDismissalKindRecord::Caught)
                    && dismissal.fielder_player_id.as_deref() == Some(player_id)
                {
                    *counters.entry("catches".to_string()).or_insert(0) += 1;
                }
            }
            for entry in inning.bowling.iter().flatten() {
                if entry.player_id == player_id {
                    let balls =
                        overs_to_balls(entry.overs.overs, entry.overs.balls, balls_per_over);
                    *counters.entry("wickets".to_string()).or_insert(0) += entry.wickets as u64;
                    *counters.entry("runs_conceded".to_string()).or_insert(0) +=
                        entry.runs_conceded as u64;
                    *counters.entry("balls_bowled".to_string()).or_insert(0) += balls;
                    bowled = true;
                    spell_wickets += entry.wickets as u64;
                    spell_runs_conceded += entry.runs_conceded as u64;
                    spell_balls += balls;
                }
            }
        }

        if let Some(runs) = counters.get("runs") {
            best_candidates.insert("runs".to_string(), *runs);
        }

        SportContribution {
            counters,
            best_candidates,
            bowling_spell: bowled.then(|| BowlingSpell {
                wickets: spell_wickets,
                runs_conceded: spell_runs_conceded,
                balls_bowled: spell_balls,
                overs: balls_to_overs(spell_balls, balls_per_over),
                balls_per_over,
            }),
        }
    }
}

/// This match's legal deliveries per over — its own `CricketFormatRecord`'s,
/// or the standard 6 when it has no (cricket) format configured.
fn balls_per_over(format: Option<&MatchFormatRecord>) -> u32 {
    match format {
        Some(MatchFormatRecord::Cricket(f)) => f.balls_per_over,
        _ => 6,
    }
}

/// Legal balls bowled, exact — uses this match's own `balls_per_over` (from
/// its `CricketFormatRecord`, or the standard 6 if unconfigured), not an
/// assumed one, so a 5-ball-over match (e.g. The Hundred) contributes its
/// true ball count rather than a slightly-off one.
fn overs_to_balls(overs: u32, balls: u32, balls_per_over: u32) -> u64 {
    overs as u64 * balls_per_over as u64 + balls as u64
}

/// The inverse of `overs_to_balls` — a raw ball count back to whole overs +
/// balls, in the same `balls_per_over`.
fn balls_to_overs(balls: u64, balls_per_over: u32) -> OversRecord {
    OversRecord {
        overs: (balls / balls_per_over as u64) as u32,
        balls: (balls % balls_per_over as u64) as u32,
    }
}

#[cfg(test)]
mod tests {
    use aws_sdk_dynamodb::types::AttributeValue;

    use super::*;
    use crate::dao::records::UserStatsRecord;

    /// The whole point of threading a match's real `balls_per_over` through
    /// instead of assuming 6: a 5-ball-over spell (The Hundred) must convert
    /// to its true ball count, not one that's off by however many overs it
    /// ran.
    #[test]
    fn overs_to_balls_respects_a_non_standard_balls_per_over() {
        // 3 overs + 4 balls at 5 balls/over = 19 balls, not 3*6+4 = 22.
        assert_eq!(overs_to_balls(3, 4, 5), 19);
        // The standard case still works as before.
        assert_eq!(overs_to_balls(8, 4, 6), 52);
    }

    /// `balls_to_overs` is the exact inverse of `overs_to_balls` for any
    /// `balls_per_over`, including a partial (not-yet-complete) over.
    #[test]
    fn balls_to_overs_round_trips_overs_to_balls() {
        for balls_per_over in [5, 6] {
            for overs in 0..10u32 {
                for balls in 0..balls_per_over {
                    let total = overs_to_balls(overs, balls, balls_per_over);
                    let round_tripped = balls_to_overs(total, balls_per_over);
                    assert_eq!(
                        round_tripped,
                        OversRecord { overs, balls },
                        "{overs}.{balls} at {balls_per_over}/over -> {total} balls -> {round_tripped:?}"
                    );
                }
            }
        }
    }

    /// Summing two spells from the same match re-derives `overs` from the
    /// summed raw ball count in that match's own over length — not field-wise
    /// (2.3 + 1.4 at 5 balls/over is 13 + 9 = 22 balls = 4.2, not "3.7").
    #[test]
    fn absorbing_a_second_spell_rolls_balls_over_into_whole_overs() {
        let spell = |wickets, balls| BowlingSpell {
            wickets,
            runs_conceded: 10,
            balls_bowled: balls,
            overs: balls_to_overs(balls, 5),
            balls_per_over: 5,
        };
        let mut acc = spell(1, 13);
        acc.absorb(&spell(2, 9));
        assert_eq!(acc.wickets, 3);
        assert_eq!(acc.runs_conceded, 20);
        assert_eq!(acc.balls_bowled, 22);
        assert_eq!(acc.overs, OversRecord { overs: 4, balls: 2 });
    }

    /// `stats.<sport>` is only ever populated with the counters that have
    /// actually been incremented (see `Dao::stats_delta`/`ensure_stats_sport`),
    /// so a player who has only ever bowled (no `runs`, `fours`, `sixes`,
    /// `balls_faced`, `dismissals`, `wins`/`draws`/`losses` beyond whichever
    /// outcome actually happened, ...) has a sparse `stats.cricket` map, not
    /// one with every counter present at `0`. This must deserialize rather
    /// than 500 with "missing field" (the bug behind this test).
    #[test]
    fn sparse_cricket_stats_deserializes() {
        let stats_av = AttributeValue::M(HashMap::from([(
            "cricket".to_string(),
            AttributeValue::M(HashMap::from([
                ("matches_played".to_string(), AttributeValue::N("3".into())),
                ("wins".to_string(), AttributeValue::N("3".into())),
                ("wickets".to_string(), AttributeValue::N("5".into())),
            ])),
        )]));
        let rec: UserStatsRecord = serde_dynamo::from_attribute_value(stats_av).unwrap();
        let cricket = rec.cricket.expect("cricket stats present");
        assert_eq!(cricket.common.matches_played, 3);
        assert_eq!(cricket.common.wins, 3);
        assert_eq!(cricket.common.draws, 0);
        assert_eq!(cricket.common.losses, 0);
        assert_eq!(cricket.wickets, 5);
        assert_eq!(cricket.runs, 0);
        assert_eq!(cricket.balls_faced, 0);
        assert_eq!(cricket.balls_bowled, 0);
    }
}
