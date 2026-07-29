use poem_openapi::{Enum, Object};

/// Full cricket scorecard: one entry per innings, each with batting and
/// bowling cards plus the extras/fall-of-wicket detail.
#[derive(Object)]
pub struct CricketDetail {
    pub innings: Vec<CricketInnings>,
}

/// A count of overs bowled/faced: whole overs plus balls into the current
/// (not yet complete) over — not a single float like `19.4`, which only
/// works by coincidence for a standard 6-ball over. A float can't safely
/// represent a ball count that doesn't fit in one decimal digit (a
/// hypothetical over of 10+ balls collides with itself: 10 balls and 1 ball
/// both read as `.10`/`.1`), and an exact count has no business being a
/// float in the first place.
#[derive(Object, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overs {
    /// Completed overs.
    pub overs: u32,
    /// Balls into the current over (0..balls_per_over).
    pub balls: u32,
}

/// A cricket innings supports three tiers of fidelity:
///   1. Minimal — just `runs`/`wickets`/`overs`, leaving the cards empty.
///   2. Scorecard — full per-player `batting`/`bowling` cards.
///   3. Ball-by-ball — a `deliveries` log (for live in-app scoring).
///
/// The aggregate fields (`runs`, `wickets`, `overs`, `batting`, `bowling`,
/// `extras`, `fall_of_wickets`) are the always-present *overview*. When
/// `deliveries` is populated it is the source of truth and the overview is
/// generated from it; otherwise the overview is entered directly.
#[derive(Object)]
pub struct CricketInnings {
    /// The batting side for this innings (references MatchSide.id).
    pub batting_side_id: String,
    /// The bowling/fielding side for this innings.
    pub bowling_side_id: String,
    /// Total runs scored in the innings.
    pub runs: u32,
    /// Wickets lost (0-10).
    pub wickets: u32,
    /// Overs bowled, e.g. 19 overs + 4 balls into the 20th.
    pub overs: Overs,
    /// Whether the innings has been declared closed.
    pub declared: bool,
    pub batting: Vec<CricketBattingEntry>,
    pub bowling: Vec<CricketBowlingEntry>,
    pub extras: CricketExtras,
    pub fall_of_wickets: Vec<CricketFallOfWicket>,
    /// Optional ball-by-ball log. When present, the overview above is generated
    /// from these deliveries. Empty when the innings was not scored ball-by-ball.
    pub deliveries: Vec<CricketDelivery>,
}

/// A single delivery (ball) in an innings. The atomic unit of in-app scoring.
/// Also the live-scoring "delivery" event payload verbatim (see
/// `crate::live_score::cricket::CricketLiveEvent::Delivery`) — a live match's
/// ball-by-ball log *is* a sequence of these, folded incrementally instead of
/// submitted whole.
#[derive(Object, Clone)]
pub struct CricketDelivery {
    /// Over number, 0-based.
    pub over: u32,
    /// Legal-ball number within the over (1-6). Extras may add deliveries that
    /// do not advance this count (wides, no-balls).
    pub ball: u32,
    pub bowler_player_id: String,
    /// Batter on strike for this delivery.
    pub striker_player_id: String,
    /// Batter at the non-striker's end.
    pub non_striker_player_id: String,
    /// Runs scored off the bat (excludes extras).
    pub runs_off_bat: u32,
    /// Extra (wide / no-ball / bye / leg-bye / penalty), if this was one.
    pub extra: Option<CricketDeliveryExtra>,
    /// Wicket that fell on this delivery, if any.
    pub wicket: Option<CricketDeliveryWicket>,
}

#[derive(Object, Clone)]
pub struct CricketDeliveryExtra {
    pub kind: CricketExtraKind,
    /// Extra runs awarded for this delivery.
    pub runs: u32,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum CricketExtraKind {
    Wide,
    NoBall,
    Bye,
    LegBye,
    Penalty,
}

#[derive(Object, Clone)]
pub struct CricketDeliveryWicket {
    pub kind: CricketDismissalKind,
    /// The batter dismissed (usually the striker, but run-outs can be either).
    pub dismissed_player_id: String,
    /// Bowler credited (none for run outs / retired).
    pub bowler_player_id: Option<String>,
    /// Fielder involved: catcher, stumper, run-out thrower.
    pub fielder_player_id: Option<String>,
}

#[derive(Object)]
pub struct CricketBattingEntry {
    pub player_id: String,
    pub runs: u32,
    pub balls_faced: u32,
    pub fours: u32,
    pub sixes: u32,
    /// How the batter was dismissed. None if not out.
    pub dismissal: Option<CricketDismissal>,
    /// Batting order position (1 = opener).
    pub batting_position: Option<u32>,
}

#[derive(Object)]
pub struct CricketDismissal {
    pub kind: CricketDismissalKind,
    /// Bowler credited with the wicket (none for run outs / retired).
    pub bowler_player_id: Option<String>,
    /// Fielder involved: catcher, stumper, run-out thrower.
    pub fielder_player_id: Option<String>,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum CricketDismissalKind {
    Bowled,
    Caught,
    LegBeforeWicket,
    RunOut,
    Stumped,
    HitWicket,
    /// Left the field (injury/illness) and did not return. Counts as a
    /// wicket, unlike `RetiredHurt`. Recorded via a live `Retire` event with
    /// `retired_out: true` (see `crate::live_score::cricket`).
    RetiredOut,
    /// Left the field (injury/illness) but may return — the same
    /// `player_id` simply reappears on a later delivery. Does not count as a
    /// wicket. Recorded via a live `Retire` event with `retired_out: false`.
    RetiredHurt,
}

#[derive(Object)]
pub struct CricketBowlingEntry {
    pub player_id: String,
    /// Overs bowled, e.g. 4 overs + 0 balls, or 3 overs + 2 balls.
    pub overs: Overs,
    pub maidens: u32,
    pub runs_conceded: u32,
    pub wickets: u32,
    pub wides: u32,
    pub no_balls: u32,
}

#[derive(Object)]
pub struct CricketExtras {
    pub byes: u32,
    pub leg_byes: u32,
    pub wides: u32,
    pub no_balls: u32,
    pub penalty: u32,
}

#[derive(Object)]
pub struct CricketFallOfWicket {
    /// Wicket number (1 = first wicket to fall).
    pub wicket: u32,
    /// Team score when this wicket fell.
    pub runs: u32,
    /// Batter dismissed.
    pub player_id: String,
    /// Overs completed when the wicket fell, if recorded.
    pub overs: Option<Overs>,
}

/// Cricket scoring rules encoded by the aggregator below:
/// - A delivery is *legal* (counts toward overs and balls faced) unless it is a
///   wide or no-ball.
/// - `runs_off_bat` is credited to the striker and the team total.
/// - Wides and no-balls are charged to the bowler; byes, leg-byes and penalties
///   are added to the team total but NOT charged to the bowler.
/// - A bowler is credited with a wicket only for bowled, caught, LBW, stumped,
///   or hit-wicket; run-outs and retirements are not.
/// - A maiden is an over in which the bowler concedes no runs (byes/leg-byes do
///   not count against the bowler, so they do not break a maiden).
fn dismissal_credited_to_bowler(kind: &CricketDismissalKind) -> bool {
    matches!(
        kind,
        CricketDismissalKind::Bowled
            | CricketDismissalKind::Caught
            | CricketDismissalKind::LegBeforeWicket
            | CricketDismissalKind::Stumped
            | CricketDismissalKind::HitWicket
    )
}

/// True if the delivery counts as a legal ball (advances the over).
fn is_legal_delivery(delivery: &CricketDelivery) -> bool {
    !matches!(
        delivery.extra.as_ref().map(|e| &e.kind),
        Some(CricketExtraKind::Wide) | Some(CricketExtraKind::NoBall)
    )
}

/// Runs charged to the bowler for a delivery: runs off the bat plus wides and
/// no-balls. Byes, leg-byes and penalties are not the bowler's responsibility.
fn runs_charged_to_bowler(delivery: &CricketDelivery) -> u32 {
    let extra = delivery
        .extra
        .as_ref()
        .filter(|e| matches!(e.kind, CricketExtraKind::Wide | CricketExtraKind::NoBall))
        .map(|e| e.runs)
        .unwrap_or(0);
    delivery.runs_off_bat + extra
}

/// Converts a count of legal balls into whole overs + balls, e.g. 13 balls
/// -> 2 overs + 1 ball, given how many legal deliveries make an over (6 for
/// almost everything, 5 for The Hundred).
fn balls_to_overs(balls: u32, balls_per_over: u32) -> Overs {
    Overs {
        overs: balls / balls_per_over,
        balls: balls % balls_per_over,
    }
}

impl CricketInnings {
    /// Builds the innings overview (totals, batting/bowling cards, extras,
    /// fall-of-wickets) from a ball-by-ball delivery log. The deliveries are
    /// retained on the returned innings as the source of truth.
    /// `balls_per_over` is the match's configured over length (see
    /// `agon_service::match_format::CricketFormat::balls_per_over`) — it's
    /// the only piece of match format the DAO-agnostic scoring math actually
    /// needs, so it's threaded in as a plain argument rather than the whole
    /// format.
    pub fn from_deliveries(
        batting_side_id: String,
        bowling_side_id: String,
        declared: bool,
        deliveries: Vec<CricketDelivery>,
        balls_per_over: u32,
    ) -> Self {
        // First-appearance-ordered accumulators keyed by player_id.
        let mut batting: Vec<CricketBattingEntry> = Vec::new();
        let mut bowling: Vec<CricketBowlingEntry> = Vec::new();
        let mut extras = CricketExtras {
            byes: 0,
            leg_byes: 0,
            wides: 0,
            no_balls: 0,
            penalty: 0,
        };
        let mut fall_of_wickets: Vec<CricketFallOfWicket> = Vec::new();
        let mut total_runs: u32 = 0;
        let mut wickets: u32 = 0;
        let mut legal_balls: u32 = 0;
        // (bowler_player_id, over) -> runs charged, to detect maidens.
        let mut over_runs: Vec<((String, u32), u32)> = Vec::new();

        fn batter<'a>(
            batting: &'a mut Vec<CricketBattingEntry>,
            player_id: &str,
        ) -> &'a mut CricketBattingEntry {
            if let Some(pos) = batting.iter().position(|b| b.player_id == player_id) {
                &mut batting[pos]
            } else {
                batting.push(CricketBattingEntry {
                    player_id: player_id.to_string(),
                    runs: 0,
                    balls_faced: 0,
                    fours: 0,
                    sixes: 0,
                    dismissal: None,
                    batting_position: Some(batting.len() as u32 + 1),
                });
                batting.last_mut().unwrap()
            }
        }

        fn bowler<'a>(
            bowling: &'a mut Vec<CricketBowlingEntry>,
            player_id: &str,
        ) -> &'a mut CricketBowlingEntry {
            if let Some(pos) = bowling.iter().position(|b| b.player_id == player_id) {
                &mut bowling[pos]
            } else {
                bowling.push(CricketBowlingEntry {
                    player_id: player_id.to_string(),
                    overs: Overs { overs: 0, balls: 0 },
                    maidens: 0,
                    runs_conceded: 0,
                    wickets: 0,
                    wides: 0,
                    no_balls: 0,
                });
                bowling.last_mut().unwrap()
            }
        }

        for delivery in &deliveries {
            let legal = is_legal_delivery(delivery);
            let charged = runs_charged_to_bowler(delivery);
            let extra_runs = delivery.extra.as_ref().map(|e| e.runs).unwrap_or(0);

            total_runs += delivery.runs_off_bat + extra_runs;
            if legal {
                legal_balls += 1;
            }

            // Batting: striker is credited runs off the bat and faces legal
            // balls and no-balls (but not wides).
            {
                let b = batter(&mut batting, &delivery.striker_player_id);
                b.runs += delivery.runs_off_bat;
                let faced_ball = !matches!(
                    delivery.extra.as_ref().map(|e| &e.kind),
                    Some(CricketExtraKind::Wide)
                );
                if faced_ball {
                    b.balls_faced += 1;
                }
                match delivery.runs_off_bat {
                    4 => b.fours += 1,
                    6 => b.sixes += 1,
                    _ => {}
                }
            }

            // Bowling figures.
            {
                let bw = bowler(&mut bowling, &delivery.bowler_player_id);
                bw.runs_conceded += charged;
                if let Some(extra) = &delivery.extra {
                    match extra.kind {
                        CricketExtraKind::Wide => bw.wides += 1,
                        CricketExtraKind::NoBall => bw.no_balls += 1,
                        _ => {}
                    }
                }
            }

            // Track runs per bowler-over for maiden detection.
            let key = (delivery.bowler_player_id.clone(), delivery.over);
            if let Some(entry) = over_runs.iter_mut().find(|(k, _)| *k == key) {
                entry.1 += charged;
            } else {
                over_runs.push((key, charged));
            }

            // Extras breakdown.
            if let Some(extra) = &delivery.extra {
                match extra.kind {
                    CricketExtraKind::Bye => extras.byes += extra.runs,
                    CricketExtraKind::LegBye => extras.leg_byes += extra.runs,
                    CricketExtraKind::Wide => extras.wides += extra.runs,
                    CricketExtraKind::NoBall => extras.no_balls += extra.runs,
                    CricketExtraKind::Penalty => extras.penalty += extra.runs,
                }
            }

            // Wicket.
            if let Some(wicket) = &delivery.wicket {
                wickets += 1;
                fall_of_wickets.push(CricketFallOfWicket {
                    wicket: wickets,
                    runs: total_runs,
                    player_id: wicket.dismissed_player_id.clone(),
                    overs: Some(balls_to_overs(legal_balls, balls_per_over)),
                });
                if dismissal_credited_to_bowler(&wicket.kind) {
                    bowler(&mut bowling, &delivery.bowler_player_id).wickets += 1;
                }
                let b = batter(&mut batting, &wicket.dismissed_player_id);
                b.dismissal = Some(CricketDismissal {
                    kind: wicket.kind.clone(),
                    bowler_player_id: wicket.bowler_player_id.clone(),
                    fielder_player_id: wicket.fielder_player_id.clone(),
                });
            }
        }

        // Per-bowler overs and maidens.
        for bw in &mut bowling {
            let balls: u32 = deliveries
                .iter()
                .filter(|d| d.bowler_player_id == bw.player_id && is_legal_delivery(d))
                .count() as u32;
            bw.overs = balls_to_overs(balls, balls_per_over);
            bw.maidens = over_runs
                .iter()
                .filter(|((bowler_id, _), runs)| *bowler_id == bw.player_id && *runs == 0)
                .count() as u32;
        }

        CricketInnings {
            batting_side_id,
            bowling_side_id,
            runs: total_runs,
            wickets,
            overs: balls_to_overs(legal_balls, balls_per_over),
            declared,
            batting,
            bowling,
            extras,
            fall_of_wickets,
            deliveries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain run-scoring delivery (no extra, no wicket).
    fn ball(
        over: u32,
        ball: u32,
        bowler: &str,
        striker: &str,
        non_striker: &str,
        runs: u32,
    ) -> CricketDelivery {
        CricketDelivery {
            over,
            ball,
            bowler_player_id: bowler.to_string(),
            striker_player_id: striker.to_string(),
            non_striker_player_id: non_striker.to_string(),
            runs_off_bat: runs,
            extra: None,
            wicket: None,
        }
    }

    #[test]
    fn aggregates_overview_from_deliveries() {
        // One over from bowler B1: 4, 6, wide(+1), dot, 1, W(bowled striker S1).
        let deliveries = vec![
            ball(0, 1, "B1", "S1", "S2", 4),
            ball(0, 2, "B1", "S1", "S2", 6),
            CricketDelivery {
                over: 0,
                ball: 2,
                bowler_player_id: "B1".into(),
                striker_player_id: "S1".into(),
                non_striker_player_id: "S2".into(),
                runs_off_bat: 0,
                extra: Some(CricketDeliveryExtra {
                    kind: CricketExtraKind::Wide,
                    runs: 1,
                }),
                wicket: None,
            },
            ball(0, 3, "B1", "S1", "S2", 0),
            ball(0, 4, "B1", "S1", "S2", 1),
            CricketDelivery {
                over: 0,
                ball: 5,
                bowler_player_id: "B1".into(),
                striker_player_id: "S1".into(),
                non_striker_player_id: "S2".into(),
                runs_off_bat: 0,
                extra: None,
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "S1".into(),
                    bowler_player_id: Some("B1".into()),
                    fielder_player_id: None,
                }),
            },
        ];

        let innings =
            CricketInnings::from_deliveries("team_a".into(), "team_b".into(), false, deliveries, 6);

        // Total runs: 4 + 6 + 1(wide) + 0 + 1 + 0 = 12.
        assert_eq!(innings.runs, 12);
        assert_eq!(innings.wickets, 1);
        // 5 legal balls (wide excluded) -> 0 overs + 5 balls.
        assert_eq!(innings.overs, Overs { overs: 0, balls: 5 });
        assert_eq!(innings.extras.wides, 1);

        // Striker S1: 11 runs off the bat, faced 5 balls (wide not faced),
        // one four, one six, bowled.
        let s1 = innings
            .batting
            .iter()
            .find(|b| b.player_id == "S1")
            .expect("S1 batting entry");
        assert_eq!(s1.runs, 11);
        assert_eq!(s1.balls_faced, 5);
        assert_eq!(s1.fours, 1);
        assert_eq!(s1.sixes, 1);
        assert!(matches!(
            s1.dismissal.as_ref().map(|d| &d.kind),
            Some(CricketDismissalKind::Bowled)
        ));

        // Bowler B1: all 12 runs charged, 1 wicket, 1 wide, not a maiden.
        let b1 = innings
            .bowling
            .iter()
            .find(|b| b.player_id == "B1")
            .expect("B1 bowling entry");
        assert_eq!(b1.runs_conceded, 12);
        assert_eq!(b1.wickets, 1);
        assert_eq!(b1.wides, 1);
        assert_eq!(b1.maidens, 0);
    }

    #[test]
    fn respects_a_configured_balls_per_over_other_than_six() {
        // The Hundred-style 5-ball over: 5 legal deliveries should complete
        // exactly one over (1 over + 0 balls), not 0 overs + 5 balls as it
        // would under a 6-ball over.
        let deliveries = vec![
            ball(0, 1, "B1", "S1", "S2", 1),
            ball(0, 2, "B1", "S1", "S2", 1),
            ball(0, 3, "B1", "S1", "S2", 1),
            ball(0, 4, "B1", "S1", "S2", 1),
            ball(0, 5, "B1", "S1", "S2", 1),
        ];

        let innings =
            CricketInnings::from_deliveries("team_a".into(), "team_b".into(), false, deliveries, 5);

        assert_eq!(innings.overs, Overs { overs: 1, balls: 0 });
        let b1 = innings
            .bowling
            .iter()
            .find(|b| b.player_id == "B1")
            .expect("B1 bowling entry");
        assert_eq!(b1.overs, Overs { overs: 1, balls: 0 });
    }
}
