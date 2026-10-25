//! Cricket: its stored record types and `SportRecord` impl.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{BestCandidate, SportContribution, SportRecord};
use crate::dao::records::{BestFigureRecord, GenericSportStatsRecord, default_true};

/// Cricket's stored score: the `ScoreRecord::Cricket` variant. Its stored shape is
/// guarded by `agon_core/tests/cricket_score_record_roundtrip.rs`.
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

/// Stored form of `agon_service::sports::cricket::CricketBattingEntry`.
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

/// Stored form of `agon_service::sports::cricket::CricketBowlingEntry`.
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

/// Stored form of `agon_service::sports::cricket::CricketExtras`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketExtrasRecord {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

/// Stored form of `agon_service::sports::cricket::CricketFallOfWicket`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CricketFallOfWicketRecord {
    pub wicket: u32,
    pub runs: u32,
    pub player_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overs: Option<OversRecord>,
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// over — mirrors `agon_service::sports::cricket::Overs`. Two integer
/// fields rather than a single float, which can't safely represent a ball
/// count that doesn't fit in one decimal digit.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OversRecord {
    pub overs: u32,
    pub balls: u32,
}

/// Stored form of `agon_service::sports::cricket::CricketFormat`.
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

/// Stored form of `agon_service::sports::cricket::CricketLiveEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CricketLiveEventRecord {
    Delivery(CricketDeliveryRecord),
    Retire(CricketRetireEventRecord),
    InningsStart(CricketInningsStartEventRecord),
    InningsEnd(CricketInningsEndEventRecord),
}

/// Stored form of `agon_service::sports::cricket::CricketDelivery` (reused verbatim as the
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
    /// Stored form of `agon_service::sports::cricket::CricketDelivery::occurred_at`
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

/// Stored form of `agon_service::sports::cricket::NextBallContext`.
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
    /// wicket count. See `Dao::update_best_figures`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_bowling: Option<BestBowlingFiguresRecord>,
}

/// A personal-best single-match bowling spell: most wickets taken, plus the
/// runs conceded and overs bowled in that same spell — e.g. "5 wickets for
/// 32 runs off 5.4 overs", not just "5 wickets". Ranked by `wickets` alone
/// (ties aren't broken by economy). See `Dao::update_best_figures`
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

/// Cricket: the marker type its sport traits are implemented for.
pub struct Cricket;

impl SportRecord for Cricket {
    const LABEL: &'static str = "Cricket";

    type ScoreRecord = CricketScoreRecord;
    type FormatRecord = CricketFormatRecord;
    type LiveEventRecord = CricketLiveEventRecord;
    type StatsRecord = CricketStatsRecord;

    /// Sums this player's batting (runs/fours/sixes/balls faced/dismissals),
    /// fielding (catches), and bowling (wickets/runs conceded/balls bowled)
    /// across every innings of a confirmed cricket score. A player can
    /// feature in more than one innings (e.g. a two-innings match), so these
    /// accumulate rather than take the last entry.
    ///
    /// Bests are "runs" (high score) and "bowling": the match's bowling
    /// figures ranked by wickets, stored with runs conceded and overs as a
    /// `BestBowlingFiguresRecord`. The rest (fours,
    /// sixes, balls faced, dismissals, catches, runs conceded, balls bowled)
    /// are lifetime totals only — nobody's asked to see "most balls faced in
    /// a game" as a record, so there's no reason to pay for tracking it.
    fn contribution(
        rec: &CricketScoreRecord,
        player_ids: &[&str],
        format: Option<&CricketFormatRecord>,
    ) -> SportContribution {
        let balls_per_over = balls_per_over(format);
        let is_theirs = |id: &str| player_ids.contains(&id);

        let mut counters = HashMap::new();
        let mut bests = HashMap::new();
        let mut spell_wickets = 0;
        let mut spell_runs_conceded = 0;
        let mut spell_balls = 0;
        let mut bowled = false;

        for inning in &rec.innings {
            for entry in inning.batting.iter().flatten() {
                if is_theirs(&entry.player_id) {
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
                    && dismissal
                        .fielder_player_id
                        .as_deref()
                        .is_some_and(is_theirs)
                {
                    *counters.entry("catches".to_string()).or_insert(0) += 1;
                }
            }
            for entry in inning.bowling.iter().flatten() {
                if is_theirs(&entry.player_id) {
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
            bests.insert("runs".to_string(), BestCandidate::value(*runs));
        }
        if bowled {
            let overs = balls_to_overs(spell_balls, balls_per_over);
            let detail = serde_json::json!({
                "runs_conceded": spell_runs_conceded,
                "overs": overs,
            });
            let serde_json::Value::Object(detail) = detail else {
                unreachable!("json! of an object literal is an object")
            };
            bests.insert(
                "bowling".to_string(),
                BestCandidate::ranked_by("wickets", spell_wickets, detail),
            );
        }

        SportContribution { counters, bests }
    }
}

/// This match's legal deliveries per over — its own `CricketFormatRecord`'s,
/// or the standard 6 when it has no (cricket) format configured.
fn balls_per_over(format: Option<&CricketFormatRecord>) -> u32 {
    format.map_or(6, |f| f.balls_per_over)
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
