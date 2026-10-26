//! Publication — whose rating anyone may see, and what confidence a player
//! enters ranked play with.
//!
//! Every player is rated. A rating is computed from the first completed match
//! in a rated sport, whether or not that match was ranked and whether or not
//! the player has ever opted in, because a rating the system keeps to itself
//! is what makes the rest of the design possible: a ringer has a real strength
//! to enter the maths at, so nobody has to be refused, asked, or approximated
//! with a placeholder.
//!
//! What opting in buys is not a rating. It is **publication**, and the two
//! rules below are the whole of it:
//!
//! ```text
//! persist   = ranked ∨ ¬opted_in     casual stops counting once you opt in
//! placement = ranked ∧ opted_in      only post-opt-in ranked play places you
//! ```
//!
//! The second implies the first, which is worth stating because they are
//! applied in different places (the write filter and the counter) and a drift
//! between them would silently publish a player off matches that were never
//! persisted. `tests::placing_a_match_implies_persisting_it` pins it.
//!
//! ## Why the prior is kept and the confidence is not
//!
//! Opting in resets `σ`, never `μ` — see [`sigma_on_entry`]. The alternative
//! people reach for first is a *reset to baseline that climbs back towards the
//! old number*, and it is worse in three ways: it displays a number already
//! known to be wrong, it converges to a value that was computed before the
//! journey started, and during the climb a strong player reads as weak, which
//! is a smurf window designed in rather than exploited.
//!
//! Throwing away the confidence does the same job honestly. The prior came
//! from casual play — scores nobody disputes, people out of position, someone
//! carrying a beginner — so it is worth something as a starting point and
//! close to nothing as a statement of certainty. A fresh `σ` says exactly
//! that, and because Weng-Lin updates scale with `σ`, it also means a handful
//! of real results move the player a long way. That is the placement period
//! doing its work, out of the engine's own machinery rather than bolted on
//! beside it.
//!
//! ## Why nothing here is stored
//!
//! There is no status field, no state machine, and no type naming the stages
//! a player passes through. [`published`] is a two-line function of two facts
//! already on the profile — whether `rating_opt_ins` holds the ladder, and
//! `RatingRecord::ranked_matches_rated` — so a player cannot be left in a
//! state their rating contradicts, and nothing has to be migrated when the
//! rules change.
//!
//! It also means the confidence reset cannot fire twice without anything
//! having to remember that it fired. The counter only ever advances on
//! post-opt-in ranked play, so it is `0` at the moment someone opts in and
//! non-zero immediately after their first ranked match, which is exactly the
//! window [`sigma_on_entry`] tests for. `opt_in_to_rating` writes one flag and
//! nothing else.

use super::engine::INITIAL_SIGMA;

/// Ranked matches, after opting in, before a rating is shown to anyone.
///
/// Five is a product choice rather than a derived one. A `σ` threshold would
/// be the principled version — publish once we are actually confident — but it
/// cannot answer "how many more?", and a placement period whose end nobody can
/// see is worse than one that ends slightly too early. The count is legible;
/// the fresh `σ` from [`sigma_on_entry`] is what makes five of them enough.
///
/// Retunable, but **forward-only**: the reset is applied at rate time and the
/// result is stored, so changing this moves future opt-ins and leaves settled
/// players where they are. That is unlike `scale`, which is genuinely
/// retunable by deploy because no display rating is ever stored.
pub const PLACEMENT_MATCHES: u64 = 5;

/// Whether this ladder's rating may be shown to anyone.
///
/// The single question every read boundary asks. Anything that serialises a
/// rating — a profile response, a match card's `+18`, a ladder row — gates on
/// this, and on nothing narrower. Reading the two stored facts off a record:
///
/// ```ignore
/// published(user.has_opted_in(ladder), rating.ranked_matches_rated)
/// ```
///
/// The opt-in alone is the wrong gate, because a player who has just opted in
/// has had their `σ` thrown away by [`sigma_on_entry`] and is about to move by
/// hundreds of points over a handful of games. That swing is the placement
/// period working as intended and is nobody's business while it happens.
///
/// The counter alone is *currently* sufficient — it only advances while opted
/// in, and there is no way to opt back out — but it is sufficient by way of an
/// invariant held in a different crate's worker handler. Asking both costs a
/// map lookup on a record already in hand, and means a change to
/// [`counts_towards_placement`] cannot quietly start publishing ratings that
/// were promised to nobody.
#[must_use]
pub fn published(opted_in: bool, ranked_matches_rated: u64) -> bool {
    opted_in && ranked_matches_rated >= PLACEMENT_MATCHES
}

/// Whether this match's movement is written to this player's rating at all.
///
/// `ranked ∨ ¬opted_in`. A player who has not opted in is rated by everything
/// they play, which is what builds the prior. From the moment they opt in,
/// only ranked matches move them — casual scores are noisy in a way that is
/// perfectly acceptable for a hidden prior and not acceptable for a number
/// people are judged on.
///
/// Note what this does *not* say. A player whose movement is discarded is
/// still an **input** to everyone else's: their rating feeds the maths, they
/// simply do not receive an update from it. The engine rates the whole match
/// either way; this filters the writes.
#[must_use]
pub fn counts_towards_rating(ranked: bool, opted_in: bool) -> bool {
    ranked || !opted_in
}

/// Whether this match advances the player's placement progress.
///
/// `ranked ∧ opted_in` — only ranked play, and only after opting in. Ranked
/// matches played *before* opting in deliberately do not count towards
/// placements: were they counted, someone who had turned out as a ringer
/// twenty times would opt in and be published immediately, at the very moment
/// [`sigma_on_entry`] has made their rating least settled. They would see a
/// number swinging by hundreds of points and so would everyone else.
#[must_use]
pub fn counts_towards_placement(ranked: bool, opted_in: bool) -> bool {
    ranked && opted_in
}

/// The `σ` a player carries **into** a match, given the one stored against
/// them.
///
/// Everything but the opt-in boundary returns `stored` untouched. At that one
/// boundary — opted in, no ranked match yet folded in — it returns
/// [`INITIAL_SIGMA`]: the prior is kept, the confidence is discarded. The
/// module doc argues why that is the right shape; this is the whole of its
/// implementation.
///
/// It is applied here, at rate time, rather than written by `opt_in_to_rating`
/// for two reasons. The profile is a single item, so the opt-in write and a
/// `SET ratings.<ladder>.sigma` cannot be transacted together, and a crash
/// between them would leave a silently un-inflated rating — the kind of bug
/// that surfaces months later as "why did this player never move". And the
/// ladder may have no rating to update at all, which no single conditional
/// expression handles cleanly.
///
/// Deriving it instead keeps opt-in a single conditional write and keeps this
/// a pure function of stored state, which is what lets `RepairRatings` replay
/// a ladder and land on the same numbers the incremental path produced. See
/// `engine::tests::replaying_from_scratch_matches_incremental_rating`.
#[must_use]
pub fn sigma_on_entry(stored: f64, opted_in: bool, ranked_matches_rated: u64) -> f64 {
    if opted_in && ranked_matches_rated == 0 {
        INITIAL_SIGMA
    } else {
        stored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two rules are applied in different places — one filters writes, the
    /// other advances a counter — so nothing in the type system stops them
    /// drifting. If a match could ever place a player without being persisted
    /// for them, publication would be reachable off matches that left no
    /// history, and the repair replay would have nothing to rebuild it from.
    #[test]
    fn placing_a_match_implies_persisting_it() {
        for ranked in [true, false] {
            for opted_in in [true, false] {
                if counts_towards_placement(ranked, opted_in) {
                    assert!(
                        counts_towards_rating(ranked, opted_in),
                        "ranked={ranked} opted_in={opted_in} places but does not persist"
                    );
                }
            }
        }
    }

    /// The prior is built out of casual play, so a player who has not opted in
    /// has to be rated by matches nobody called ranked. This is the rule that
    /// makes a ringer's rating real by the time they are first needed as one.
    #[test]
    fn casual_play_builds_a_latent_players_prior() {
        assert!(counts_towards_rating(false, false));
        assert!(counts_towards_rating(true, false));
    }

    /// ...and stops the moment they opt in. The scores in a friendly are not
    /// checked by anyone, which is tolerable for a hidden prior and not for a
    /// published number.
    #[test]
    fn casual_play_stops_counting_at_opt_in() {
        assert!(!counts_towards_rating(false, true));
        assert!(counts_towards_rating(true, true));
    }

    /// Opting in starts placements from zero even for someone with a long
    /// ranked history as a ringer — see [`counts_towards_placement`] for why
    /// publishing them immediately would be the worst possible moment to do
    /// it.
    #[test]
    fn ranked_play_before_opting_in_does_not_place_you() {
        assert!(!counts_towards_placement(true, false));
        // The counter cannot have advanced, so however much ranked play is
        // behind them they enter placements at nought.
        assert!(!published(false, 0));
        assert!(!published(true, 0));
    }

    /// The counter is zero at the moment of opt-in by construction, so
    /// `opt_in_to_rating` has nothing to reset and nothing has to remember
    /// that the reset already happened.
    #[test]
    fn the_confidence_reset_fires_once_and_only_before_the_first_ranked_match() {
        let settled = 2.0;
        assert_eq!(sigma_on_entry(settled, true, 0), INITIAL_SIGMA);
        for played in 1..=PLACEMENT_MATCHES + 3 {
            assert_eq!(
                sigma_on_entry(settled, true, played),
                settled,
                "reset fired again at {played} ranked matches"
            );
        }
    }

    /// A player who never opted in keeps whatever `σ` their casual play has
    /// made. The reset belongs to opting in, so applying it to someone who
    /// never did would widen a belief we have no reason to doubt.
    #[test]
    fn a_latent_player_keeps_the_confidence_their_play_earned() {
        assert_eq!(sigma_on_entry(2.0, false, 0), 2.0);
    }

    /// Publication is the question every read boundary asks, and the opt-in
    /// alone is the wrong answer to it: mid-placement ratings are moving by
    /// design and must not be shown.
    #[test]
    fn only_a_placed_player_is_published() {
        assert!(!published(false, 0));
        assert!(!published(false, 99));
        for played in 0..PLACEMENT_MATCHES {
            assert!(
                !published(true, played),
                "published after only {played} placement matches"
            );
        }
        assert!(published(true, PLACEMENT_MATCHES));
        assert!(published(true, PLACEMENT_MATCHES + 40));
    }
}
