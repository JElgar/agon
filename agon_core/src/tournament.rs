//! Pure tournament generation algorithms: knockout bracket construction,
//! round-robin fixture lists, and the snake draft a teamless tournament uses
//! to assemble squads. No DynamoDB/DAO types here — same separation as
//! `agon_core::series`'s date math versus `dao::series_ops`'s storage, kept
//! so these are directly unit-testable and so the service layer and any
//! future caller can never disagree on what a given entrant list produces.
//!
//! Match ids aren't minted here: a plan addresses its own matches by
//! placeholder ids (`"R<round>M<slot>"`) so the whole bracket/fixture shape
//! is deterministic and assertable in a test without touching `agon_core::id`.
//! The caller (the DAO/service layer, which does mint real ids) replaces
//! each placeholder with a freshly minted match id before writing anything.

use std::collections::HashMap;

// ===========================================================================
// Knockout
// ===========================================================================

/// One side of a planned knockout match: either a seeded entrant (known at
/// generation time, whether from direct seeding or a resolved bye chain) or
/// a placeholder awaiting another match's winner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnockoutSide {
    Entrant(String),
    /// Awaiting the winner of the match at this placeholder id.
    Tbd(String),
}

/// One match a knockout bracket plan says to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedKnockoutMatch {
    /// This match's placeholder id (`"R<round>M<slot>"`).
    pub id: String,
    /// 0-based: the first round that actually has matches, after byes are
    /// resolved — see `plan_knockout_bracket`'s doc comment.
    pub round: u32,
    /// Bracket position within `round`, for stable display ordering only.
    pub slot: u32,
    pub side_a: KnockoutSide,
    pub side_b: KnockoutSide,
}

/// A knockout bracket's full shape: every match to create, and which
/// match's winner feeds into which other match's side (`'A'`/`'B'`) — the
/// `TournamentBracketEdgeRecord` data, keyed by the *placeholder* ids in
/// `matches`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KnockoutBracketPlan {
    pub matches: Vec<PlannedKnockoutMatch>,
    /// `(from_match_id, into_match_id, into_side)`.
    pub edges: Vec<(String, String, char)>,
}

/// The standard tournament seeding order for a bracket of size `n` (a power
/// of two): the seed number occupying each leaf position, arranged so seed 1
/// meets the lowest seed, seed 2 the next-lowest, and so on, with top seeds
/// meeting as late as possible. `n` must be a power of two — callers always
/// derive it via [`next_power_of_two`].
///
/// Classic recursive construction: start from `[1]` and, to double the
/// bracket from size `k` to `2k`, replace each seed `s` with the pair `(s,
/// 2k + 1 - s)`. E.g. size 4: `[1,2]` -> `[1,4,2,3]` (pairs 1v4, 2v3); size
/// 8: `[1,4,2,3]` -> `[1,8,4,5,2,7,3,6]` (pairs 1v8, 4v5, 2v7, 3v6) — the
/// standard seeding every bracket app uses.
fn seeding_order(n: u32) -> Vec<u32> {
    let mut seeds = vec![1u32];
    while (seeds.len() as u32) < n {
        let size = seeds.len() as u32 * 2;
        let mut next = Vec::with_capacity(size as usize);
        for s in seeds {
            next.push(s);
            next.push(size + 1 - s);
        }
        seeds = next;
    }
    seeds
}

/// The smallest power of two `>= n` (`1` for `n == 0`, matching "no entrants
/// needs no bracket" rather than panicking).
pub fn next_power_of_two(n: u32) -> u32 {
    if n <= 1 {
        return 1;
    }
    let mut size = 1u32;
    while size < n {
        size *= 2;
    }
    size
}

/// A leaf slot's resolved state while folding the bracket bottom-up.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// A real entrant, known without needing a match (seeded directly, or a
    /// bye chain that's propagated all the way up with nothing to play).
    Known(String),
    /// Awaiting the winner of the (already-planned) match at this
    /// placeholder id.
    Pending(String),
    /// No entrant anywhere in this subtree (only possible when `entrants` is
    /// smaller than half the bracket at some internal split — see the merge
    /// rules below for how this is kept harmless even then).
    Empty,
}

/// Build a knockout bracket for `entrants`, already seeded strongest-first
/// (`entrants[0]` is seed 1). Byes go to the top seeds: the bracket is sized
/// to the next power of two, the gap is filled with empty leaves at the
/// bottom seeds, and any seed paired with an empty leaf advances with no
/// match at all, exactly as real byes work.
///
/// Folds the bracket bottom-up, one level (round) at a time, merging
/// sibling pairs:
/// - two known entrants -> a real match between them;
/// - a known entrant and a pending match's winner -> a real match, one side
///   pre-filled, the other to be filled by that match's result;
/// - two pending matches' winners -> a real match, both sides TBD;
/// - a known entrant and an empty slot -> the entrant advances with no
///   match (a bye);
/// - a pending match and an empty slot -> that match's winner advances with
///   no *additional* match — its edge is deferred, not dropped, so it's
///   still wired into whichever real match eventually consumes it, however
///   many empty siblings it skips past on the way. This can't actually
///   happen for `entrants.len()` at least half the bracket size (true
///   whenever the bracket is sized by `next_power_of_two`), but folding it
///   correctly rather than relying on that costs nothing extra here;
/// - two empty slots -> still empty (only possible with under 2 entrants).
///
/// A round's `round` index only increments for a level that actually
/// produces a match — a level that's pure bye-propagation isn't "a round",
/// per `PlannedKnockoutMatch::round`'s doc comment.
pub fn plan_knockout_bracket(entrants: &[String]) -> KnockoutBracketPlan {
    if entrants.len() < 2 {
        return KnockoutBracketPlan::default();
    }

    let size = next_power_of_two(entrants.len() as u32);
    let order = seeding_order(size);
    let mut slots: Vec<Slot> = order
        .into_iter()
        .map(|seed| match entrants.get((seed - 1) as usize) {
            Some(id) => Slot::Known(id.clone()),
            None => Slot::Empty,
        })
        .collect();

    let mut plan = KnockoutBracketPlan::default();
    let mut round = 0u32;

    while slots.len() > 1 {
        let mut next_level = Vec::with_capacity(slots.len() / 2);
        let mut level_matches: Vec<PlannedKnockoutMatch> = Vec::new();

        for (slot_idx, pair) in slots.chunks(2).enumerate() {
            let (a, b) = (pair[0].clone(), pair[1].clone());
            match (a, b) {
                (Slot::Empty, Slot::Empty) => next_level.push(Slot::Empty),
                (Slot::Known(x), Slot::Empty) | (Slot::Empty, Slot::Known(x)) => {
                    next_level.push(Slot::Known(x))
                }
                (Slot::Pending(p), Slot::Empty) | (Slot::Empty, Slot::Pending(p)) => {
                    next_level.push(Slot::Pending(p))
                }
                (a, b) => {
                    let match_id = format!("R{round}M{slot_idx}");
                    let (side_a, side_b) = (
                        match &a {
                            Slot::Known(x) => KnockoutSide::Entrant(x.clone()),
                            Slot::Pending(p) => {
                                plan.edges.push((p.clone(), match_id.clone(), 'A'));
                                KnockoutSide::Tbd(p.clone())
                            }
                            Slot::Empty => unreachable!("handled above"),
                        },
                        match &b {
                            Slot::Known(x) => KnockoutSide::Entrant(x.clone()),
                            Slot::Pending(p) => {
                                plan.edges.push((p.clone(), match_id.clone(), 'B'));
                                KnockoutSide::Tbd(p.clone())
                            }
                            Slot::Empty => unreachable!("handled above"),
                        },
                    );
                    level_matches.push(PlannedKnockoutMatch {
                        id: match_id.clone(),
                        round,
                        slot: slot_idx as u32,
                        side_a,
                        side_b,
                    });
                    next_level.push(Slot::Pending(match_id));
                }
            }
        }

        if !level_matches.is_empty() {
            plan.matches.extend(level_matches);
            round += 1;
        }
        slots = next_level;
    }

    plan
}

// ===========================================================================
// League (round robin)
// ===========================================================================

/// One matchday's fixtures: pairs of `entrants` indices.
pub type Matchday = Vec<(usize, usize)>;

/// A round-robin fixture list over `entrant_count` entrants via the circle
/// method: fix entrant 0, rotate the rest. `entrant_count - 1` matchdays for
/// an even count; `entrant_count` matchdays (one entrant sitting out each
/// matchday) for odd. Indices refer to the caller's own entrant ordering,
/// not a seed.
pub fn round_robin_fixtures(entrant_count: usize) -> Vec<Matchday> {
    if entrant_count < 2 {
        return Vec::new();
    }

    // Odd count: add a placeholder "bye" slot so the even-count algorithm
    // applies unchanged; whichever real entrant is paired with it each
    // matchday simply sits out.
    let byes = entrant_count % 2 == 1;
    let n = if byes {
        entrant_count + 1
    } else {
        entrant_count
    };
    let mut circle: Vec<usize> = (0..n).collect();
    let matchdays_count = n - 1;

    let mut matchdays = Vec::with_capacity(matchdays_count);
    for _ in 0..matchdays_count {
        let mut day = Vec::with_capacity(n / 2);
        for i in 0..n / 2 {
            let (a, b) = (circle[i], circle[n - 1 - i]);
            if !byes || (a < entrant_count && b < entrant_count) {
                day.push((a, b));
            }
        }
        matchdays.push(day);
        // Rotate everyone except the fixed first entrant.
        circle[1..].rotate_right(1);
    }
    matchdays
}

// ===========================================================================
// Teamless squad formation (snake draft)
// ===========================================================================

/// Split `entrants` (already ordered strongest-first by whatever signal —
/// win count, rating, or games played) into `squad_count` squads via a
/// snake draft: pass 1 deals seed 1, 2, 3, … one to each squad in order;
/// pass 2 deals the next `squad_count` in *reverse* order; and so on,
/// alternating direction each pass so summed squad strength stays as even
/// as possible — a standard balanced-team draft, nothing bespoke.
///
/// `squad_count` is computed by the caller as `entrants.len() / squad_size`
/// (at least 1); a non-divisible remainder lands wherever the snake order
/// puts it, so squads can differ in size by at most one.
pub fn snake_draft(entrants: &[String], squad_count: usize) -> Vec<Vec<String>> {
    if squad_count == 0 || entrants.is_empty() {
        return Vec::new();
    }
    let mut squads: Vec<Vec<String>> = vec![Vec::new(); squad_count];
    for (pass, chunk) in entrants.chunks(squad_count).enumerate() {
        let indices: Vec<usize> = if pass % 2 == 0 {
            (0..squad_count).collect()
        } else {
            (0..squad_count).rev().collect()
        };
        for (entrant, squad_idx) in chunk.iter().zip(indices) {
            squads[squad_idx].push(entrant.clone());
        }
    }
    squads
}

/// A teamless tournament squad's display label/colour palette, keyed by its
/// 0-based position — used both by `start_tournament`'s squad-from-entrants
/// generation and by a manually-added match's `tournament_squad` side,
/// which has no persisted "squad" entity of its own to carry a label/colour.
const SQUAD_PALETTE: [&str; 6] = [
    "#2952D9", "#D92929", "#1FA37A", "#D9A429", "#7A29D9", "#29A3D9",
];

/// See [`SQUAD_PALETTE`].
pub fn squad_label_colour(position: u32) -> (String, String) {
    (
        format!("Squad {}", position + 1),
        SQUAD_PALETTE[position as usize % SQUAD_PALETTE.len()].to_string(),
    )
}

/// A random seeding order for `entrants` — used when a tournament's
/// `seeding` is `"random"` rather than `"manual"` (caller-supplied `seed`
/// order, sorted by the caller instead). Pure, one-shot: there's no stored
/// state to keep consistent across calls, since the service layer calls
/// this exactly once, right before writing the generated bracket/fixtures.
pub fn random_seed_order(entrants: &[String]) -> Vec<String> {
    use rand::seq::SliceRandom;
    let mut order = entrants.to_vec();
    order.shuffle(&mut rand::rng());
    order
}

/// Resolve placeholder match ids in a [`KnockoutBracketPlan`] (and its
/// matches' `Tbd` sides) to the real ids `minted` maps them to — every
/// placeholder in a plan returned by [`plan_knockout_bracket`] is a key in
/// `minted`, by construction, so a missing entry is a caller bug, not a
/// recoverable condition.
pub fn knockout_placeholder_to_real_id(
    plan: &KnockoutBracketPlan,
    minted: &HashMap<String, String>,
) -> KnockoutBracketPlan {
    let resolve = |id: &str| minted.get(id).cloned().unwrap_or_else(|| id.to_string());
    KnockoutBracketPlan {
        matches: plan
            .matches
            .iter()
            .map(|m| PlannedKnockoutMatch {
                id: resolve(&m.id),
                round: m.round,
                slot: m.slot,
                side_a: match &m.side_a {
                    KnockoutSide::Entrant(e) => KnockoutSide::Entrant(e.clone()),
                    KnockoutSide::Tbd(p) => KnockoutSide::Tbd(resolve(p)),
                },
                side_b: match &m.side_b {
                    KnockoutSide::Entrant(e) => KnockoutSide::Entrant(e.clone()),
                    KnockoutSide::Tbd(p) => KnockoutSide::Tbd(resolve(p)),
                },
            })
            .collect(),
        edges: plan
            .edges
            .iter()
            .map(|(from, into, side)| (resolve(from), resolve(into), *side))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("t{i}")).collect()
    }

    #[test]
    fn next_power_of_two_examples() {
        assert_eq!(next_power_of_two(0), 1);
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(5), 8);
        assert_eq!(next_power_of_two(8), 8);
        assert_eq!(next_power_of_two(9), 16);
    }

    #[test]
    fn seeding_order_matches_standard_bracket_seeding() {
        assert_eq!(seeding_order(2), vec![1, 2]);
        assert_eq!(seeding_order(4), vec![1, 4, 2, 3]);
        assert_eq!(seeding_order(8), vec![1, 8, 4, 5, 2, 7, 3, 6]);
    }

    /// A power-of-two entrant count: every round is full, no byes.
    #[test]
    fn knockout_power_of_two_has_no_byes() {
        let plan = plan_knockout_bracket(&ids(4));
        // Round 0: 2 matches (1v4, 2v3 by seed -> t0v3, t1v2). Round 1: the final.
        let round0: Vec<_> = plan.matches.iter().filter(|m| m.round == 0).collect();
        let round1: Vec<_> = plan.matches.iter().filter(|m| m.round == 1).collect();
        assert_eq!(round0.len(), 2);
        assert_eq!(round1.len(), 1);
        // Every round-0 side is a real entrant, not TBD.
        for m in &round0 {
            assert!(matches!(m.side_a, KnockoutSide::Entrant(_)));
            assert!(matches!(m.side_b, KnockoutSide::Entrant(_)));
        }
        // The final's sides are both TBD, fed by the two round-0 matches.
        let final_ = round1[0];
        assert!(matches!(final_.side_a, KnockoutSide::Tbd(_)));
        assert!(matches!(final_.side_b, KnockoutSide::Tbd(_)));
        assert_eq!(plan.edges.len(), 2);
        for (_, into, _) in &plan.edges {
            assert_eq!(into, &final_.id);
        }
    }

    /// 3 entrants: one bye straight to the final, no round-0 match for them.
    #[test]
    fn knockout_three_entrants_gives_one_bye() {
        let plan = plan_knockout_bracket(&ids(3));
        // Only one round-0 match (the other bracket slot was a bye).
        let round0: Vec<_> = plan.matches.iter().filter(|m| m.round == 0).collect();
        assert_eq!(round0.len(), 1);
        let round1: Vec<_> = plan.matches.iter().filter(|m| m.round == 1).collect();
        assert_eq!(round1.len(), 1, "the final");
        let final_ = round1[0];
        // One final side is the bye'd entrant (seed 1, t0) directly; the
        // other is TBD, fed by the round-0 match.
        let sides = [&final_.side_a, &final_.side_b];
        assert!(sides.contains(&&KnockoutSide::Entrant("t0".to_string())));
        assert!(sides.iter().any(|s| matches!(s, KnockoutSide::Tbd(_))));
        assert_eq!(plan.edges.len(), 1);
        assert_eq!(plan.edges[0].1, final_.id);
    }

    /// 2 entrants in a bracket sized for 8 (heavily sparse) still folds to
    /// exactly one match (the final), with every intermediate level pure
    /// bye-propagation — the "first round that actually has matches" is
    /// round 0, and it's the only round.
    #[test]
    fn knockout_sparse_bracket_skips_empty_rounds() {
        let plan = plan_knockout_bracket(&ids(2));
        assert_eq!(plan.matches.len(), 1);
        assert_eq!(plan.matches[0].round, 0);
        assert!(plan.edges.is_empty());
    }

    /// Every entrant appears in the bracket exactly once (seeded correctly,
    /// strongest apart), for a selection of sizes including non-powers-of-two.
    #[test]
    fn knockout_every_entrant_appears_exactly_once() {
        for n in [2, 3, 4, 5, 6, 7, 8, 9, 13, 16] {
            let entrants = ids(n);
            let plan = plan_knockout_bracket(&entrants);
            let mut seen: Vec<&str> = Vec::new();
            for m in &plan.matches {
                for side in [&m.side_a, &m.side_b] {
                    if let KnockoutSide::Entrant(e) = side {
                        seen.push(e);
                    }
                }
            }
            seen.sort();
            seen.dedup();
            assert_eq!(
                seen.len(),
                n,
                "n={n}: every entrant should appear exactly once as a direct seed somewhere"
            );
        }
    }

    /// Top seeds get the byes (met latest) — for 5 entrants in an 8-bracket,
    /// seeds 1-3 (the top 3) bye through round 0, and only seeds 4 vs 5
    /// actually play a round-0 match.
    #[test]
    fn knockout_byes_go_to_top_seeds() {
        let plan = plan_knockout_bracket(&ids(5));
        let round0: Vec<_> = plan.matches.iter().filter(|m| m.round == 0).collect();
        assert_eq!(round0.len(), 1);
        let sides: Vec<&KnockoutSide> = [&round0[0].side_a, &round0[0].side_b].into();
        assert!(sides.contains(&&KnockoutSide::Entrant("t3".to_string())));
        assert!(sides.contains(&&KnockoutSide::Entrant("t4".to_string())));
    }

    #[test]
    fn round_robin_even_count() {
        let fixtures = round_robin_fixtures(4);
        assert_eq!(fixtures.len(), 3);
        for day in &fixtures {
            assert_eq!(day.len(), 2);
        }
        // Every unordered pair of the 4 entrants appears exactly once.
        let mut seen: Vec<(usize, usize)> = fixtures
            .iter()
            .flatten()
            .map(|&(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect();
        seen.sort();
        let mut expected: Vec<(usize, usize)> = Vec::new();
        for a in 0..4 {
            for b in (a + 1)..4 {
                expected.push((a, b));
            }
        }
        assert_eq!(seen, expected);
    }

    #[test]
    fn round_robin_odd_count_has_one_sitting_out_each_matchday() {
        let fixtures = round_robin_fixtures(5);
        assert_eq!(fixtures.len(), 5);
        for day in &fixtures {
            // 5 entrants, one sits out -> 2 matches.
            assert_eq!(day.len(), 2);
        }
        let mut seen: Vec<(usize, usize)> = fixtures
            .iter()
            .flatten()
            .map(|&(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect();
        seen.sort();
        let mut expected: Vec<(usize, usize)> = Vec::new();
        for a in 0..5 {
            for b in (a + 1)..5 {
                expected.push((a, b));
            }
        }
        assert_eq!(seen, expected);
    }

    #[test]
    fn round_robin_trivial_counts() {
        assert_eq!(round_robin_fixtures(0), Vec::<Matchday>::new());
        assert_eq!(round_robin_fixtures(1), Vec::<Matchday>::new());
    }

    #[test]
    fn snake_draft_alternates_direction() {
        let entrants = ids(6);
        let squads = snake_draft(&entrants, 3);
        assert_eq!(squads.len(), 3);
        // Pass 1 (forward): t0->squad0, t1->squad1, t2->squad2.
        // Pass 2 (reverse): t3->squad2, t4->squad1, t5->squad0.
        assert_eq!(squads[0], vec!["t0", "t5"]);
        assert_eq!(squads[1], vec!["t1", "t4"]);
        assert_eq!(squads[2], vec!["t2", "t3"]);
    }

    #[test]
    fn snake_draft_uneven_remainder_lands_in_the_last_partial_pass() {
        let entrants = ids(7);
        let squads = snake_draft(&entrants, 3);
        let total: usize = squads.iter().map(|s| s.len()).sum();
        assert_eq!(total, 7);
        // Sizes differ by at most one.
        let sizes: Vec<usize> = squads.iter().map(|s| s.len()).collect();
        assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
    }

    #[test]
    fn snake_draft_trivial_inputs() {
        assert_eq!(snake_draft(&[], 3), Vec::<Vec<String>>::new());
        assert_eq!(snake_draft(&ids(3), 0), Vec::<Vec<String>>::new());
    }

    #[test]
    fn knockout_placeholder_ids_resolve_to_minted_ids() {
        let plan = plan_knockout_bracket(&ids(4));
        let minted: HashMap<String, String> = plan
            .matches
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), format!("real-{i}")))
            .collect();
        let resolved = knockout_placeholder_to_real_id(&plan, &minted);
        for m in &resolved.matches {
            assert!(m.id.starts_with("real-"));
            for side in [&m.side_a, &m.side_b] {
                if let KnockoutSide::Tbd(p) = side {
                    assert!(p.starts_with("real-"), "TBD side should resolve too");
                }
            }
        }
        for (from, into, _) in &resolved.edges {
            assert!(from.starts_with("real-"));
            assert!(into.starts_with("real-"));
        }
    }
}
