//! Auto team builder — split a pool of players into squads or sides,
//! at random or balanced.
//!
//! Lives in this shared crate so the browser runs exactly the code the
//! tests cover: `agon_core_wasm` exposes [`plan::plan`] to `agon_ui`, which
//! previews and re-rolls locally and applies the result through the
//! ordinary roster/squad endpoints. Pure: no I/O, no clock, and the only
//! randomness is the caller's seed — same input and seed, same split.
//!
//! - [`strength`] turns each player's history (published rating, win/draw/
//!   loss record, or nothing) into an estimate `μ` and an uncertainty `σ`.
//!   Rating counts more with every ranked game, fully from 10.
//! - [`solver`] splits players between sides. Random shuffles and deals.
//!   Auto Balanced evens out `Σμ` first, then spreads the uncertain players.
//! - [`plan`] is the one-call entry point: estimate, split, measure, label.
//! - [`scale`] mirrors the rating engine's constants and display mapping.
//!
//! See `docs/auto-team-builder.md` for the product decisions.

pub mod plan;
pub mod scale;
pub mod solver;
pub mod strength;

pub use plan::{PlanInput, PlanOutput, plan};
