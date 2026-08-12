use poem_openapi::{Enum, Object, Union};

use crate::detailed_score::rounders::{
    RECENT_DELIVERIES_LIMIT, RoundersDelivery, RoundersNextBallContext, RoundersPost,
};
use crate::{RoundersBattingEntry, RoundersBowlingEntry, RoundersFallOfOut, RoundersScoreInnings};

/// Rounders live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Rounders`), discriminated by `kind` — same pattern as
/// `CricketLiveEvent`/`FootballLiveEvent`.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum RoundersLiveEvent {
    /// One ball. Reuses `detailed_score::rounders::RoundersDelivery`
    /// verbatim — same reasoning as `CricketLiveEvent::Delivery`.
    Delivery(RoundersDelivery),
    InningsStart(RoundersInningsStartEvent),
    InningsEnd(RoundersInningsEndEvent),
}

#[derive(Object, Clone)]
pub struct RoundersInningsStartEvent {
    pub batting_side_id: String,
    pub fielding_side_id: String,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum RoundersInningsEndReason {
    /// All batting-order slots (the "Batters out" grid on the standard
    /// scoresheet) marked out.
    AllBattersOut,
    /// The innings' good-ball cap reached (the "Good balls" grid) —
    /// no-balls don't count toward it.
    GoodBallsComplete,
    /// A timed variant some leagues play instead of/alongside a ball cap.
    TimeUp,
}

#[derive(Object, Clone)]
pub struct RoundersInningsEndEvent {
    pub reason: RoundersInningsEndReason,
}

fn batting_entry<'a>(
    batting: &'a mut Vec<RoundersBattingEntry>,
    player_id: &str,
) -> &'a mut RoundersBattingEntry {
    if let Some(pos) = batting.iter().position(|b| b.player_id == player_id) {
        &mut batting[pos]
    } else {
        batting.push(RoundersBattingEntry {
            player_id: player_id.to_string(),
            half_rounders: 0,
            balls_faced: 0,
            out: None,
            batting_position: Some(batting.len() as u32 + 1),
        });
        batting.last_mut().unwrap()
    }
}

fn bowling_entry<'a>(
    bowling: &'a mut Vec<RoundersBowlingEntry>,
    player_id: &str,
) -> &'a mut RoundersBowlingEntry {
    if let Some(pos) = bowling.iter().position(|b| b.player_id == player_id) {
        &mut bowling[pos]
    } else {
        bowling.push(RoundersBowlingEntry {
            player_id: player_id.to_string(),
            balls_bowled: 0,
            no_balls: 0,
            outs: 0,
            half_rounders_conceded: 0,
        });
        bowling.last_mut().unwrap()
    }
}

/// Folds one delivery into an innings already in progress — updates the
/// good-ball/bye counters, the batting/bowling cards and fall-of-outs, and
/// every runner's position — and returns the next-ball context that follows
/// it. Same role and shape as cricket's `apply_delivery`: called once per
/// new delivery on the append path, and repeatedly from
/// `RoundersScore::from_events` to bootstrap or recover a `RoundersScore`
/// from the full event log, so the two paths can't disagree.
///
/// Whether a movement reaching the fourth post is a full or half rounder is
/// derived here rather than trusted from the input: `player_id ==
/// d.striker_player_id` means they started this ball fresh from the
/// batting square, so reaching home is a single uninterrupted hit (full,
/// worth 2 half-rounder units); anyone else in `movements` reaching home
/// was already a runner from an earlier delivery, so it necessarily took
/// more than one hit (half, worth 1 unit). A client asserting the wrong
/// one is therefore not possible — same reasoning that keeps cricket's
/// fours/sixes/maidens derived from `runs_off_bat`/`runs_conceded_this_over`
/// rather than stored as flags.
pub fn apply_delivery(
    innings: &mut RoundersScoreInnings,
    context: &RoundersNextBallContext,
    d: &RoundersDelivery,
) -> RoundersNextBallContext {
    if !d.no_ball {
        innings.good_balls_bowled += 1;
    }
    if d.byes {
        innings.byes += 1;
    }

    {
        let bw = bowling_entry(
            innings.bowling.get_or_insert_with(Vec::new),
            &d.bowler_player_id,
        );
        if d.no_ball {
            bw.no_balls += 1;
        } else {
            bw.balls_bowled += 1;
        }
    }

    // Striker: faces a ball (excluding no-balls, same convention as
    // cricket's `balls_faced`) regardless of whether they end up running.
    if !d.no_ball {
        batting_entry(
            innings.batting.get_or_insert_with(Vec::new),
            &d.striker_player_id,
        )
        .balls_faced += 1;
    }

    let mut runners_on_base = context.runners_on_base.clone();

    for m in &d.movements {
        if let Some(out) = &m.out {
            runners_on_base.remove(&m.player_id);
            innings.outs += 1;
            batting_entry(innings.batting.get_or_insert_with(Vec::new), &m.player_id).out =
                Some(out.clone());
            innings
                .fall_of_outs
                .get_or_insert_with(Vec::new)
                .push(RoundersFallOfOut {
                    out_number: innings.outs,
                    half_rounders: innings.half_rounders,
                    player_id: m.player_id.clone(),
                    kind: out.kind,
                });
            bowling_entry(
                innings.bowling.get_or_insert_with(Vec::new),
                &d.bowler_player_id,
            )
            .outs += 1;
            continue;
        }

        match m.post_reached {
            Some(RoundersPost::Fourth) => {
                let half = m.player_id != d.striker_player_id;
                let points = if half { 1 } else { 2 };
                innings.half_rounders += points;
                batting_entry(innings.batting.get_or_insert_with(Vec::new), &m.player_id)
                    .half_rounders += points;
                bowling_entry(
                    innings.bowling.get_or_insert_with(Vec::new),
                    &d.bowler_player_id,
                )
                .half_rounders_conceded += points;
                runners_on_base.remove(&m.player_id);
            }
            Some(post) => {
                runners_on_base.insert(m.player_id.clone(), post);
            }
            None => {
                // Stayed on their existing post (if any), or the striker
                // didn't run at all — nothing to update.
            }
        }
    }

    let mut balls_in_spell = context.balls_in_spell;
    if context.bowler_player_id.as_deref() != Some(d.bowler_player_id.as_str()) {
        balls_in_spell = 0;
    }
    balls_in_spell += 1;

    RoundersNextBallContext {
        runners_on_base,
        bowler_player_id: Some(d.bowler_player_id.clone()),
        balls_in_spell,
    }
}

impl crate::RoundersScore {
    /// Folds the whole event log into a `RoundersScore` from scratch — the
    /// slow path, used to bootstrap a match's first score, recover from a
    /// missing/unparseable cache, or rebuild after undoing the last event.
    /// Just `apply_event` run once per event in order — the fast
    /// (single-event) and slow (whole-log) paths share the exact same fold,
    /// so they can't disagree, same pattern as `CricketScore::from_events`.
    pub fn from_events(events: &[RoundersLiveEvent]) -> Self {
        let mut score = crate::RoundersScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: std::collections::HashMap::new(),
        };
        for event in events {
            score.apply_event(event);
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run on
    /// every append.
    pub fn apply_event(&mut self, event: &RoundersLiveEvent) {
        match event {
            RoundersLiveEvent::InningsStart(start) => {
                self.innings.push(RoundersScoreInnings::opening(
                    start.batting_side_id.clone(),
                    start.fielding_side_id.clone(),
                ));
                self.recent_deliveries = Some(Vec::new());
                self.next_ball_context = Some(RoundersNextBallContext::opening());
                self.awaiting_next_innings = Some(false);
            }
            RoundersLiveEvent::Delivery(d) => {
                let Some(current) = self.innings.last_mut() else {
                    // A delivery with no open innings is malformed input —
                    // there's nothing to fold it into.
                    return;
                };
                let context = self
                    .next_ball_context
                    .clone()
                    .unwrap_or_else(RoundersNextBallContext::opening);
                let next_context = apply_delivery(current, &context, d);
                self.next_ball_context = Some(next_context);

                let deliveries = self.recent_deliveries.get_or_insert_with(Vec::new);
                deliveries.push(d.clone());
                if deliveries.len() > RECENT_DELIVERIES_LIMIT {
                    deliveries.remove(0);
                }
            }
            RoundersLiveEvent::InningsEnd(_) => {
                self.recent_deliveries = None;
                self.next_ball_context = None;
                self.awaiting_next_innings = Some(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detailed_score::rounders::{RoundersOut, RoundersOutKind, RoundersRunnerMovement};

    fn delivery(
        bowler: &str,
        striker: &str,
        movements: Vec<RoundersRunnerMovement>,
    ) -> RoundersDelivery {
        RoundersDelivery {
            bowler_player_id: bowler.into(),
            striker_player_id: striker.into(),
            no_ball: false,
            byes: false,
            movements,
        }
    }

    fn moved(player_id: &str, post: RoundersPost) -> RoundersRunnerMovement {
        RoundersRunnerMovement {
            player_id: player_id.into(),
            post_reached: Some(post),
            out: None,
        }
    }

    fn score(events: &[RoundersLiveEvent]) -> crate::RoundersScore {
        crate::RoundersScore::from_events(events)
    }

    #[test]
    fn a_single_hit_all_the_way_around_is_a_full_rounder() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::Fourth)],
            )),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 1);
        assert_eq!(
            d.innings[0].half_rounders, 2,
            "a full rounder is 2 half-rounder units"
        );
        assert_eq!(d.innings[0].good_balls_bowled, 1);
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(sharma.half_rounders, 2);
        assert!(
            !d.next_ball_context
                .as_ref()
                .unwrap()
                .runners_on_base
                .contains_key("sharma")
        );
    }

    #[test]
    fn reaching_home_over_two_deliveries_is_a_half_rounder() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            // Sharma bats, stops at 2nd.
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::Second)],
            )),
            // Verma bats next; sharma (already a runner) advances home off it.
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "verma",
                vec![moved("sharma", RoundersPost::Fourth)],
            )),
        ];

        let d = score(&events);
        assert_eq!(
            d.innings[0].half_rounders, 1,
            "took two deliveries, so only half a rounder"
        );
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(sharma.half_rounders, 1);
        assert!(
            !d.next_ball_context
                .as_ref()
                .unwrap()
                .runners_on_base
                .contains_key("sharma")
        );
    }

    #[test]
    fn stopping_short_of_home_scores_nothing_yet() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::Second)],
            )),
        ];

        let d = score(&events);
        assert_eq!(d.innings[0].half_rounders, 0);
        assert_eq!(
            d.next_ball_context
                .as_ref()
                .unwrap()
                .runners_on_base
                .get("sharma"),
            Some(&RoundersPost::Second)
        );
    }

    #[test]
    fn an_out_ends_that_batting_order_slot_and_removes_the_runner() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(RoundersDelivery {
                movements: vec![RoundersRunnerMovement {
                    player_id: "sharma".into(),
                    post_reached: None,
                    out: Some(RoundersOut {
                        kind: RoundersOutKind::Caught,
                        fielder_player_id: Some("khan".into()),
                    }),
                }],
                ..delivery("patel", "sharma", vec![])
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings[0].outs, 1);
        assert_eq!(d.innings[0].fall_of_outs.as_ref().unwrap().len(), 1);
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert!(matches!(
            sharma.out.as_ref().map(|o| o.kind),
            Some(RoundersOutKind::Caught)
        ));
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.outs, 1);
    }

    #[test]
    fn the_batting_order_can_lap_the_same_player_faces_more_than_one_turn() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::First)],
            )),
            RoundersLiveEvent::Delivery(delivery("patel", "verma", vec![])),
            // Sharma comes around again for a second turn while still on base.
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::Fourth)],
            )),
        ];

        let d = score(&events);
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(sharma.balls_faced, 2, "faced two separate turns");
        assert_eq!(
            sharma.half_rounders, 2,
            "the second turn's hit is the striker's own single hit home, so it's still full"
        );
    }

    #[test]
    fn no_ball_does_not_count_toward_good_balls_bowled() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(RoundersDelivery {
                no_ball: true,
                ..delivery("patel", "sharma", vec![])
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings[0].good_balls_bowled, 0);
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.no_balls, 1);
        assert_eq!(patel.balls_bowled, 0);
    }

    #[test]
    fn bowling_spell_resets_when_the_bowler_changes() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery("patel", "sharma", vec![])),
            RoundersLiveEvent::Delivery(delivery("patel", "verma", vec![])),
            RoundersLiveEvent::Delivery(delivery("khan", "cole", vec![])),
        ];

        let d = score(&events);
        let ctx = d.next_ball_context.unwrap();
        assert_eq!(ctx.bowler_player_id.as_deref(), Some("khan"));
        assert_eq!(ctx.balls_in_spell, 1);
    }

    #[test]
    fn innings_end_clears_live_context_and_awaits_the_next_innings() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery("patel", "sharma", vec![])),
            RoundersLiveEvent::InningsEnd(RoundersInningsEndEvent {
                reason: RoundersInningsEndReason::AllBattersOut,
            }),
        ];

        let d = score(&events);
        assert!(d.recent_deliveries.is_none());
        assert!(d.next_ball_context.is_none());
        assert_eq!(d.awaiting_next_innings, Some(true));
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            RoundersLiveEvent::InningsStart(RoundersInningsStartEvent {
                batting_side_id: "warriors".into(),
                fielding_side_id: "mill_lane".into(),
            }),
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "sharma",
                vec![moved("sharma", RoundersPost::Second)],
            )),
            RoundersLiveEvent::Delivery(delivery(
                "patel",
                "verma",
                vec![moved("sharma", RoundersPost::Fourth)],
            )),
        ];

        let full = crate::RoundersScore::from_events(&events);

        let mut incremental = crate::RoundersScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: std::collections::HashMap::new(),
        };
        for event in &events {
            incremental.apply_event(event);
        }

        assert_eq!(incremental.innings.len(), full.innings.len());
        assert_eq!(
            incremental.innings[0].half_rounders,
            full.innings[0].half_rounders
        );
        assert_eq!(
            incremental.innings[0].batting.as_ref().unwrap().len(),
            full.innings[0].batting.as_ref().unwrap().len()
        );
    }
}
