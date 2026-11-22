//! The rating engine's constants the team builder needs, and the mapping
//! between its native Weng-Lin `μ`/`σ` and the displayed numbers the API
//! serves.
//!
//! The engine itself (`skillratings`, storage, repair) lives in the backend
//! crate `agon_services_core::rating` and can't come here — it isn't
//! wasm-light. These values mirror its `engine` and `scale` modules exactly;
//! `agon_service`'s `team_builder_scale_matches_the_rating_engine` test,
//! which can see both crates, fails if they ever drift apart.

/// Native `μ` a new player starts at (`rating::INITIAL_MU`).
pub const INITIAL_MU: f64 = 25.0;
/// Native `σ` a new player starts at (`rating::INITIAL_SIGMA`).
pub const INITIAL_SIGMA: f64 = 25.0 / 3.0;
/// Per-player performance noise (`rating::BETA`).
pub const BETA: f64 = 25.0 / 6.0;
/// Ranked games before a rating is published (`rating::PLACEMENT_MATCHES`).
pub const PLACEMENT_MATCHES: u64 = 5;
/// Displayed rating of an average, unproven player (`rating::RATING_ORIGIN`).
pub const RATING_ORIGIN: f64 = 1500.0;
/// Display points per native `μ` (`rating::SCALE`).
pub const SCALE: f64 = 30.0;
/// The displayed `±` is this many `σ` (`rating::CONFIDENCE_SIGMAS`).
pub const CONFIDENCE_SIGMAS: f64 = 3.0;

/// Displayed rating for a native `μ`, unrounded.
#[must_use]
pub fn display_rating(mu: f64) -> f64 {
    RATING_ORIGIN + (mu - INITIAL_MU) * SCALE
}

/// Native `μ` for a displayed rating.
#[must_use]
pub fn mu_from_display(rating: f64) -> f64 {
    INITIAL_MU + (rating - RATING_ORIGIN) / SCALE
}

/// Native `σ` for a displayed `±` half-width.
#[must_use]
pub fn sigma_from_display(confidence: f64) -> f64 {
    confidence / (CONFIDENCE_SIGMAS * SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trips() {
        assert!((mu_from_display(display_rating(31.7)) - 31.7).abs() < 1e-9);
        assert!((sigma_from_display(CONFIDENCE_SIGMAS * SCALE * 4.2) - 4.2).abs() < 1e-9);
        assert!((display_rating(INITIAL_MU) - RATING_ORIGIN).abs() < 1e-9);
    }
}
