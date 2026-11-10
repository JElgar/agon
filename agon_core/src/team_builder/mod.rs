//! Auto team builder — the pure core behind "Auto-assign".
//!
//! An organiser has a pool of players and some sides (a teamless
//! tournament's squads, or an intra-squad match's sides) and wants the pool
//! split between them, either at random or balanced. This module is the
//! whole of the decision; the DAO reads the pool and writes the result, and
//! the service decides who may ask.
//!
//! Same rule as `rating::engine` and `tournament`: no DynamoDB, no clock, no
//! ids minted, and the only randomness is a caller-supplied seed. Same input
//! and seed, same output — which is what makes "re-roll" a new seed and
//! keeps every test deterministic.
//!
//! Two files, one job each:
//!
//! - [`strength`] turns each player's history (rating on the sport's ladder,
//!   win/draw/loss record, or nothing at all) into an [`strength::Estimate`]:
//!   a best guess `μ` on the rating engine's native scale, plus `σ`, how
//!   unsure that guess is. It is the only place the "rating counts more with
//!   every ranked game" policy lives.
//! - [`solver`] splits players between sides. Random shuffles and deals.
//!   Auto Balanced evens out expected strength (`Σμ`) first and then spreads
//!   the uncertain players (`Σσ²`), so no side ends up holding every unknown.
//!
//! See `docs/auto-team-builder.md` for the product decisions behind the
//! numbers.

pub mod solver;
pub mod strength;

pub use solver::{Balance, BuildError, Method, PoolSlot, Verdict, balance, build};
pub use strength::{Estimate, History, Level, RatingInput, Signal, WinRateInfo, estimate_pool};
