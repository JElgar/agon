//! The display scale — native Weng-Lin `μ`/`σ` to the Elo-shaped numbers a
//! player actually reads.
//!
//! `μ₀ = 25`, `σ₀ = 8.33` mean nothing to anyone. Everything is stored and
//! computed natively; this mapping is applied at the read boundary only, so
//! retuning it never rewrites a stored rating.
//!
//! ```text
//! rating     = 1500 + (μ − 25) × 30     a new player reads exactly 1500
//! confidence = ±3σ × 30                 new: ±750;  settled: ≈ ±180
//! floor      = rating − confidence      the conservative value
//! ```
//!
//! `SCALE = 30` is not arbitrary: it is picked so the engine's `β` (`25/6`,
//! the skill gap giving a ~67% win rate) lands on **125 display points**,
//! deliberately close to Elo's own ~120. The numbers then *feel* like Elo to
//! anyone who knows Elo, and — more usefully — band widths can be stated in
//! win probability (see `bands`). The two constants are therefore coupled:
//! changing `β` without changing `SCALE` silently changes what a band means.
//! `tests::beta_maps_to_the_elo_like_gap_the_scale_was_chosen_for` fails if
//! they drift apart.

use super::engine::INITIAL_MU;

/// What a brand-new player reads. `μ₀` maps here exactly.
pub const RATING_ORIGIN: f64 = 1500.0;

/// Display points per native rating point. See the module doc for why 30.
pub const SCALE: f64 = 30.0;

/// How many `σ` the quoted `±` covers — 3, so ~99.7% of the belief. Wide on
/// purpose: the number is used to gate entry ("this game is 1400–1600") via
/// the floor, and a gate should be conservative about players it isn't sure
/// about, not optimistic.
pub const CONFIDENCE_SIGMAS: f64 = 3.0;

/// The three numbers shown together for one rating, rounded to whole points.
///
/// Rounded here rather than by each caller so they are guaranteed consistent
/// with each other: `floor` is `rating - confidence` computed **after**
/// rounding, not a separately-rounded exact floor. Otherwise a profile
/// reading "1520 ±180" could sit next to a floor of 1341, and someone would
/// reasonably conclude one of the three was lying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRating {
    /// The headline number. What the band is derived from.
    pub rating: i32,
    /// The `±` half-width.
    pub confidence: i32,
    /// `rating - confidence`. The conservative value, used for gating and
    /// leaderboards so an unproven player can't sit at the top on variance
    /// alone.
    pub floor: i32,
}

/// The displayed rating for a native `μ`, unrounded.
#[must_use]
pub fn display_rating(mu: f64) -> f64 {
    RATING_ORIGIN + (mu - INITIAL_MU) * SCALE
}

/// The displayed `±` half-width for a native `σ`, unrounded.
#[must_use]
pub fn display_confidence(sigma: f64) -> f64 {
    CONFIDENCE_SIGMAS * sigma * SCALE
}

/// The full displayed triple for a native `μ`/`σ`.
#[must_use]
pub fn display(mu: f64, sigma: f64) -> DisplayRating {
    let rating = display_rating(mu).round() as i32;
    let confidence = display_confidence(sigma).round() as i32;
    DisplayRating {
        rating,
        confidence,
        floor: rating - confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::engine::{BETA, INITIAL_SIGMA, PlayerRating};

    /// Everyone starts at exactly 1500 — not 1499.99, not "about 1500". The
    /// band table centres 1500 in the middle band precisely so starting
    /// players sit nowhere near a boundary, and that only holds if the origin
    /// is exact.
    #[test]
    fn a_new_player_reads_exactly_1500() {
        let new = PlayerRating::default();
        assert_eq!(display_rating(new.mu), 1500.0);
        assert_eq!(display(new.mu, new.sigma).rating, 1500);
    }

    /// The reason `SCALE` is 30 and not any other number: `β` — the gap that
    /// buys a ~67% win rate — has to land near Elo's own ~120-point
    /// equivalent, so the scale reads as Elo to people who know Elo. 125 here
    /// (`25/6 × 30`), to the last bit an `f64` can hold: `25/6` is not
    /// exactly representable, so the product is 125.000000000000014, not
    /// 125.0. If someone retunes `β` for a sport and leaves `SCALE` alone,
    /// this is the test that says so.
    #[test]
    fn beta_maps_to_the_elo_like_gap_the_scale_was_chosen_for() {
        assert!(
            (BETA * SCALE - 125.0).abs() < 1e-9,
            "beta maps to {}",
            BETA * SCALE
        );
    }

    /// A brand-new player's belief is enormous and the display must say so —
    /// ±750, i.e. the floor of an unplayed account is 750, far below any
    /// gated game. This is what stops a fresh account gate-crashing an
    /// 1800-rated match on its nominal 1500.
    #[test]
    fn a_new_players_confidence_is_the_full_advertised_750() {
        let d = display(INITIAL_MU, INITIAL_SIGMA);
        assert_eq!(d.confidence, 750);
        assert_eq!(d.floor, 750);
    }

    /// A settled player (σ ≈ 2) reads ≈ ±180 — the other end of the range
    /// quoted in the design. Loose bound, since σ is emergent, not chosen.
    #[test]
    fn a_settled_players_confidence_is_around_180() {
        let d = display(INITIAL_MU, 2.0);
        assert_eq!(d.confidence, 180);
    }

    /// The three numbers are shown side by side, so they have to agree
    /// arithmetically after rounding — see `DisplayRating`. Swept across
    /// values chosen to land the exact floor on either side of a .5.
    #[test]
    fn floor_is_exactly_rating_minus_confidence_after_rounding() {
        for mu in [12.345, 25.0, 31.007, 40.99] {
            for sigma in [0.517, 2.0, 8.333_333_333_333_334] {
                let d = display(mu, sigma);
                assert_eq!(
                    d.floor,
                    d.rating - d.confidence,
                    "mu={mu} sigma={sigma} must stay self-consistent"
                );
            }
        }
    }

    /// The mapping is linear and strictly increasing, so ordering by μ and
    /// ordering by display rating are the same ordering — leaderboards can
    /// sort on either without disagreeing.
    #[test]
    fn the_mapping_is_monotonic_in_mu() {
        let mut previous = f64::NEG_INFINITY;
        for step in 0..100 {
            let mu = f64::from(step) * 0.5;
            let rating = display_rating(mu);
            assert!(rating > previous, "mu={mu} broke monotonicity");
            previous = rating;
        }
    }

    /// One native point is 30 display points, in both directions — the
    /// sanity check that catches a sign or origin slip in the affine map.
    #[test]
    fn one_native_point_is_thirty_display_points() {
        assert_eq!(display_rating(26.0) - display_rating(25.0), 30.0);
        assert_eq!(display_rating(24.0), 1470.0);
    }
}
