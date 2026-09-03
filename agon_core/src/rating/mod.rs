//! Player and team ratings — the pure engine.
//!
//! Everything in here is a pure function: no DynamoDB, no Meilisearch, no
//! clock, no ids minted. Give it ratings in, get ratings out. That is
//! deliberate and load-bearing rather than tidiness: the repair path
//! (`RepairRatings`) has to be able to replay a player's whole history from
//! stored history items and land on exactly the numbers the incremental path
//! produced. That equality is only checkable — and only *true* — if rating a
//! match depends on nothing but its inputs. See
//! `engine::tests::replaying_from_scratch_matches_incremental_rating`, which
//! is the test the repair design rests on.
//!
//! The pieces, one file each:
//!
//! - [`ladder`] — which pool a result counts towards. The one place a ladder
//!   key is minted, so a new sport is a compile error in exactly one match
//!   arm and an opaque string everywhere else.
//! - [`engine`] — the Weng-Lin wrapper. 1v1, team-vs-team and N-way, draws
//!   included, all through one code path.
//! - [`scale`] — native `μ`/`σ` to the Elo-shaped numbers players see.
//! - [`bands`] — the named tiers (`Advanced`, …), derived at read time from
//!   a data table and never stored.
//!
//! The plan filed this as a single `rating/mod.rs`. Split per concern
//! instead, matching how `dao/` is laid out — the four jobs above have
//! nothing to say to each other beyond their types, and the band table in
//! particular is meant to be a file you can open, retune, and close.
//!
//! ## What this module does *not* know
//!
//! It has no idea what a `MatchRecord` is. The projection into rating groups
//! takes loose [`MatchParticipant`]s (`competitor_id` + `side_id`), not
//! records, for two reasons:
//!
//! 1. There is no single record to take. A match's sides live on
//!    `MatchRecord`, but its *rosters* do not — `MatchSideRecord::
//!    roster_preview` is capped at `ROSTER_PREVIEW_CAP` (4), so an 11-a-side
//!    football match's players are only in the separate `MATCH#/PLAYER#`
//!    items. Any record-shaped signature would already be a pair.
//! 2. Eligibility — `ranked`, `Completed`, every side confirmed, every player
//!    linked to a real account — is a question about stored state, and the
//!    answer to it is what decides whether the engine is called at all. That
//!    filter belongs at the storage boundary with the records, not in here.
//!
//! So the record → participant adaptation lives in the phase-2 worker
//! handler, next to the eligibility check that produces it. The cost of that
//! choice is that the adapter is not covered by this module's unit tests; the
//! benefit is that the engine's tests construct three-line participants
//! instead of eighteen-field match records, and stay honest about what
//! they're testing.

pub mod bands;
pub mod engine;
pub mod ladder;
pub mod scale;

pub use bands::{
    Band, BandScale, BandStep, DEFAULT_BANDS, PLACEMENT_MATCHES, Placement, bands_for,
};
pub use engine::{
    BETA, INITIAL_MU, INITIAL_SIGMA, MatchParticipant, PlayerRating, RatedPlayer, RatingError,
    RatingSide, RatingTable, RatingUpdate, UNCERTAINTY_TOLERANCE, apply, group_by_side, rate_match,
    rate_sides,
};
pub use ladder::{Ladder, Sport, ladder_for, ladder_for_tag};
pub use scale::{
    CONFIDENCE_SIGMAS, DisplayRating, RATING_ORIGIN, SCALE, display, display_confidence,
    display_rating,
};
