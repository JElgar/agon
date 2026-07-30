use poem_openapi::{Enum, Object, Union};

use crate::detailed_score::cricket::{
    CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDismissal,
    CricketDismissalKind, CricketExtras, CricketFallOfWicket, CricketInnings, Overs,
};

/// Cricket live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Cricket`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
#[derive(Union)]
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

#[derive(Object)]
pub struct CricketRetireEvent {
    pub batter_player_id: String,
    /// True: counts as a wicket (fall-of-wickets, team tally) and the batter
    /// does not return. False: "retired hurt" — doesn't touch the wicket
    /// count, and the same `player_id` can simply reappear on a later
    /// delivery to resume batting (no separate "resume" event needed).
    pub retired_out: bool,
}

#[derive(Object)]
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

#[derive(Object)]
pub struct CricketInningsEndEvent {
    pub reason: InningsEndReason,
}

/// The live-scoring state derived by folding a match's cricket event log.
/// One `CricketLiveInnings` per `InningsStart`...`InningsEnd` span (plus a
/// still-open trailing innings, if the match is mid-innings).
#[derive(Object)]
pub struct CricketLiveState {
    pub innings: Vec<CricketLiveInnings>,
    /// True once the log's last innings has an `InningsEnd` and no following
    /// `InningsStart` has opened a new one yet (i.e. between innings, or the
    /// log is empty).
    pub awaiting_next_innings: bool,
}

/// A `detailed_score::cricket::CricketInnings` overview plus the ball-by-ball
/// log it was folded from — everything a live-scoring view needs (the "this
/// over" ball row, striker/non-striker/bowler for the next delivery) that
/// the persisted overview alone can't answer. Exists only in the live
/// snapshot, derived fresh from the event log on each read; nothing this
/// large is ever written back to a single stored record.
#[derive(Object)]
pub struct CricketLiveInnings {
    pub batting_side_id: String,
    pub bowling_side_id: String,
    pub runs: u32,
    pub wickets: u32,
    pub overs: Overs,
    pub declared: bool,
    pub batting: Vec<CricketBattingEntry>,
    pub bowling: Vec<CricketBowlingEntry>,
    pub extras: CricketExtras,
    pub fall_of_wickets: Vec<CricketFallOfWicket>,
    pub deliveries: Vec<CricketDelivery>,
}

struct OpenInnings {
    batting_side_id: String,
    bowling_side_id: String,
    deliveries: Vec<CricketDelivery>,
    retirements: Vec<(String, bool)>,
}

/// Folds an ordered list of events into the derived live state. Callers pass
/// whatever the DAO currently has on record — a deleted event is simply
/// absent from that list, and an amended one shows up with its corrected
/// content, so this never needs to know a correction happened at all.
///
/// Innings boundaries come entirely from `InningsStart`/`InningsEnd` markers,
/// not a stored index — so deleting a wrongly-placed boundary automatically
/// re-flows every event after it back into the earlier innings on the next
/// fold, with nothing to rewrite.
/// `balls_per_over`, `wide_is_extra_ball` and `no_ball_is_extra_ball` are the
/// match's configured over length and extra-ball rules (see
/// `agon_service::match_format::CricketFormat`); callers without a
/// configured format pass the standard defaults (6, true, true).
pub fn derive_state(
    events: &[CricketLiveEvent],
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> CricketLiveState {
    let mut innings: Vec<CricketLiveInnings> = Vec::new();
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

    let awaiting_next_innings = current.is_none();
    if let Some(open) = current {
        innings.push(finish(open, false));
    }

    CricketLiveState {
        innings,
        awaiting_next_innings,
    }
}

fn finish_innings(
    open: OpenInnings,
    declared: bool,
    balls_per_over: u32,
    wide_is_extra_ball: bool,
    no_ball_is_extra_ball: bool,
) -> CricketLiveInnings {
    let overview = CricketInnings::from_deliveries(
        open.batting_side_id,
        open.bowling_side_id,
        declared,
        &open.deliveries,
        balls_per_over,
        wide_is_extra_ball,
        no_ball_is_extra_ball,
    );
    let mut built = CricketLiveInnings {
        batting_side_id: overview.batting_side_id,
        bowling_side_id: overview.bowling_side_id,
        runs: overview.runs,
        wickets: overview.wickets,
        overs: overview.overs,
        declared: overview.declared,
        batting: overview.batting,
        bowling: overview.bowling,
        extras: overview.extras,
        fall_of_wickets: overview.fall_of_wickets,
        deliveries: open.deliveries,
    };
    apply_retirements(&mut built, &open.retirements);
    built
}

/// Annotates the batting card with retirements that never went through a
/// delivery-based dismissal. A later real dismissal (from a delivery) always
/// takes precedence over an earlier retirement note for the same batter.
fn apply_retirements(innings: &mut CricketLiveInnings, retirements: &[(String, bool)]) {
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

        let state = derive_state(&events, 6, true, true);
        assert_eq!(state.innings.len(), 1);
        let innings = &state.innings[0];
        assert_eq!(
            innings.wickets, 0,
            "retiring hurt must not count as a wicket"
        );
        let sharma = innings
            .batting
            .iter()
            .find(|b| b.player_id == "sharma")
            .unwrap();
        assert_eq!(
            sharma.runs, 5,
            "runs from before and after the retirement both count"
        );
        assert!(matches!(
            sharma.dismissal.as_ref().map(|d| &d.kind),
            Some(CricketDismissalKind::RetiredHurt)
        ));
        assert!(
            !state.awaiting_next_innings,
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

        let state = derive_state(&events, 6, true, true);
        let innings = &state.innings[0];
        assert_eq!(innings.wickets, 1);
        assert_eq!(innings.fall_of_wickets.len(), 1);
        assert_eq!(innings.fall_of_wickets[0].player_id, "sharma");
    }

    #[test]
    fn innings_end_and_start_split_deliveries_by_inferred_index_not_a_stored_field() {
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

        let state = derive_state(&events, 6, true, true);
        assert_eq!(state.innings.len(), 2);
        assert_eq!(state.innings[0].batting_side_id, "warriors");
        assert_eq!(state.innings[0].runs, 4);
        assert!(state.innings[0].declared);
        assert_eq!(state.innings[1].batting_side_id, "mill_lane");
        assert_eq!(state.innings[1].runs, 1);
        assert!(
            !state.awaiting_next_innings,
            "second innings is still open (no matching InningsEnd)"
        );
    }

    #[test]
    fn deleting_a_wrongly_placed_innings_end_re_flows_events_into_the_earlier_innings() {
        // A scorer wrongly taps "end innings" after one ball and deletes that
        // event (DELETE /matches/:id/live/events/:seq) rather than voiding
        // it — from derive_state's point of view that's indistinguishable
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

        let state = derive_state(&events, 6, true, true);

        assert_eq!(
            state.innings.len(),
            1,
            "the deleted end must not have split the log into two innings"
        );
        assert_eq!(state.innings[0].runs, 6);
        assert!(!state.awaiting_next_innings);
    }
}
