//! League standings, computed on read from a tournament's generated matches
//! — never denormalized onto the tournament or entry records, so they can't
//! drift from the matches themselves. Shared by `GET
//! /tournaments/{id}/standings` and `GET /teams/{id}/competitions` (a team's
//! position/points in each league it's in), so the two can never disagree.
//!
//! [`league_standings`] adapts `MatchRecord`s; [`tally_standings`] is the
//! pure sum, unit-tested without building whole match records.

use std::collections::HashMap;

use crate::dao::records::{MatchRecord, TournamentRecord};

/// One entrant's (team's) standing in a league.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingsRow {
    pub entrant_id: String,
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
    pub points: u32,
}

/// Points per result — `TournamentRecord::points_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointsRule {
    pub win: u32,
    pub draw: u32,
    pub loss: u32,
}

/// One confirmed match, reduced to what standings need: each team-linked
/// side as `(side_id, team_id)`, and the winning side (`None` = a draw).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedResult {
    pub sides: Vec<(String, String)>,
    pub winner_side_id: Option<String>,
}

/// Standings for a league over its generated matches. Every confirmed match
/// counts; unconfirmed ones are skipped. Sides without a team are ignored
/// (every tournament side is team-linked, so none are in practice).
pub fn league_standings(
    tournament: &TournamentRecord,
    matches: &[MatchRecord],
) -> Vec<StandingsRow> {
    let results = matches.iter().filter_map(|m| {
        let confirmed = m.confirmed_score.as_ref()?;
        Some(ConfirmedResult {
            sides: m
                .sides
                .values()
                .filter_map(|s| s.team_id.as_ref().map(|t| (s.side_id.clone(), t.clone())))
                .collect(),
            winner_side_id: confirmed.winner_side_id.clone(),
        })
    });
    tally_standings(
        PointsRule {
            win: tournament.points_win,
            draw: tournament.points_draw,
            loss: tournament.points_loss,
        },
        results,
    )
}

/// Sum results into rows, ordered by points descending, then entrant id
/// (so the order — and therefore a team's position — is deterministic).
pub fn tally_standings(
    points: PointsRule,
    results: impl IntoIterator<Item = ConfirmedResult>,
) -> Vec<StandingsRow> {
    let mut rows: HashMap<String, StandingsRow> = HashMap::new();
    for r in results {
        for (side_id, team_id) in &r.sides {
            let row = rows.entry(team_id.clone()).or_insert_with(|| StandingsRow {
                entrant_id: team_id.clone(),
                played: 0,
                won: 0,
                drawn: 0,
                lost: 0,
                points: 0,
            });
            row.played += 1;
            match &r.winner_side_id {
                Some(w) if w == side_id => {
                    row.won += 1;
                    row.points += points.win;
                }
                Some(_) => {
                    row.lost += 1;
                    row.points += points.loss;
                }
                None => {
                    row.drawn += 1;
                    row.points += points.draw;
                }
            }
        }
    }
    let mut rows: Vec<StandingsRow> = rows.into_values().collect();
    rows.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then_with(|| a.entrant_id.cmp(&b.entrant_id))
    });
    rows
}

/// A team's 1-based position in `rows` (as ordered by [`tally_standings`])
/// and its row. `None` before it has a confirmed match.
pub fn position_of<'a>(rows: &'a [StandingsRow], team_id: &str) -> Option<(u32, &'a StandingsRow)> {
    rows.iter()
        .position(|r| r.entrant_id == team_id)
        .map(|i| (i as u32 + 1, &rows[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE: PointsRule = PointsRule {
        win: 3,
        draw: 1,
        loss: 0,
    };

    fn result(a: &str, b: &str, winner: Option<&str>) -> ConfirmedResult {
        ConfirmedResult {
            sides: vec![
                (format!("s{a}"), a.to_string()),
                (format!("s{b}"), b.to_string()),
            ],
            winner_side_id: winner.map(|w| format!("s{w}")),
        }
    }

    #[test]
    fn tallies_wins_draws_and_losses() {
        let rows = tally_standings(
            RULE,
            vec![
                result("a", "b", Some("a")),
                result("b", "c", None),
                result("a", "c", Some("c")),
            ],
        );
        let by: HashMap<_, _> = rows.iter().map(|r| (r.entrant_id.as_str(), r)).collect();
        assert_eq!((by["a"].won, by["a"].lost, by["a"].points), (1, 1, 3));
        assert_eq!((by["b"].drawn, by["b"].lost, by["b"].points), (1, 1, 1));
        assert_eq!((by["c"].won, by["c"].drawn, by["c"].points), (1, 1, 4));
        assert!(rows.iter().all(|r| r.played == 2));
        assert_eq!(rows[0].entrant_id, "c");
    }

    #[test]
    fn ties_order_by_entrant_id_and_position_is_one_based() {
        let rows = tally_standings(RULE, vec![result("z", "y", None)]);
        assert_eq!(rows[0].entrant_id, "y");
        assert_eq!(position_of(&rows, "z").map(|(p, _)| p), Some(2));
        assert_eq!(position_of(&rows, "x"), None);
    }

    #[test]
    fn empty_when_nothing_confirmed() {
        assert!(tally_standings(RULE, Vec::new()).is_empty());
    }
}
