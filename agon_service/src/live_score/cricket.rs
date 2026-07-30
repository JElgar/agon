use poem_openapi::{Enum, Object, Union};

use crate::detailed_score::cricket::{
    CricketDelivery, CricketDismissal, CricketDismissalKind, CricketExtraKind, CricketFallOfWicket,
    CricketInnings, Overs, balls_to_overs, is_legal_delivery,
};

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

/// How many balls back the live summary keeps for the "this over"/recent-balls
/// row — enough to show a bit more than the current over, nowhere near a
/// whole innings.
pub const RECENT_DELIVERIES_LIMIT: usize = 18;

/// One innings' totals — no batting/bowling cards, no ball-by-ball log.
/// Deliberately the same trimmed shape as `main::CricketScoreInnings`
/// (`Score::Cricket`'s confirmed-result equivalent), kept as its own type
/// since the two serve unrelated endpoints that shouldn't need to change
/// together.
#[derive(Object, Clone)]
pub struct CricketInningsTotals {
    pub batting_side_id: String,
    pub bowling_side_id: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: Overs,
    pub declared: bool,
}

/// What's known about who's at the crease/bowling for the *next* delivery,
/// folded incrementally from the current innings' events as they're
/// recorded. A `None` field means the scorer needs to pick someone before
/// the next ball can be recorded — either a fresh innings (nothing bowled
/// yet), a wicket just fell (the dismissed batter's slot is open), or an
/// over just completed (a new bowler is required; the same bowler can't
/// bowl consecutive overs).
///
/// Mirrors `agon_ui/src/lib/cricketScore.ts`'s `NextBallContext` and
/// `nextBallContext` field-for-field and step-for-step — kept in sync by
/// hand. The frontend runs the identical fold offline, against deliveries a
/// device has recorded locally but not yet synced, which is exactly the case
/// this incremental (one delivery at a time) shape is built for: apply one
/// step to the last-known context, don't replay history.
#[derive(Object, Clone)]
pub struct NextBallContext {
    pub striker_player_id: Option<String>,
    pub non_striker_player_id: Option<String>,
    pub bowler_player_id: Option<String>,
    /// 0-based over index the next delivery belongs to.
    pub over: u32,
    /// 1-based ball number within that over.
    pub ball: u32,
    /// The bowler who just finished an over — excluded from the next-bowler
    /// picker. `None` unless a bowler pick is actually needed.
    pub previous_over_bowler_player_id: Option<String>,
}

impl NextBallContext {
    /// The state before any delivery has been recorded in an innings.
    fn opening() -> Self {
        NextBallContext {
            striker_player_id: None,
            non_striker_player_id: None,
            bowler_player_id: None,
            over: 0,
            ball: 1,
            previous_over_bowler_player_id: None,
        }
    }
}

/// Folds one delivery into the running next-ball context. The whole
/// incremental step: called once per new delivery on the append path, and
/// repeatedly from `NextBallContext::opening()` to bootstrap/recover a
/// summary from the full event log (see `CricketLiveState::from_events`).
///
/// Strike rotates on an odd number of the ball's rotating runs (off the bat,
/// or byes/leg-byes — wides/no-balls don't rotate strike) and always at the
/// end of an over — including when one slot is already vacant from a wicket
/// on that same ball (e.g. dismissed on the over's final ball): the swap
/// still applies to whichever slot the survivor occupies, so the vacancy
/// lands in the correct slot for the next ball rather than always defaulting
/// to "striker".
fn apply_delivery_to_context(
    prev: &NextBallContext,
    d: &CricketDelivery,
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> NextBallContext {
    let mut striker = Some(d.striker_player_id.clone());
    let mut non_striker = Some(d.non_striker_player_id.clone());
    let mut bowler = Some(d.bowler_player_id.clone());
    let mut previous_over_bowler: Option<String> = None;
    let mut legal_in_over = prev.ball.saturating_sub(1);
    let mut over = prev.over;

    if let Some(wicket) = &d.wicket {
        if striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            striker = None;
        } else if non_striker.as_deref() == Some(wicket.dismissed_player_id.as_str()) {
            non_striker = None;
        }
    }

    if is_legal_delivery(d, wide_is_extra_ball, no_ball_is_extra_ball) {
        legal_in_over += 1;
        let rotating_runs = d.runs_off_bat
            + match d.extra.as_ref().map(|e| &e.kind) {
                Some(CricketExtraKind::Bye) | Some(CricketExtraKind::LegBye) => {
                    d.extra.as_ref().map(|e| e.runs).unwrap_or(0)
                }
                _ => 0,
            };
        if rotating_runs % 2 == 1 {
            std::mem::swap(&mut striker, &mut non_striker);
        }
        if legal_in_over == balls_per_over {
            std::mem::swap(&mut striker, &mut non_striker);
            previous_over_bowler = bowler.take();
            over += 1;
            legal_in_over = 0;
        }
    }

    NextBallContext {
        striker_player_id: striker,
        non_striker_player_id: non_striker,
        bowler_player_id: bowler,
        over,
        ball: legal_in_over + 1,
        previous_over_bowler_player_id: previous_over_bowler,
    }
}

/// The live-scoring summary, folded from a match's cricket event log — fixed
/// size regardless of how long the match runs or how many events its log
/// holds, so it's safe to cache and update on every single delivery (see
/// `agon_core::dao::live_score_ops`). `innings` totals cover the whole match
/// so far; `recent_deliveries` and `next_ball_context` only ever describe the
/// current (open, or just-finished) innings — a finished match's complete
/// ball-by-ball history reads the raw event log instead (paginated —
/// `GET /matches/:id/live/events`, folded client-side by
/// `inningsDeliveriesFromEvents`).
#[derive(Object, Clone)]
pub struct CricketLiveState {
    pub innings: Vec<CricketInningsTotals>,
    /// The current innings' most recent deliveries, oldest first, capped at
    /// `RECENT_DELIVERIES_LIMIT`. Empty between innings.
    pub recent_deliveries: Vec<CricketDelivery>,
    /// `None` between innings — there's no next ball to give context for.
    pub next_ball_context: Option<NextBallContext>,
    /// True once the log's last innings has an `InningsEnd` and no following
    /// `InningsStart` has opened a new one yet (i.e. between innings, or the
    /// log is empty).
    pub awaiting_next_innings: bool,
}

impl CricketLiveState {
    fn empty() -> Self {
        CricketLiveState {
            innings: Vec::new(),
            recent_deliveries: Vec::new(),
            next_ball_context: None,
            awaiting_next_innings: true,
        }
    }

    /// Folds the whole event log into a summary from scratch — the slow
    /// path, used to bootstrap a match's first summary, recover from a
    /// missing/unparseable cache, or rebuild after a correction (an
    /// arbitrary-position delete/amend can't be applied as a single
    /// incremental step). Just `apply_event` run once per event in order;
    /// the fast (single-event) and slow (whole-log) paths share the exact
    /// same fold, so they can't disagree.
    pub fn from_events(
        events: &[CricketLiveEvent],
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) -> Self {
        let mut state = CricketLiveState::empty();
        for event in events {
            state.apply_event(
                event,
                balls_per_over,
                wide_is_extra_ball,
                no_ball_is_extra_ball,
            );
        }
        state
    }

    /// Folds one new event into this summary in place — the fast path, run
    /// on every append.
    pub fn apply_event(
        &mut self,
        event: &CricketLiveEvent,
        balls_per_over: u32,
        wide_is_extra_ball: bool,
        no_ball_is_extra_ball: bool,
    ) {
        match event {
            CricketLiveEvent::InningsStart(start) => {
                self.innings.push(CricketInningsTotals {
                    batting_side_id: start.batting_side_id.clone(),
                    bowling_side_id: start.bowling_side_id.clone(),
                    runs: 0,
                    wickets: 0,
                    overs: Overs { overs: 0, balls: 0 },
                    declared: false,
                });
                self.recent_deliveries.clear();
                self.next_ball_context = Some(NextBallContext::opening());
                self.awaiting_next_innings = false;
            }
            CricketLiveEvent::Delivery(d) => {
                if let Some(current) = self.innings.last_mut() {
                    let extra_runs = d.extra.as_ref().map(|e| e.runs).unwrap_or(0);
                    current.runs += d.runs_off_bat + extra_runs;
                    if is_legal_delivery(d, wide_is_extra_ball, no_ball_is_extra_ball) {
                        let legal_balls =
                            current.overs.overs * balls_per_over + current.overs.balls + 1;
                        current.overs = balls_to_overs(legal_balls, balls_per_over);
                    }
                    if d.wicket.is_some() {
                        current.wickets += 1;
                    }
                }
                self.recent_deliveries.push(d.clone());
                if self.recent_deliveries.len() > RECENT_DELIVERIES_LIMIT {
                    self.recent_deliveries.remove(0);
                }
                let prev = self
                    .next_ball_context
                    .clone()
                    .unwrap_or_else(NextBallContext::opening);
                self.next_ball_context = Some(apply_delivery_to_context(
                    &prev,
                    d,
                    balls_per_over,
                    wide_is_extra_ball,
                    no_ball_is_extra_ball,
                ));
            }
            CricketLiveEvent::Retire(r) => {
                if r.retired_out
                    && let Some(current) = self.innings.last_mut()
                {
                    current.wickets += 1;
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
                self.recent_deliveries.clear();
                self.next_ball_context = None;
                self.awaiting_next_innings = true;
            }
        }
    }
}

struct OpenInnings {
    batting_side_id: String,
    bowling_side_id: String,
    deliveries: Vec<CricketDelivery>,
    retirements: Vec<(String, bool)>,
}

/// Folds a match's whole cricket event log into its full per-innings
/// scorecard (batting/bowling cards, extras, fall-of-wickets) — the same
/// shape `detailed_score` persists. Internal only: the one place this runs
/// is server-side, once, when a live-scored cricket match is marked
/// complete without an explicit `detailed_score` (see `update_match`), to
/// derive and persist it from the authoritative log. Never exposed on a
/// hot/polled path — `GET /live` serves `CricketLiveState` instead, which
/// carries totals only plus a bounded recent-deliveries window.
///
/// Innings boundaries come entirely from `InningsStart`/`InningsEnd`
/// markers, not a stored index — so deleting a wrongly-placed boundary
/// automatically re-flows every event after it back into the earlier
/// innings on the next fold, with nothing to rewrite.
/// `balls_per_over`, `wide_is_extra_ball` and `no_ball_is_extra_ball` are the
/// match's configured over length and extra-ball rules (see
/// `agon_service::match_format::CricketFormat`); callers without a
/// configured format pass the standard defaults (6, true, true).
pub fn fold_full_scorecard(
    events: &[CricketLiveEvent],
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> Vec<CricketInnings> {
    let mut innings: Vec<CricketInnings> = Vec::new();
    let mut current: Option<OpenInnings> = None;

    let finish = |open: OpenInnings, declared: bool| {
        finish_innings(
            open,
            declared,
            balls_per_over,
            wide_is_extra_ball,
            no_ball_is_extra_ball,
        )
    };

    for event in events {
        match event {
            CricketLiveEvent::InningsStart(start) => {
                // Close out anything left open without an explicit End,
                // rather than losing it — shouldn't normally happen.
                if let Some(open) = current.take() {
                    innings.push(finish(open, false));
                }
                current = Some(OpenInnings {
                    batting_side_id: start.batting_side_id.clone(),
                    bowling_side_id: start.bowling_side_id.clone(),
                    deliveries: Vec::new(),
                    retirements: Vec::new(),
                });
            }
            CricketLiveEvent::Delivery(d) => {
                if let Some(open) = &mut current {
                    open.deliveries.push(d.clone());
                }
            }
            CricketLiveEvent::Retire(r) => {
                if let Some(open) = &mut current {
                    open.retirements
                        .push((r.batter_player_id.clone(), r.retired_out));
                }
            }
            CricketLiveEvent::InningsEnd(end) => {
                if let Some(open) = current.take() {
                    let declared = matches!(end.reason, InningsEndReason::Declared);
                    innings.push(finish(open, declared));
                }
            }
        }
    }

    if let Some(open) = current {
        innings.push(finish(open, false));
    }

    innings
}

fn finish_innings(
    open: OpenInnings,
    declared: bool,
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> CricketInnings {
    let mut built = CricketInnings::from_deliveries(
        open.batting_side_id,
        open.bowling_side_id,
        declared,
        &open.deliveries,
        balls_per_over,
        wide_is_extra_ball,
        no_ball_is_extra_ball,
    );
    apply_retirements(&mut built, &open.retirements);
    built
}

/// Annotates the batting card with retirements that never went through a
/// delivery-based dismissal. A later real dismissal (from a delivery) always
/// takes precedence over an earlier retirement note for the same batter.
fn apply_retirements(innings: &mut CricketInnings, retirements: &[(String, bool)]) {
    for (player_id, retired_out) in retirements {
        let Some(entry) = innings
            .batting
            .iter_mut()
            .find(|b| &b.player_id == player_id)
        else {
            // The batter retired without ever facing a ball (e.g. injured
            // before their first delivery) — no batting-card row exists yet
            // to annotate. Rare enough in practice not to synthesize one.
            continue;
        };
        if entry.dismissal.is_some() {
            continue;
        }
        entry.dismissal = Some(CricketDismissal {
            kind: if *retired_out {
                CricketDismissalKind::RetiredOut
            } else {
                CricketDismissalKind::RetiredHurt
            },
            bowler_player_id: None,
            fielder_player_id: None,
        });
        if *retired_out {
            innings.wickets += 1;
            innings.fall_of_wickets.push(CricketFallOfWicket {
                wicket: innings.wickets,
                runs: innings.runs,
                player_id: player_id.clone(),
                overs: Some(innings.overs),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    fn summary(events: &[CricketLiveEvent]) -> CricketLiveState {
        CricketLiveState::from_events(events, 6, true, true)
    }

    #[test]
    fn tracks_runs_wickets_overs_per_innings() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Delivery(CricketDelivery {
                wicket: Some(crate::detailed_score::cricket::CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];

        let state = summary(&events);
        assert_eq!(state.innings.len(), 1);
        assert_eq!(state.innings[0].runs, 4);
        assert_eq!(state.innings[0].wickets, 1);
        assert_eq!(state.innings[0].overs, Overs { overs: 0, balls: 2 });
        assert!(!state.awaiting_next_innings);
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
        let state = summary(&events);
        assert_eq!(state.recent_deliveries.len(), RECENT_DELIVERIES_LIMIT);
        assert_eq!(state.innings[0].runs, RECENT_DELIVERIES_LIMIT as u32 + 5);

        events.push(CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
            reason: InningsEndReason::Declared,
        }));
        let state = summary(&events);
        assert!(state.recent_deliveries.is_empty());
        assert!(state.next_ball_context.is_none());
        assert!(state.awaiting_next_innings);
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
        let ctx = summary(&events).next_ball_context.unwrap();
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
        let ctx = summary(&all_events).next_ball_context.unwrap();
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
                wicket: Some(crate::detailed_score::cricket::CricketDeliveryWicket {
                    kind: CricketDismissalKind::Bowled,
                    dismissed_player_id: "sharma".into(),
                    bowler_player_id: Some("patel".into()),
                    fielder_player_id: None,
                }),
                ..ball("patel", "sharma", "verma", 0)
            }),
        ];
        let ctx = summary(&events).next_ball_context.unwrap();
        assert!(ctx.striker_player_id.is_none());
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("verma"));
    }

    #[test]
    fn retiring_hurt_vacates_the_slot_without_a_wicket() {
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
        ];
        let state = summary(&events);
        assert_eq!(state.innings[0].wickets, 0);
        let ctx = state.next_ball_context.unwrap();
        assert!(ctx.striker_player_id.is_none());
        assert_eq!(ctx.non_striker_player_id.as_deref(), Some("verma"));
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
        let state = summary(&events);
        assert_eq!(state.innings[0].wickets, 1);
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

        let state = summary(&events);
        assert_eq!(state.innings.len(), 2);
        assert_eq!(state.innings[0].batting_side_id, "warriors");
        assert_eq!(state.innings[0].runs, 4);
        assert!(state.innings[0].declared);
        assert_eq!(state.innings[1].batting_side_id, "mill_lane");
        assert_eq!(state.innings[1].runs, 1);
        assert!(!state.awaiting_next_innings);
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

        let state = summary(&events);
        assert_eq!(
            state.innings.len(),
            1,
            "the deleted end must not have split the log into two innings"
        );
        assert_eq!(state.innings[0].runs, 6);
        assert!(!state.awaiting_next_innings);
    }

    #[test]
    fn fold_full_scorecard_produces_batting_and_bowling_cards() {
        let events = vec![
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: "warriors".into(),
                bowling_side_id: "mill_lane".into(),
            }),
            CricketLiveEvent::Delivery(ball("patel", "sharma", "verma", 4)),
            CricketLiveEvent::Retire(CricketRetireEvent {
                batter_player_id: "sharma".into(),
                retired_out: true,
            }),
        ];
        let innings = fold_full_scorecard(&events, 6, true, true);
        assert_eq!(innings.len(), 1);
        assert_eq!(innings[0].wickets, 1);
        let sharma = innings[0]
            .batting
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert!(matches!(
            sharma.dismissal.as_ref().map(|d| &d.kind),
            Some(CricketDismissalKind::RetiredOut)
        ));
    }
}
