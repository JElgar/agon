//! One call from a pool to a shaped preview: estimate everyone, split,
//! measure, and label each player for display. This is the whole surface
//! the browser uses (through `agon_core_wasm`), as plain serde data in and
//! out, so the UI never re-implements any of it.

use serde::{Deserialize, Serialize};

use super::scale::display_rating;
use super::solver::{self, Method, PoolSlot, Verdict, WinProbability};
use super::strength::{self, History, Level, RatingInput, Signal, WinRateInfo, WinRecord};

/// Who the split may move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub enum Scope {
    /// Only players with no side yet; everyone placed stays put.
    #[default]
    Unassigned,
    /// Start over. Pins still hold.
    Everyone,
}

/// A rating as the public API shows it (`LadderRating`): display scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct DisplayedRating {
    pub rating: f64,
    pub confidence: f64,
    pub matches_rated: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct PlanPlayer {
    /// Whatever the caller keys players by (entrant id, player id).
    pub id: String,
    /// The player's published rating on this sport's ladder, if any.
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub rating: Option<DisplayedRating>,
    /// Their played/won/drawn/lost in this sport.
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub record: WinRecord,
    /// No account behind this roster entry.
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub guest: bool,
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub level: Option<Level>,
    /// Side index they're on now, if any.
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub current_side: Option<usize>,
    /// Side index the organiser pinned them to, if any.
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub pinned_side: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(from_wasm_abi))]
pub struct PlanInput {
    pub players: Vec<PlanPlayer>,
    /// One entry per side, in the caller's side order: a cap, or `null`.
    pub caps: Vec<Option<usize>>,
    pub method: Method,
    #[serde(default)]
    #[cfg_attr(feature = "wasm", tsify(optional))]
    pub scope: Scope,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct PlannedPlayer {
    pub id: String,
    pub signal: Signal,
    /// 1 = little history, 2 = some, 3 = solid.
    pub certainty: u8,
    /// Estimated strength, display scale. `None` for a placement rating.
    pub estimate: Option<i32>,
    /// `None` for a placement rating.
    pub ranked_games: Option<u64>,
    pub win_rate: Option<WinRateInfo>,
    /// Share of the estimate that came from the rating (0–1).
    pub rating_weight: f64,
    pub level: Option<Level>,
    pub pinned: bool,
    /// This split puts them somewhere other than where they are now.
    pub moved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct PlannedSide {
    /// Average estimated strength, display scale, rounded to 10.
    pub average: Option<i32>,
    pub unsure_count: u32,
    pub players: Vec<PlannedPlayer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi))]
pub struct PlanOutput {
    pub seed: u64,
    pub method: Method,
    pub verdict: Verdict,
    /// Display points between the strongest and weakest side's average.
    pub spread: i32,
    /// Two sides only: chance the first side wins, with a ±1σ range.
    pub win_probability: Option<WinProbability>,
    pub unsure_count: u32,
    pub sides: Vec<PlannedSide>,
}

/// Estimate, split and shape. Errors are the solver's (no sides, a bad
/// pin, a pool that doesn't fit the caps), as display text.
pub fn plan(input: &PlanInput) -> Result<PlanOutput, solver::BuildError> {
    let histories: Vec<History> = input
        .players
        .iter()
        .map(|p| History {
            rating: p
                .rating
                .map(|r| RatingInput::from_display(r.rating, r.confidence, r.matches_rated)),
            record: p.record,
            guest: p.guest,
            level: p.level,
        })
        .collect();
    let estimates = strength::estimate_pool(&histories);
    let slots: Vec<PoolSlot> = input
        .players
        .iter()
        .zip(&estimates)
        .map(|(p, e)| PoolSlot {
            mu: e.mu,
            sigma: e.sigma,
            pinned_to: p.pinned_side.or(match input.scope {
                Scope::Unassigned => p.current_side,
                Scope::Everyone => None,
            }),
        })
        .collect();
    let sides = solver::build(&slots, &input.caps, input.method, input.seed)?;
    let balance = solver::balance(&slots, &sides);

    let sides = sides
        .iter()
        .enumerate()
        .map(|(s, idxs)| PlannedSide {
            average: balance.side_mean[s]
                .map(|m| ((display_rating(m) / 10.0).round() * 10.0) as i32),
            unsure_count: idxs
                .iter()
                .filter(|&&i| estimates[i].certainty == 1)
                .count() as u32,
            players: idxs
                .iter()
                .map(|&i| {
                    let (p, e) = (&input.players[i], &estimates[i]);
                    let hidden = e.signal == Signal::Placement;
                    PlannedPlayer {
                        id: p.id.clone(),
                        signal: e.signal,
                        certainty: e.certainty,
                        estimate: (!hidden).then(|| display_rating(e.mu).round() as i32),
                        ranked_games: (!hidden).then_some(e.ranked_games),
                        win_rate: e.win_rate,
                        rating_weight: e.rating_weight,
                        level: e.level,
                        pinned: slots[i].pinned_to.is_some(),
                        moved: p.current_side != Some(s),
                    }
                })
                .collect(),
        })
        .collect();

    Ok(PlanOutput {
        seed: input.seed,
        method: input.method,
        verdict: balance.verdict,
        spread: balance.spread.round() as i32,
        win_probability: balance.win_probability,
        unsure_count: estimates.iter().filter(|e| e.certainty == 1).count() as u32,
        sides,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(
        id: &str,
        rating: Option<f64>,
        played: u64,
        wins: u64,
        current: Option<usize>,
    ) -> PlanPlayer {
        PlanPlayer {
            id: id.into(),
            rating: rating.map(|r| DisplayedRating {
                rating: r,
                confidence: 120.0,
                matches_rated: 20,
            }),
            record: WinRecord {
                played,
                wins,
                draws: 0,
                losses: played - wins,
            },
            guest: false,
            level: None,
            current_side: current,
            pinned_side: None,
        }
    }

    #[test]
    fn plans_every_player_exactly_once_and_respects_placed_players() {
        let players = vec![
            player("a", Some(1700.0), 30, 20, Some(1)),
            player("b", Some(1450.0), 30, 10, None),
            player("c", None, 12, 8, None),
            player("d", None, 0, 0, None),
        ];
        let out = plan(&PlanInput {
            players,
            caps: vec![None, None],
            method: Method::AutoBalanced,
            scope: Scope::Unassigned,
            seed: 3,
        })
        .unwrap();
        let ids: Vec<&str> = out
            .sides
            .iter()
            .flat_map(|s| s.players.iter().map(|p| p.id.as_str()))
            .collect();
        assert_eq!(ids.len(), 4);
        let a = out.sides[1]
            .players
            .iter()
            .find(|p| p.id == "a")
            .expect("a stays on side 1");
        assert!(a.pinned && !a.moved);
        assert_eq!(a.signal, Signal::Rated);
        assert_eq!(a.estimate, Some(1700));
        assert!(out.win_probability.is_some());
    }

    #[test]
    fn json_round_trip_matches_what_the_browser_sends() {
        let json = r#"{
            "players": [
                {"id": "p1", "rating": {"rating": 1600, "confidence": 150, "matches_rated": 12},
                 "record": {"played": 20, "wins": 12, "draws": 0, "losses": 8}},
                {"id": "p2", "guest": true, "level": "stronger"},
                {"id": "p3", "record": {"played": 4, "wins": 1, "draws": 1, "losses": 2}, "pinned_side": 0},
                {"id": "p4"}
            ],
            "caps": [null, 3],
            "method": "auto_balanced",
            "scope": "everyone",
            "seed": 42
        }"#;
        let input: PlanInput = serde_json::from_str(json).unwrap();
        let out = plan(&input).unwrap();
        let text = serde_json::to_string(&out).unwrap();
        assert!(text.contains("\"verdict\""));
        assert!(text.contains("\"signal\":\"guest\""));
        assert!(text.contains("\"level\":\"stronger\""));
    }

    #[test]
    fn solver_errors_surface_as_errors() {
        let input = PlanInput {
            players: vec![player("a", None, 0, 0, None); 3],
            caps: vec![Some(1), Some(1)],
            method: Method::Random,
            scope: Scope::Everyone,
            seed: 0,
        };
        assert!(plan(&input).is_err());
    }
}
