use std::collections::HashMap;

use poem_openapi::{Enum, Object, Union};

use crate::detailed_score::cricket::{
    CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDismissal,
    CricketDismissalKind, CricketExtraKind, CricketExtras, CricketFallOfWicket, NextBallContext,
    Overs, RECENT_DELIVERIES_LIMIT, balls_to_overs, dismissal_credited_to_bowler,
    is_legal_delivery, runs_charged_to_bowler,
};
use crate::{CricketScore, CricketScoreInnings};

/// Cricket live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Cricket`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum CricketLiveEvent {
    /// One ball. Reuses `detailed_score::cricket::CricketDelivery` verbatim —
    /// a live innings' ball-by-ball log *is* that log, built up one delivery
    /// at a time instead of submitted whole at the end.
    Delivery(CricketDelivery),
    Retire(CricketRetireEvent),
    InningsStart(CricketInningsStartEvent),
    InningsEnd(CricketInningsEndEvent),
}

#[derive(Object, Clone)]
pub struct CricketRetireEvent {
    pub batter_player_id: String,
    /// True: counts as a wicket (fall-of-wickets, team tally) and the batter
    /// does not return. False: "retired hurt" — doesn't touch the wicket
    /// count, and the same `player_id` can simply reappear on a later
    /// delivery to resume batting (no separate "resume" event needed).
    pub retired_out: bool,
}

#[derive(Object, Clone)]
pub struct CricketInningsStartEvent {
    pub batting_side_id: String,
    pub bowling_side_id: String,
}

#[derive(Enum, Clone)]
#[oai(rename_all = "snake_case")]
pub enum InningsEndReason {
    AllOut,
    OversComplete,
    Declared,
    TargetReached,
}

#[derive(Object, Clone)]
pub struct CricketInningsEndEvent {
    pub reason: InningsEndReason,
}

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

/// Folds one delivery into an innings already in progress — updates totals,
/// the batting/bowling cards, extras, and fall-of-wickets in place — and
/// returns the next-ball context that follows it. The whole incremental
/// step: called once per new delivery on the append path, and repeatedly
/// from `CricketScoreInnings::opening`/`NextBallContext::opening` to
/// bootstrap or recover a `CricketScore` from the full event log (see
/// `CricketScore::from_events`) — one implementation either way, so the two
/// paths can't disagree. The card fields (`batting`/`bowling`/`extras`/
/// `fall_of_wickets`) are always `Some` by the time this runs — `opening()`
/// populates them empty rather than `None` — but every access still goes
/// through `get_or_insert_with` defensively, since the same fields are
/// `None` for a bare manually-entered result and it costs nothing here to
/// not assume otherwise.
///
/// Strike rotates on an odd number of the ball's rotating runs (off the bat,
/// or byes/leg-byes — wides/no-balls don't rotate strike) and always at the
/// end of an over — including when one slot is already vacant from a wicket
/// on that same ball (e.g. dismissed on the over's final ball): the swap
/// still applies to whichever slot the survivor occupies, so the vacancy
/// lands in the correct slot for the next ball rather than always defaulting
/// to "striker". A maiden is credited to whoever bowled the over that just
/// completed, using `runs_conceded_this_over` — tracked alongside the
/// context rather than recomputed by scanning deliveries.
pub fn apply_delivery(
    innings: &mut CricketScoreInnings,
    context: &NextBallContext,
    d: &CricketDelivery,
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> NextBallContext {
    let legal = is_legal_delivery(d, wide_is_extra_ball, no_ball_is_extra_ball);
    let charged = runs_charged_to_bowler(d);
    let extra_runs = d.extra.as_ref().map(|e| e.runs).unwrap_or(0);

    innings.runs += d.runs_off_bat + extra_runs;
    if legal {
        let legal_balls = innings.overs.overs * balls_per_over + innings.overs.balls + 1;
        innings.overs = balls_to_overs(legal_balls, balls_per_over);
    }

    // Batting: striker is credited runs off the bat and faces legal balls
    // and no-balls (but not wides).
    {
        let b = batter(
            innings.batting.get_or_insert_with(Vec::new),
            &d.striker_player_id,
        );
        b.runs += d.runs_off_bat;
        let faced_ball = !matches!(
            d.extra.as_ref().map(|e| &e.kind),
            Some(CricketExtraKind::Wide)
        );
        if faced_ball {
            b.balls_faced += 1;
        }
        match d.runs_off_bat {
            4 => b.fours += 1,
            6 => b.sixes += 1,
            _ => {}
        }
    }

    // Bowling figures.
    {
        let bw = bowler(
            innings.bowling.get_or_insert_with(Vec::new),
            &d.bowler_player_id,
        );
        bw.runs_conceded += charged;
        if let Some(extra) = &d.extra {
            match extra.kind {
                CricketExtraKind::Wide => bw.wides += 1,
                CricketExtraKind::NoBall => bw.no_balls += 1,
                _ => {}
            }
        }
        if legal {
            let legal_balls = bw.overs.overs * balls_per_over + bw.overs.balls + 1;
            bw.overs = balls_to_overs(legal_balls, balls_per_over);
        }
    }

    // Extras breakdown.
    if let Some(extra) = &d.extra {
        let extras = innings.extras.get_or_insert_with(CricketExtras::default);
        match extra.kind {
            CricketExtraKind::Bye => extras.byes += extra.runs,
            CricketExtraKind::LegBye => extras.leg_byes += extra.runs,
            CricketExtraKind::Wide => extras.wides += extra.runs,
            CricketExtraKind::NoBall => extras.no_balls += extra.runs,
            CricketExtraKind::Penalty => extras.penalty += extra.runs,
        }
    }

    // Wicket.
    if let Some(wicket) = &d.wicket {
        innings.wickets += 1;
        innings
            .fall_of_wickets
            .get_or_insert_with(Vec::new)
            .push(CricketFallOfWicket {
                wicket: innings.wickets,
                runs: innings.runs,
                player_id: wicket.dismissed_player_id.clone(),
                overs: Some(innings.overs),
            });
        if dismissal_credited_to_bowler(&wicket.kind) {
            bowler(
                innings.bowling.get_or_insert_with(Vec::new),
                &d.bowler_player_id,
            )
            .wickets += 1;
        }
        let b = batter(
            innings.batting.get_or_insert_with(Vec::new),
            &wicket.dismissed_player_id,
        );
        b.dismissal = Some(CricketDismissal {
            kind: wicket.kind.clone(),
            bowler_player_id: wicket.bowler_player_id.clone(),
            fielder_player_id: wicket.fielder_player_id.clone(),
        });
    }

    // Next-ball context, folded from the previous one.
    let mut striker = Some(d.striker_player_id.clone());
    let mut non_striker = Some(d.non_striker_player_id.clone());
    let mut bowler_id = Some(d.bowler_player_id.clone());
    let mut previous_over_bowler: Option<String> = None;
    let mut legal_in_over = context.ball.saturating_sub(1);
    let mut over = context.over;
    let mut runs_conceded_this_over = context.runs_conceded_this_over + charged;

    if let Some(wicket) = &d.wicket {
        if striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            striker = None;
        } else if non_striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            non_striker = None;
        }
    }

    if legal {
        legal_in_over += 1;
        let rotating_runs = d.runs_off_bat
            + match d.extra.as_ref().map(|e| &e.kind) {
                Some(CricketExtraKind::Bye) | Some(CricketExtraKind::LegBye) => extra_runs,
                _ => 0,
            };
        if rotating_runs % 2 == 1 {
            std::mem::swap(&mut striker, &mut non_striker);
        }
        if legal_in_over == balls_per_over {
            std::mem::swap(&mut striker, &mut non_striker);
            if runs_conceded_this_over == 0
                && let Some(over_bowler_id) = &bowler_id
            {
                bowler(innings.bowling.get_or_insert_with(Vec::new), over_bowler_id).maidens += 1;
            }
            previous_over_bowler = bowler_id.take();
            over += 1;
            legal_in_over = 0;
            runs_conceded_this_over = 0;
        }
    }

    NextBallContext {
        striker_player_id: striker,
        non_striker_player_id: non_striker,
        bowler_player_id: bowler_id,
        over,
        ball: legal_in_over + 1,
        previous_over_bowler_player_id: previous_over_bowler,
        runs_conceded_this_over,
    }
}

impl CricketScore {
    /// Folds the whole event log into a `CricketScore` from scratch — the
    /// slow path, used to bootstrap a match's first score, recover from a
    /// missing/unparseable cache, or rebuild after undoing the last event.
    /// Just `apply_event` run once per event in order; the fast
    /// (single-event) and slow (whole-log) paths share the exact same fold,
    /// so they can't disagree.
    pub fn from_events(
        events: &[(chrono::DateTime<chrono::Utc>, CricketLiveEvent)],
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) -> Self {
        let mut score = CricketScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(
                *occurred_at,
                event,
                balls_per_over,
                wide_is_extra_ball,
                no_ball_is_extra_ball,
            );
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run
    /// on every append. `occurred_at` (not `recorded_at`) is threaded
    /// through separately from `event`, same reasoning as
    /// `FootballScore::apply_event` — only `Delivery` reads it (see
    /// `CricketDelivery::occurred_at`'s doc comment); every other variant
    /// ignores it, same as before this parameter existed.
    pub fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &CricketLiveEvent,
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) {
        match event {
            CricketLiveEvent::InningsStart(start) => {
                self.innings.push(CricketScoreInnings::opening(
                    start.batting_side_id.clone(),
                    start.bowling_side_id.clone(),
                ));
                self.recent_deliveries = Some(Vec::new());
                self.next_ball_context = Some(NextBallContext::opening());
                self.awaiting_next_innings = Some(false);
            }
            CricketLiveEvent::Delivery(d) => {
                let Some(current) = self.innings.last_mut() else {
                    // A delivery with no open innings is malformed input —
                    // there's nothing to fold it into.
                    return;
                };
                let context = self
                    .next_ball_context
                    .clone()
                    .unwrap_or_else(NextBallContext::opening);
                let next_context = apply_delivery(
                    current,
                    &context,
                    d,
                    balls_per_over,
                    wide_is_extra_ball,
                    no_ball_is_extra_ball,
                );
                self.next_ball_context = Some(next_context);

                // Stamp with the envelope's own `occurred_at` before storing
                // — see `CricketDelivery::occurred_at`'s doc comment. Stats
                // are already folded above off the un-stamped `d`, so this
                // only affects what gets stored/returned.
                let mut d = d.clone();
                d.occurred_at = Some(occurred_at);
                let deliveries = self.recent_deliveries.get_or_insert_with(Vec::new);
                deliveries.push(d);
                if deliveries.len() > RECENT_DELIVERIES_LIMIT {
                    deliveries.remove(0);
                }
            }
            CricketLiveEvent::Retire(r) => {
                let Some(current) = self.innings.last_mut() else {
                    return;
                };
                let Some(entry) = current
                    .batting
                    .get_or_insert_with(Vec::new)
                    .iter_mut()
                    .find(|b| b.player_id == r.batter_player_id)
                else {
                    // Retired without ever facing a ball — no batting-card
                    // row exists yet to annotate. Rare enough not to
                    // synthesize one.
                    return;
                };
                if entry.dismissal.is_some() {
                    // A later real dismissal (from a delivery) always takes
                    // precedence over an earlier retirement note — and by
                    // the time we're processing events in order, that's
                    // already reflected here.
                    return;
                }
                entry.dismissal = Some(CricketDismissal {
                    kind: if r.retired_out {
                        CricketDismissalKind::RetiredOut
                    } else {
                        CricketDismissalKind::RetiredHurt
                    },
                    bowler_player_id: None,
                    fielder_player_id: None,
                });
                if r.retired_out {
                    current.wickets += 1;
                    current.fall_of_wickets.get_or_insert_with(Vec::new).push(
                        CricketFallOfWicket {
                            wicket: current.wickets,
                            runs: current.runs,
                            player_id: r.batter_player_id.clone(),
                            overs: Some(current.overs),
                        },
                    );
                }
                if let Some(ctx) = &mut self.next_ball_context {
                    if ctx.striker_player_id.as_deref() == Some(r.batter_player_id.as_str()) {
                        ctx.striker_player_id = None;
                    } else if ctx.non_striker_player_id.as_deref()
                        == Some(r.batter_player_id.as_str())
                    {
                        ctx.non_striker_player_id = None;
                    }
                }
            }
            CricketLiveEvent::InningsEnd(end) => {
                if let Some(current) = self.innings.last_mut() {
                    current.declared = matches!(end.reason, InningsEndReason::Declared);
                }
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
    use crate::detailed_score::cricket::CricketDeliveryWicket;
    use chrono::{DateTime, TimeZone, Utc};

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
    }

    fn ball(bowler: &str, striker: &str, non_striker: &str, runs: u32) -> CricketDelivery {
        CricketDelivery {
            over: 0,
            ball: 1,
            bowler_player_id: bowler.into(),
            striker_player_id: striker.into(),
            non_striker_player_id: non_striker.into(),
            runs_off_bat: runs,
            extra: None,
            wicket: None,
            occurred_at: None,
        }
    }

    /// Most tests only care about the fold's result, not real timestamps —
    /// this pairs each event with a synthetic, strictly increasing one so
    /// `from_events` (which now always wants a timestamp — see
    /// `CricketScore::apply_event`) has something to thread through.
    fn score(events: &[CricketLiveEvent]) -> CricketScore {
        let timed: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(i, e)| (ts(i as i64), e.clone()))
            .collect();
        CricketScore::from_events(&timed, 6, true, true)
    }

    #[test]
    fn deliveries_are_stamped_with_occurred_at_from_the_envelope() {
        let events = vec![
            (
                ts(0),
                CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                    batting_side_id: "warriors".into(),
                    bowling_side_id: "mill_lane".into(),
                }),
            ),
            (
                ts(1),
                CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            ),
        ];

        let d = CricketScore::from_events(&events, 6, true, true);

        assert_eq!(
            d.recent_deliveries.as_ref().unwrap()[0].occurred_at,
            Some(ts(1))
        );
    }

    #[test]
    fn tracks_runs_wickets_overs_and_cards_per_innings() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 1);
        assert_eq!(d.innings[0].runs, 4);
        assert_eq!(d.innings[0].wickets, 1);
        assert_eq!(d.innings[0].overs, Overs { overs: 0, balls: 2 });
        assert_eq!(d.awaiting_next_innings, Some(false));

        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(sharma.runs, 4);
        assert!(matches!(
            sharma.dismissal.as_ref().map(|w| &w.kind),
            Some(CricketDismissalKind::Bowled)
        ));

        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.runs_conceded, 4);
        assert_eq!(patel.wickets, 1);
    }

    #[test]
    fn a_wicketless_over_is_a_maiden() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        for _ in 0..6 {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "sharma", "verma", 0,
            )));
        }
        let d = score(&events);
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.maidens, 1);
        assert_eq!(patel.overs, Overs { overs: 1, balls: 0 });
    }

    #[test]
    fn a_single_run_breaks_the_maiden() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        events.push(CricketLiveEvent::Delivery(ball(
            "patel", "sharma", "verma", 1,
        )));
        for _ in 0..5 {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "verma", "sharma", 0,
            )));
        }
        let d = score(&events);
        let patel = d.innings[0]
            .bowling
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "patel")
            .unwrap();
        assert_eq!(patel.maidens, 0);
    }

    #[test]
    fn recent_deliveries_is_capped_and_cleared_between_innings() {
        let mut events = vec![CricketLiveEvent::InningsStart(CricketInningsStartEvent {
            batting_side_id: "warriors".into(),
            bowling_side_id: "mill_lane".into(),
        })];
        for _ in 0..(RECENT_DELIVERIES_LIMIT + 5) {
            events.push(CricketLiveEvent::Delivery(ball(
                "patel", "sharma", "verma", 1,
            )));
        }
        let d = score(&events);
        assert_eq!(
            d.recent_deliveries.as_ref().map(Vec::len),
            Some(RECENT_DELIVERIES_LIMIT)
        );
        assert_eq!(d.innings[0].runs, RECENT_DELIVERIES_LIMIT as u32 + 5);

        events.push(CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
            reason: InningsEndReason::Declared,
        }));
        let d = score(&events);
        assert!(d.recent_deliveries.is_none());
        assert!(d.next_ball_context.is_none());
        assert_eq!(d.awaiting_next_innings, Some(true));
    }

    #[test]
    fn next_ball_context_rotates_strike_on_odd_runs_and_over_boundary() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
        ];
        let ctx = score(&events).next_ball_context.unwrap();
        assert_eq!(ctx.striker_player_id.as_deref(), Some("verma"));
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("sharma"));
        assert_eq!(ctx.bowler_player_id.as_deref(), Some("patel"));
        assert_eq!(ctx.ball, 2);
        assert_eq!(ctx.over, 0);

        let mut all_events = events;
        for _ in 0..5 {
            all_events.push(CricketLiveEvent::Delivery(ball(
                "patel", "verma", "sharma", 0,
            )));
        }
        let ctx = score(&all_events).next_ball_context.unwrap();
        assert_eq!(
            ctx.over, 1,
            "over completes after 6 legal balls (with the standard 6-ball-over default)"
        );
        assert_eq!(ctx.ball, 1);
        assert_eq!(
            ctx.previous_over_bowler_player_id.as_deref(),
            Some("patel"),
            "the over's bowler can't be picked again for the next one"
        );
        assert!(
            ctx.bowler_player_id.is_none(),
            "a new bowler must be picked for the next over"
        );
    }

    #[test]
    fn a_wicket_vacates_the_dismissed_batters_slot() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];
        let ctx = score(&events).next_ball_context.unwrap();
        assert!(ctx.striker_player_id.is_none());
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("verma"));
    }

    #[test]
    fn retiring_hurt_lets_the_same_batter_resume_without_a_wicket() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: false,
            }),
            // Sharma comes back and faces another ball — same player_id, no
            // extra event needed for the "resume" side of things.
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 1);
        assert_eq!(
            d.innings[0].wickets, 0,
            "retiring hurt must not count as a wicket"
        );
        let sharma = d.innings[0]
            .batting
            .as_ref()
            .unwrap()
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(
            sharma.runs, 5,
            "runs from before and after the retirement both count"
        );
        assert!(matches!(
            sharma.dismissal.as_ref().map(|w| &w.kind),
            Some(CricketDismissalKind::RetiredHurt)
        ));
        assert_eq!(
            d.awaiting_next_innings,
            Some(false),
            "the innings is still open (no InningsEnd), so we're not \"awaiting\" the next one"
        );
    }

    #[test]
    fn retiring_out_counts_as_a_wicket() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 2)),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: true,
            }),
        ];

        let d = score(&events);
        assert_eq!(d.innings[0].wickets, 1);
        assert_eq!(d.innings[0].fall_of_wickets.as_ref().unwrap().len(), 1);
        assert_eq!(
            d.innings[0].fall_of_wickets.as_ref().unwrap()[0].player_id,
            "sharma"
        );
    }

    #[test]
    fn innings_end_and_start_split_totals_by_inferred_index_not_a_stored_field() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: InningsEndReason::Declared,
            }),
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "mill_lane".into(),
                bowling_side_id: "warriors".into(),
            }),
            CricketLiveEvent::Delivery(ball("sharma", "cole", "adeyemi", 1)),
        ];

        let d = score(&events);
        assert_eq!(d.innings.len(), 2);
        assert_eq!(d.innings[0].batting_side_id, "warriors");
        assert_eq!(d.innings[0].runs, 4);
        assert!(d.innings[0].declared);
        assert_eq!(d.innings[1].batting_side_id, "mill_lane");
        assert_eq!(d.innings[1].runs, 1);
        assert_eq!(d.awaiting_next_innings, Some(false));
    }

    #[test]
    fn deleting_a_wrongly_placed_innings_end_re_flows_events_into_the_earlier_innings() {
        // A scorer wrongly taps "end innings" after one ball and deletes that
        // event (DELETE /matches/:id/live/events/:seq) rather than voiding
        // it — from a full refold's point of view that's indistinguishable
        // from it never having been recorded: the deleted event is just
        // absent from the list it's given.
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            // The wrongly-placed InningsEnd has already been deleted, so it
            // never appears here at all.
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 2)),
        ];

        let d = score(&events);
        assert_eq!(
            d.innings.len(),
            1,
            "the deleted end must not have split the log into two innings"
        );
        assert_eq!(d.innings[0].runs, 6);
        assert_eq!(d.awaiting_next_innings, Some(false));
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 1)),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(CricketDeliveryWicket {
                    kind: CricketDismissalKind::Caught,
                    dismissed_player_id: "verma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: Some("khan".into()),
                }),
                ..ball("patel", "verma", "sharma", 0)
            }),
        ];
        let events: Vec<_> = events
            .into_iter()
            .enumerate()
            .map(|(i, e)| (ts(i as i64), e))
            .collect();

        let full = CricketScore::from_events(&events, 6, true, true);

        // Apply the same events one at a time, incrementally, and check the
        // final state matches the full fold exactly.
        let mut incremental = CricketScore {
            innings: Vec::new(),
            recent_deliveries: None,
            next_ball_context: None,
            awaiting_next_innings: Some(true),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event, 6, true, true);
        }

        assert_eq!(incremental.innings.len(), full.innings.len());
        assert_eq!(incremental.innings[0].runs, full.innings[0].runs);
        assert_eq!(incremental.innings[0].wickets, full.innings[0].wickets);
        assert_eq!(
            incremental.innings[0].batting.as_ref().unwrap().len(),
            full.innings[0].batting.as_ref().unwrap().len()
        );
        assert_eq!(
            incremental.innings[0].bowling.as_ref().unwrap().len(),
            full.innings[0].bowling.as_ref().unwrap().len()
        );
    }
}
