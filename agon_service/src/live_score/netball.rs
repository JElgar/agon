use std::collections::HashMap;

use poem_openapi::{Object, Union};

use crate::NetballScore;
use crate::detailed_score::netball::{NetballFoulEvent, NetballGoalEvent, NetballPeriod};

/// Netball live-scoring events, nested under the outer sport union
/// (`LiveEventInput::Netball`), discriminated by `kind`. Corrections are
/// handled by directly deleting or amending the stored event (see
/// `DELETE`/`PATCH /matches/:id/live/events/:seq`), not a variant here.
///
/// The same three variants serve both of netball's live-scoring methods —
/// there's no separate vocabulary per method:
///
/// - **Event-by-event**: `Goal`/`Foul` as they happen, plus a `Period`
///   marker at each quarter's end for time-tracking. `NetballScore::score`
///   is folded from `Goal` events; the `Period` marker's own `score` should
///   just agree with that running total (see `NetballPeriodEvent::score`'s
///   doc comment for what happens if it doesn't).
/// - **Quarter-only**: no `Goal`/`Foul` events at all — just a `Period`
///   marker at each quarter's end, each one carrying the score directly.
///   `NetballScore::score` comes *only* from these markers.
#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "kind")]
pub enum NetballLiveEvent {
    Goal(NetballGoalEvent),
    Foul(NetballFoulEvent),
    Period(NetballPeriodEvent),
}

#[derive(Object, Clone)]
pub struct NetballPeriodEvent {
    pub period: NetballPeriod,
    /// Cumulative score per side as of this marker — always present, not
    /// `Option`, unlike `FootballPeriodEvent` (which carries no score at
    /// all, since football's score is always derivable from `Goal` events
    /// alone). This is what lets both of netball's live-scoring methods
    /// share one event vocabulary: for event-by-event scoring this is a
    /// checkpoint that should already agree with the total folded from
    /// `Goal` events; for quarter-only scoring it's the *only* place the
    /// score ever comes from. Either way, `NetballScore::apply_event`
    /// simply overwrites `score` with this value — it doesn't need to know
    /// which method a given match is using.
    pub score: HashMap<String, u32>,
}

impl NetballScore {
    /// Folds the whole event log into a `NetballScore` from scratch — the
    /// slow path, used to bootstrap a match's first score or recover from a
    /// missing/unparseable persisted record. Just `apply_event` run once per
    /// event in order; the fast (single-event) and slow (whole-log) paths
    /// share the exact same fold, so they can't disagree — same pattern as
    /// `FootballScore::from_events`.
    pub fn from_events(events: &[(chrono::DateTime<chrono::Utc>, NetballLiveEvent)]) -> Self {
        let mut score = NetballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            fouls: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            period_scores: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in events {
            score.apply_event(*occurred_at, event);
        }
        score
    }

    /// Folds one new event into this score in place — the fast path, run on
    /// every append. `occurred_at` (not `recorded_at`) is threaded through
    /// separately from `event`, same reasoning as `FootballScore::apply_event`.
    pub fn apply_event(
        &mut self,
        occurred_at: chrono::DateTime<chrono::Utc>,
        event: &NetballLiveEvent,
    ) {
        match event {
            NetballLiveEvent::Goal(g) => {
                *self.score.entry(g.side_id.clone()).or_insert(0) +=
                    if g.two_points { 2 } else { 1 };
                // Stamp with the envelope's own `occurred_at` — the
                // recording device's clock — regardless of whatever (if
                // anything) the client sent in the payload itself. This is
                // what makes `occurred_at`, not the quarter-relative
                // `minute`, the authoritative order for a live-scored goal
                // (see `NetballGoalEvent::occurred_at`'s doc comment).
                let mut g = g.clone();
                g.occurred_at = Some(occurred_at);
                self.goals.get_or_insert_with(Vec::new).push(g);
            }
            NetballLiveEvent::Foul(fo) => {
                let mut fo = fo.clone();
                fo.occurred_at = Some(occurred_at);
                self.fouls.get_or_insert_with(Vec::new).push(fo);
            }
            NetballLiveEvent::Period(p) => {
                self.period_times
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, occurred_at);
                self.period_scores
                    .get_or_insert_with(HashMap::new)
                    .insert(p.period, p.score.clone());
                // The one line that unifies both live-scoring methods — see
                // `NetballPeriodEvent::score`'s doc comment.
                self.score = p.score.clone();
                self.period = Some(p.period);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detailed_score::netball::{NetballFoulKind, NetballPosition};
    use chrono::{DateTime, TimeZone, Utc};

    fn ts(minute: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0).unwrap()
    }

    fn goal(side_id: &str, minute: u32) -> NetballLiveEvent {
        NetballLiveEvent::Goal(NetballGoalEvent {
            side_id: side_id.into(),
            scorer_player_id: Some("shooter_1".into()),
            scorer_position: Some(NetballPosition::GoalShooter),
            two_points: false,
            minute: Some(minute),
            occurred_at: None,
        })
    }

    #[test]
    fn event_by_event_mode_derives_score_from_goals() {
        let events = vec![
            (ts(0), goal("kestrels", 2)),
            (ts(1), goal("kestrels", 5)),
            (ts(2), goal("harriers", 9)),
            (
                ts(3),
                NetballLiveEvent::Foul(NetballFoulEvent {
                    side_id: "harriers".into(),
                    player_id: Some("defender_1".into()),
                    foul_kind: NetballFoulKind::Contact,
                    minute: Some(10),
                    occurred_at: None,
                }),
            ),
            (
                ts(4),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 2),
                        ("harriers".to_string(), 1),
                    ]),
                }),
            ),
        ];

        let score = NetballScore::from_events(&events);

        assert_eq!(score.score.get("kestrels"), Some(&2));
        assert_eq!(score.score.get("harriers"), Some(&1));
        assert_eq!(score.goals.as_ref().unwrap().len(), 3);
        assert_eq!(score.fouls.as_ref().unwrap().len(), 1);
        assert!(matches!(score.period, Some(NetballPeriod::QuarterOneEnd)));
        assert_eq!(
            score
                .period_scores
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&HashMap::from([
                ("kestrels".to_string(), 2),
                ("harriers".to_string(), 1)
            ]))
        );
        assert_eq!(
            score
                .period_times
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&ts(4))
        );
    }

    #[test]
    fn quarter_only_mode_derives_score_purely_from_period_markers() {
        // No Goal/Foul events at all — just quarter-end markers, each
        // carrying the running score directly.
        let events = vec![
            (
                ts(15),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 12),
                        ("harriers".to_string(), 9),
                    ]),
                }),
            ),
            (
                ts(30),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterTwoEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 22),
                        ("harriers".to_string(), 18),
                    ]),
                }),
            ),
        ];

        let score = NetballScore::from_events(&events);

        assert_eq!(score.score.get("kestrels"), Some(&22));
        assert_eq!(score.score.get("harriers"), Some(&18));
        assert!(score.goals.as_ref().unwrap().is_empty());
        assert!(matches!(score.period, Some(NetballPeriod::QuarterTwoEnd)));
        assert_eq!(score.period_scores.as_ref().unwrap().len(), 2);
        // Q1's snapshot survives untouched alongside Q2's.
        assert_eq!(
            score
                .period_scores
                .as_ref()
                .unwrap()
                .get(&NetballPeriod::QuarterOneEnd),
            Some(&HashMap::from([
                ("kestrels".to_string(), 12),
                ("harriers".to_string(), 9)
            ]))
        );
    }

    #[test]
    fn two_point_goals_count_double() {
        let events = vec![(
            ts(0),
            NetballLiveEvent::Goal(NetballGoalEvent {
                side_id: "kestrels".into(),
                scorer_player_id: Some("shooter_1".into()),
                scorer_position: Some(NetballPosition::GoalAttack),
                two_points: true,
                minute: Some(4),
                occurred_at: None,
            }),
        )];

        let score = NetballScore::from_events(&events);
        assert_eq!(score.score.get("kestrels"), Some(&2));
    }

    /// The bug this guards against: `minute` is quarter-relative and reset
    /// every quarter, so two goals in different quarters can carry the same
    /// (or an out-of-order) `minute` — sorting the events list on that alone
    /// scrambles later quarters in with earlier ones. `occurred_at` is
    /// absolute, so folding always produces goals/fouls in true chronological
    /// (recorded) order regardless of what `minute` says, and it's stamped
    /// from the envelope's own clock even when the client sends none.
    #[test]
    fn goals_and_fouls_are_stamped_with_occurred_at_from_the_envelope_in_recorded_order() {
        let events = vec![
            (ts(0), goal("kestrels", 3)),
            (
                ts(1),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([("kestrels".to_string(), 1)]),
                }),
            ),
            // A new quarter's clock resets, so this goal's `minute` (2) is
            // numerically *less* than the first goal's (3) despite happening
            // later in the match — exactly the scrambling `minute`-only
            // sorting produced.
            (ts(2), goal("kestrels", 2)),
        ];

        let score = NetballScore::from_events(&events);
        let goals = score.goals.as_ref().unwrap();
        assert_eq!(goals.len(), 2);
        assert_eq!(goals[0].occurred_at, Some(ts(0)));
        assert_eq!(goals[1].occurred_at, Some(ts(2)));
        // Recorded (array) order already reflects true chronological order,
        // regardless of the quarter-relative `minute` values above.
        assert!(goals[0].occurred_at < goals[1].occurred_at);
    }

    #[test]
    fn incremental_and_full_fold_agree() {
        let events = vec![
            (ts(0), goal("kestrels", 2)),
            (ts(1), goal("harriers", 5)),
            (
                ts(2),
                NetballLiveEvent::Period(NetballPeriodEvent {
                    period: NetballPeriod::QuarterOneEnd,
                    score: HashMap::from([
                        ("kestrels".to_string(), 1),
                        ("harriers".to_string(), 1),
                    ]),
                }),
            ),
        ];

        let full = NetballScore::from_events(&events);

        let mut incremental = NetballScore {
            score: HashMap::new(),
            goals: Some(Vec::new()),
            fouls: Some(Vec::new()),
            period: None,
            period_times: Some(HashMap::new()),
            period_scores: Some(HashMap::new()),
            players: HashMap::new(),
        };
        for (occurred_at, event) in &events {
            incremental.apply_event(*occurred_at, event);
        }

        assert_eq!(incremental.score, full.score);
        assert_eq!(
            incremental.goals.as_ref().unwrap().len(),
            full.goals.as_ref().unwrap().len()
        );
        assert_eq!(incremental.period_times, full.period_times);
        assert_eq!(incremental.period_scores, full.period_scores);
    }
}
