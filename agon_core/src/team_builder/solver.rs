//! Splitting a pool between sides.
//!
//! Sides are fixed in number and order (squad 0..k, or a match's sides in
//! the caller's order); the solver only decides who goes where. Pinned
//! players stay where they were pinned — that is also how "only place the
//! unassigned" works: the caller pins everyone already placed.
//!
//! **Sizes** are decided before anyone is placed: as even as the pins and
//! caps allow, the extra players of an uneven pool going to the lowest-index
//! sides with room. Both methods then fill exactly those sizes, so Random
//! and Auto Balanced never disagree on how big a side is.
//!
//! **Auto Balanced** is lexicographic:
//!
//! 1. minimise the spread of `Σμ` across sides — the engine models a team as
//!    the sum of its players, so equal `Σμ` is what the rating system itself
//!    would call an even game;
//! 2. among splits within [`MU_TOLERANCE`] of the best, minimise the spread
//!    of `Σσ²`, which spreads the players we know least about.
//!
//! Two sides with at most [`EXACT_LIMIT`] free players are solved exactly
//! (every split enumerated — the common five-a-side and intra-squad case).
//! Anything bigger hill-climbs pairwise swaps from a snake draft plus
//! seeded random restarts. Either way, every split within tolerance of the
//! best is a candidate, and the seed picks one: re-rolling gives a
//! different, equally fair split rather than the same one again.

use std::collections::BTreeSet;

use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;

use crate::rating::{BETA, SCALE};

/// Splits whose `Σμ` spread is within this of the best (native μ; 0.5 is 15
/// display points) count as equally even.
pub const MU_TOLERANCE: f64 = 0.5;

/// Then, splits whose `Σσ²` spread is within this of the best count as
/// equally spread. One unknown swapped for a settled player moves it ~40.
pub const VAR_TOLERANCE: f64 = 10.0;

/// Free players at or under which two sides are solved by enumeration.
pub const EXACT_LIMIT: usize = 16;

/// Hill-climb restarts beyond the snake-draft start, for larger pools.
pub const RESTARTS: usize = 12;

/// At most this many distinct near-best splits are kept for the re-roll.
const CANDIDATE_CAP: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Random,
    AutoBalanced,
}

/// One player as the solver sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolSlot {
    pub mu: f64,
    pub sigma: f64,
    /// Side index this player must stay on.
    pub pinned_to: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    #[error("there are no sides to place players into")]
    NoSides,
    #[error("player is pinned to side {0}, which doesn't exist")]
    BadPin(usize),
    #[error("{players} players don't fit: the sides hold {capacity} at most")]
    OverCapacity { players: usize, capacity: usize },
    #[error("side {0} has more players pinned to it than it holds")]
    PinnedOverCap(usize),
}

/// How even a split came out, in the units the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Even,
    Close,
    Uneven,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    /// `Σμ` per side.
    pub side_mu: Vec<f64>,
    /// Mean μ per side (`None` for an empty side).
    pub side_mean: Vec<Option<f64>>,
    /// Display points between the strongest and weakest side's *average*
    /// player — the "within 6 pts" figure.
    pub spread: f64,
    /// Two sides only: chance side 0 beats side 1 under the Weng-Lin model,
    /// and the same chance if our estimates are one standard deviation off
    /// either way — the honest "could be 44–58%" range.
    pub win_probability: Option<WinProbability>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WinProbability {
    pub expected: f64,
    pub low: f64,
    pub high: f64,
}

/// Decide each side's size: pins first, then one at a time to the smallest
/// side with room (ties to the lowest index).
fn side_sizes(players: &[PoolSlot], caps: &[Option<usize>]) -> Result<Vec<usize>, BuildError> {
    let k = caps.len();
    if k == 0 {
        return Err(BuildError::NoSides);
    }
    let mut sizes = vec![0usize; k];
    for p in players {
        if let Some(s) = p.pinned_to {
            *sizes.get_mut(s).ok_or(BuildError::BadPin(s))? += 1;
        }
    }
    for (i, (&size, cap)) in sizes.iter().zip(caps).enumerate() {
        if cap.is_some_and(|c| size > c) {
            return Err(BuildError::PinnedOverCap(i));
        }
    }
    let free = players.iter().filter(|p| p.pinned_to.is_none()).count();
    for _ in 0..free {
        let next = (0..k)
            .filter(|&i| caps[i].is_none_or(|c| sizes[i] < c))
            .min_by_key(|&i| (sizes[i], i));
        match next {
            Some(i) => sizes[i] += 1,
            None => {
                return Err(BuildError::OverCapacity {
                    players: players.len(),
                    capacity: caps.iter().map(|c| c.unwrap_or(usize::MAX)).sum(),
                });
            }
        }
    }
    Ok(sizes)
}

/// Split `players` between `caps.len()` sides. Returns, per side, indices
/// into `players`, each side sorted ascending. Deterministic in `seed`.
pub fn build(
    players: &[PoolSlot],
    caps: &[Option<usize>],
    method: Method,
    seed: u64,
) -> Result<Vec<Vec<usize>>, BuildError> {
    let sizes = side_sizes(players, caps)?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let pinned: Vec<Vec<usize>> = (0..sizes.len())
        .map(|s| {
            (0..players.len())
                .filter(|&i| players[i].pinned_to == Some(s))
                .collect()
        })
        .collect();
    let mut free: Vec<usize> = (0..players.len())
        .filter(|&i| players[i].pinned_to.is_none())
        .collect();
    // Shuffle first in both methods: Random deals from it, Auto Balanced
    // breaks μ ties with it so equal players don't always land the same way.
    free.shuffle(&mut rng);

    let sides = match method {
        Method::Random => deal(&pinned, &sizes, &free),
        Method::AutoBalanced => balanced(players, &pinned, &sizes, free, &mut rng),
    };
    Ok(sides
        .into_iter()
        .map(|mut s| {
            s.sort_unstable();
            s
        })
        .collect())
}

/// Fill each side up to its size from `order`, in side order.
fn deal(pinned: &[Vec<usize>], sizes: &[usize], order: &[usize]) -> Vec<Vec<usize>> {
    let mut sides = pinned.to_vec();
    let mut it = order.iter();
    for (side, &size) in sides.iter_mut().zip(sizes) {
        while side.len() < size {
            side.push(*it.next().expect("sizes sum to the pool"));
        }
    }
    sides
}

/// `(Σμ spread, Σσ² spread)` of a split.
type Objective = (f64, f64);
/// Per side, indices into the pool.
type Split = Vec<Vec<usize>>;
/// A swap: `(side a, index in a, side b, index in b)`.
type Swap = (usize, usize, usize, usize);

/// `(Σμ spread, Σσ² spread)` of a split.
fn objective(players: &[PoolSlot], sides: &[Vec<usize>]) -> (f64, f64) {
    let (mut mu_lo, mut mu_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut var_lo, mut var_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for side in sides {
        let mu: f64 = side.iter().map(|&i| players[i].mu).sum();
        let var: f64 = side.iter().map(|&i| players[i].sigma.powi(2)).sum();
        mu_lo = mu_lo.min(mu);
        mu_hi = mu_hi.max(mu);
        var_lo = var_lo.min(var);
        var_hi = var_hi.max(var);
    }
    (mu_hi - mu_lo, var_hi - var_lo)
}

fn better(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.0 - 1e-9 || ((a.0 - b.0).abs() <= 1e-9 && a.1 < b.1 - 1e-9)
}

fn balanced(
    players: &[PoolSlot],
    pinned: &[Vec<usize>],
    sizes: &[usize],
    mut free: Vec<usize>,
    rng: &mut ChaCha8Rng,
) -> Vec<Vec<usize>> {
    // Strongest first; the shuffle above decides ties.
    free.sort_by(|&a, &b| players[b].mu.total_cmp(&players[a].mu));

    let mut found: Vec<(Objective, Split)> = Vec::new();
    if sizes.len() == 2 && free.len() <= EXACT_LIMIT {
        enumerate_two(players, pinned, sizes, &free, &mut found);
    } else {
        let starts = std::iter::once(snake(pinned, sizes, &free)).chain((0..RESTARTS).map(|_| {
            let mut order = free.clone();
            order.shuffle(rng);
            deal(pinned, sizes, &order)
        }));
        for start in starts {
            let climbed = hill_climb(players, pinned, start);
            found.push((objective(players, &climbed), climbed));
        }
    }

    // Lexicographic tolerance: even first, then spread out.
    let best_mu = found.iter().map(|(o, _)| o.0).fold(f64::INFINITY, f64::min);
    found.retain(|(o, _)| o.0 <= best_mu + MU_TOLERANCE);
    let best_var = found.iter().map(|(o, _)| o.1).fold(f64::INFINITY, f64::min);
    found.retain(|(o, _)| o.1 <= best_var + VAR_TOLERANCE);

    // Distinct splits only, in a canonical order, so the seed's pick
    // doesn't depend on how often a split was found.
    let distinct: BTreeSet<Vec<Vec<usize>>> = found
        .into_iter()
        .map(|(_, s)| {
            s.into_iter()
                .map(|mut side| {
                    side.sort_unstable();
                    side
                })
                .collect()
        })
        .collect();
    let candidates: Vec<Vec<Vec<usize>>> = distinct.into_iter().collect();
    let pick = rand::Rng::random_range(rng, 0..candidates.len());
    candidates[pick].clone()
}

/// Snake draft over the free players (strongest first) into sides with room.
fn snake(pinned: &[Vec<usize>], sizes: &[usize], free: &[usize]) -> Vec<Vec<usize>> {
    let k = sizes.len();
    let mut sides = pinned.to_vec();
    let mut forward = true;
    let mut it = free.iter().peekable();
    while it.peek().is_some() {
        let order: Vec<usize> = if forward {
            (0..k).collect()
        } else {
            (0..k).rev().collect()
        };
        for s in order {
            if sides[s].len() < sizes[s]
                && let Some(&p) = it.next()
            {
                sides[s].push(p);
            }
        }
        forward = !forward;
    }
    sides
}

/// Steepest-descent pairwise swaps between sides, never moving a pin.
fn hill_climb(
    players: &[PoolSlot],
    pinned: &[Vec<usize>],
    mut sides: Vec<Vec<usize>>,
) -> Vec<Vec<usize>> {
    let is_pinned = |i: usize| pinned.iter().any(|p| p.contains(&i));
    let mut current = objective(players, &sides);
    loop {
        let mut best: Option<(Objective, Swap)> = None;
        for a in 0..sides.len() {
            for b in a + 1..sides.len() {
                for ia in 0..sides[a].len() {
                    if is_pinned(sides[a][ia]) {
                        continue;
                    }
                    for ib in 0..sides[b].len() {
                        if is_pinned(sides[b][ib]) {
                            continue;
                        }
                        let (pa, pb) = (sides[a][ia], sides[b][ib]);
                        sides[a][ia] = pb;
                        sides[b][ib] = pa;
                        let o = objective(players, &sides);
                        sides[a][ia] = pa;
                        sides[b][ib] = pb;
                        if better(o, best.map_or(current, |(bo, _)| bo)) {
                            best = Some((o, (a, ia, b, ib)));
                        }
                    }
                }
            }
        }
        match best {
            Some((o, (a, ia, b, ib))) => {
                let tmp = sides[a][ia];
                sides[a][ia] = sides[b][ib];
                sides[b][ib] = tmp;
                current = o;
            }
            None => return sides,
        }
    }
}

/// Every way to put the free players on two sides of the given sizes.
fn enumerate_two(
    players: &[PoolSlot],
    pinned: &[Vec<usize>],
    sizes: &[usize],
    free: &[usize],
    found: &mut Vec<(Objective, Split)>,
) {
    let take = sizes[0] - pinned[0].len();
    let n = free.len();
    let mut best_mu = f64::INFINITY;
    let mut chosen = Vec::with_capacity(take);
    // Iterative combination walk over indices into `free`.
    fn walk(
        start: usize,
        n: usize,
        take: usize,
        chosen: &mut Vec<usize>,
        visit: &mut dyn FnMut(&[usize]),
    ) {
        if chosen.len() == take {
            visit(chosen);
            return;
        }
        for i in start..n {
            if n - i < take - chosen.len() {
                break;
            }
            chosen.push(i);
            walk(i + 1, n, take, chosen, visit);
            chosen.pop();
        }
    }
    walk(0, n, take, &mut chosen, &mut |combo| {
        let mut a = pinned[0].clone();
        let mut b = pinned[1].clone();
        let mut c = 0;
        for (i, &p) in free.iter().enumerate() {
            if c < combo.len() && combo[c] == i {
                a.push(p);
                c += 1;
            } else {
                b.push(p);
            }
        }
        let sides = vec![a, b];
        let o = objective(players, &sides);
        if o.0 <= best_mu + MU_TOLERANCE {
            best_mu = best_mu.min(o.0);
            found.push((o, sides));
            if found.len() > CANDIDATE_CAP * 4 {
                found.retain(|(fo, _)| fo.0 <= best_mu + MU_TOLERANCE);
                found.sort_by(|x, y| x.0.0.total_cmp(&y.0.0).then(x.0.1.total_cmp(&y.0.1)));
                found.truncate(CANDIDATE_CAP);
            }
        }
    });
}

/// How even `sides` (indices into `players`) are.
#[must_use]
pub fn balance(players: &[PoolSlot], sides: &[Vec<usize>]) -> Balance {
    let side_mu: Vec<f64> = sides
        .iter()
        .map(|s| s.iter().map(|&i| players[i].mu).sum())
        .collect();
    let side_mean: Vec<Option<f64>> = sides
        .iter()
        .zip(&side_mu)
        .map(|(s, &mu)| (!s.is_empty()).then(|| mu / s.len() as f64))
        .collect();
    let means: Vec<f64> = side_mean.iter().flatten().copied().collect();
    let spread = if means.len() < 2 {
        0.0
    } else {
        let hi = means.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lo = means.iter().copied().fold(f64::INFINITY, f64::min);
        (hi - lo) * SCALE
    };

    let win_probability = (sides.len() == 2).then(|| {
        let var = |s: &Vec<usize>| s.iter().map(|&i| players[i].sigma.powi(2)).sum::<f64>();
        let estimate_var = var(&sides[0]) + var(&sides[1]);
        let n = (sides[0].len() + sides[1].len()) as f64;
        // Weng-Lin's two-team win probability: logistic in the μ gap, scaled
        // by c = √(Σσ² + nβ²).
        let c = (estimate_var + n * BETA * BETA).sqrt();
        let p = |delta: f64| 1.0 / (1.0 + (-delta / c).exp());
        let delta = side_mu[0] - side_mu[1];
        let off = estimate_var.sqrt();
        WinProbability {
            expected: p(delta),
            low: p(delta - off),
            high: p(delta + off),
        }
    });

    let verdict = match win_probability {
        Some(w) => {
            let off = (w.expected - 0.5).abs();
            if off <= 0.03 {
                Verdict::Even
            } else if off <= 0.08 {
                Verdict::Close
            } else {
                Verdict::Uneven
            }
        }
        None if spread <= 15.0 => Verdict::Even,
        None if spread <= 40.0 => Verdict::Close,
        None => Verdict::Uneven,
    };

    Balance {
        side_mu,
        side_mean,
        spread,
        win_probability,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::INITIAL_SIGMA;

    fn slot(mu: f64, sigma: f64) -> PoolSlot {
        PoolSlot {
            mu,
            sigma,
            pinned_to: None,
        }
    }

    /// The preview board's 16-player pool, in native μ.
    fn thursday_pool() -> Vec<PoolSlot> {
        let display = [
            (1712.0, 3.0),
            (1655.0, 3.0),
            (1590.0, 3.5),
            (1540.0, 3.0),
            (1478.0, 3.5),
            (1430.0, 3.0),
            (1598.0, 5.0),
            (1530.0, 5.0),
            (1465.0, 5.0),
            (1585.0, 5.5),
            (1570.0, 5.5),
            (1470.0, 6.0),
            (1512.0, 7.3),
            (1549.0, INITIAL_SIGMA),
            (1549.0, INITIAL_SIGMA),
            (1549.0, INITIAL_SIGMA),
        ];
        display
            .iter()
            .map(|&(d, s)| slot(25.0 + (d - 1500.0) / SCALE, s))
            .collect()
    }

    fn assert_valid(players: &[PoolSlot], sides: &[Vec<usize>], caps: &[Option<usize>]) {
        let mut all: Vec<usize> = sides.iter().flatten().copied().collect();
        all.sort_unstable();
        assert_eq!(
            all,
            (0..players.len()).collect::<Vec<_>>(),
            "everyone placed exactly once"
        );
        for (i, p) in players.iter().enumerate() {
            if let Some(s) = p.pinned_to {
                assert!(sides[s].contains(&i), "pin respected");
            }
        }
        for (side, cap) in sides.iter().zip(caps) {
            assert!(cap.is_none_or(|c| side.len() <= c), "cap respected");
        }
    }

    #[test]
    fn both_methods_place_everyone_with_even_sizes() {
        let pool = thursday_pool();
        let caps = [None; 4];
        for method in [Method::Random, Method::AutoBalanced] {
            for seed in 0..5 {
                let sides = build(&pool, &caps, method, seed).unwrap();
                assert_valid(&pool, &sides, &caps);
                assert!(sides.iter().all(|s| s.len() == 4));
            }
        }
    }

    #[test]
    fn same_seed_same_split_and_reroll_changes_it() {
        let pool = thursday_pool();
        let caps = [None; 4];
        for method in [Method::Random, Method::AutoBalanced] {
            let a = build(&pool, &caps, method, 7).unwrap();
            assert_eq!(a, build(&pool, &caps, method, 7).unwrap());
            let differs = (8..20).any(|seed| build(&pool, &caps, method, seed).unwrap() != a);
            assert!(
                differs,
                "{method:?}: re-roll should be able to change the split"
            );
        }
    }

    #[test]
    fn balanced_is_even_and_beats_random() {
        let pool = thursday_pool();
        let caps = [None; 4];
        let spread = |m, seed| balance(&pool, &build(&pool, &caps, m, seed).unwrap()).spread;
        for seed in 0..10 {
            assert!(spread(Method::AutoBalanced, seed) <= 15.0, "seed {seed}");
        }
        let random_mean: f64 = (0..30).map(|s| spread(Method::Random, s)).sum::<f64>() / 30.0;
        let balanced_mean: f64 = (0..30)
            .map(|s| spread(Method::AutoBalanced, s))
            .sum::<f64>()
            / 30.0;
        assert!(
            balanced_mean * 3.0 < random_mean,
            "{balanced_mean} vs {random_mean}"
        );
    }

    #[test]
    fn unknowns_are_spread_across_sides() {
        // Four settled equals and four unknowns, two sides: every μ split is
        // perfectly even, so only the σ² rule can decide — two unknowns each.
        let mut pool: Vec<PoolSlot> = (0..4).map(|_| slot(25.0, 2.0)).collect();
        pool.extend((0..4).map(|_| slot(25.0, INITIAL_SIGMA)));
        for seed in 0..20 {
            let sides = build(&pool, &[None, None], Method::AutoBalanced, seed).unwrap();
            for side in &sides {
                assert_eq!(side.iter().filter(|&&i| i >= 4).count(), 2, "seed {seed}");
            }
        }
    }

    #[test]
    fn exact_two_side_split_matches_brute_force() {
        let pool: Vec<PoolSlot> = [31.0, 29.5, 27.0, 26.0, 25.0, 24.5, 22.0, 20.0, 19.0, 18.5]
            .iter()
            .map(|&m| slot(m, 3.0))
            .collect();
        let total: f64 = pool.iter().map(|p| p.mu).sum();
        let mut best = f64::INFINITY;
        for mask in 0u32..(1 << pool.len()) {
            if mask.count_ones() == 5 {
                let a: f64 = (0..pool.len())
                    .filter(|i| mask & (1 << i) != 0)
                    .map(|i| pool[i].mu)
                    .sum();
                best = best.min((total - 2.0 * a).abs());
            }
        }
        for seed in 0..10 {
            let sides = build(&pool, &[None, None], Method::AutoBalanced, seed).unwrap();
            let o = objective(&pool, &sides);
            assert!(o.0 <= best + MU_TOLERANCE + 1e-9);
        }
    }

    #[test]
    fn pins_hold_and_unassigned_fill_around_them() {
        let mut pool = thursday_pool();
        pool[0].pinned_to = Some(0);
        pool[1].pinned_to = Some(0);
        pool[5].pinned_to = Some(3);
        let caps = [None; 4];
        for method in [Method::Random, Method::AutoBalanced] {
            let sides = build(&pool, &caps, method, 3).unwrap();
            assert_valid(&pool, &sides, &caps);
        }
    }

    #[test]
    fn uneven_pool_sizes_differ_by_at_most_one() {
        let pool: Vec<PoolSlot> = (0..11).map(|i| slot(20.0 + i as f64, 3.0)).collect();
        let sides = build(&pool, &[None; 3], Method::AutoBalanced, 1).unwrap();
        let sizes: Vec<usize> = sides.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![4, 4, 3]);
    }

    #[test]
    fn caps_are_respected_and_overflow_is_an_error() {
        let pool: Vec<PoolSlot> = (0..6).map(|_| slot(25.0, 3.0)).collect();
        let sides = build(&pool, &[Some(2), None], Method::Random, 0).unwrap();
        assert_eq!(sides[0].len(), 2);
        assert_eq!(sides[1].len(), 4);
        assert_eq!(
            build(&pool, &[Some(2), Some(3)], Method::Random, 0),
            Err(BuildError::OverCapacity {
                players: 6,
                capacity: 5
            })
        );
        let mut pinned = pool.clone();
        pinned[0].pinned_to = Some(0);
        pinned[1].pinned_to = Some(0);
        assert_eq!(
            build(&pinned, &[Some(1), None], Method::Random, 0),
            Err(BuildError::PinnedOverCap(0))
        );
        pinned[2].pinned_to = Some(5);
        assert_eq!(
            build(&pinned, &[None, None], Method::Random, 0),
            Err(BuildError::BadPin(5))
        );
        assert_eq!(
            build(&pool, &[], Method::Random, 0),
            Err(BuildError::NoSides)
        );
    }

    #[test]
    fn two_side_balance_reports_an_honest_range() {
        let pool = vec![
            slot(27.0, 3.0),
            slot(23.0, 3.0),
            slot(25.0, INITIAL_SIGMA),
            slot(25.0, 3.0),
        ];
        let b = balance(&pool, &[vec![0, 1], vec![2, 3]]);
        let w = b.win_probability.unwrap();
        assert!((w.expected - 0.5).abs() < 1e-9);
        assert!(w.low < 0.5 && w.high > 0.5);
        assert!(
            ((w.low + w.high) - 1.0).abs() < 1e-9,
            "symmetric about an even game"
        );
        assert_eq!(b.verdict, Verdict::Even);

        let lopsided = balance(&pool, &[vec![0, 3], vec![1, 2]]);
        assert!(lopsided.win_probability.unwrap().expected > 0.5);
    }

    #[test]
    fn larger_pools_use_the_hill_climb_and_stay_valid() {
        // 3 sides, 24 players: not the exact path.
        let pool: Vec<PoolSlot> = (0..24)
            .map(|i| slot(18.0 + (i as f64 * 0.7) % 13.0, 3.0 + (i % 3) as f64))
            .collect();
        let caps = [None; 3];
        let sides = build(&pool, &caps, Method::AutoBalanced, 11).unwrap();
        assert_valid(&pool, &sides, &caps);
        assert!(balance(&pool, &sides).spread <= 15.0);
    }
}
