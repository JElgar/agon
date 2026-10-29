//! Placement — how many ranked matches it takes before a rating is shown.
//!
//! Playing a ranked match is the whole of the agreement. Any account in one
//! is rated for it; there is nothing to opt in to, nothing to consent to and
//! nobody to refuse, which is what lets a ringer turn out for a side without
//! the maths having an opinion about it. Friendlies move nobody, so a rating
//! is built only out of games its owner knowingly joined as ranked.
//!
//! The one thing that is withheld is the number itself, and only for a
//! while. Until [`PLACEMENT_MATCHES`] ranked matches are behind it a rating
//! is maintained and shown to nobody — not to other players, and not to the
//! player it belongs to.
//!
//! ## Why withhold it at all
//!
//! Because a new rating is not wrong, it is *unknown*, and the display says
//! so in a way nobody reads as a caveat. A player's first ranked match is
//! rated from [`INITIAL_SIGMA`], which through `scale` is:
//!
//! ```text
//! 1500 ±750     brand new          one win moves it +79
//! 1500 ±180     ~settled (σ = 2)   one win moves it  +9
//! ```
//!
//! A profile reading "1500 ±750" is a claim that someone is somewhere
//! between 750 and 2250, lurching eighty points a game. That is the engine
//! being honest rather than the engine misbehaving — `σ` is doing exactly
//! its job — but publishing it invites people to read a number that is not
//! yet a statement about anything. `tests::a_brand_new_rating_is_too_wide_to
//! _publish` pins the figures so this stops being true loudly rather than
//! quietly.
//!
//! The period has a second effect worth keeping on purpose: somebody who
//! rings in for one ranked game, or three, never acquires a public rating at
//! all. Being on the board takes a pattern of turning up, not an accident.
//!
//! ## Why a count and not a `σ` threshold
//!
//! Publishing once `σ` drops below some bound is the principled version — it
//! withholds the rating for exactly as long as it is unknown, which is the
//! actual reason to withhold it. It is rejected because it cannot answer
//! "how many more?". A placement period whose end is unpredictable is worse
//! to sit inside than one that ends slightly too early, and `σ` falls at a
//! rate that depends on who you played, so two people who started together
//! would finish apart with no explanation available to either.
//!
//! ## Why nothing here is stored
//!
//! [`published`] is a comparison against `RatingRecord::matches_rated`,
//! which the rating write already maintains. There is no status field, no
//! state machine and no stage a player can be left in that their rating
//! contradicts, so retuning [`PLACEMENT_MATCHES`] moves everybody at once
//! and migrates nothing.

/// Ranked matches behind a rating before anyone may see it.
///
/// A product choice, not a derived one — see the module doc on why it is a
/// count rather than a `σ` bound. Retunable by deploy in both directions,
/// because it is compared against a stored counter rather than baked into
/// one, which puts it alongside `scale` and unlike anything that writes.
pub const PLACEMENT_MATCHES: u64 = 5;

/// Whether this rating may be shown to anyone.
///
/// The single question every read boundary asks. Anything that serialises a
/// rating — a profile response, a match card's `+18`, a ladder row — gates on
/// this, and the answer is the same for the player themselves as for
/// everybody else. Showing someone their own unplaced number would undo the
/// point of withholding it: they would arrive at their fifth ranked match
/// already anchored to a figure that was never a statement about anything.
#[must_use]
pub fn published(matches_rated: u64) -> bool {
    matches_rated >= PLACEMENT_MATCHES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::engine::{
        INITIAL_MU, INITIAL_SIGMA, PlayerRating, RatedPlayer, RatingSide, rate_sides,
    };
    use crate::rating::scale::display;

    /// The module doc argues from two numbers: a brand-new rating shows as
    /// `±750` and moves ~80 points a game, against `±180` and ~9 once it has
    /// settled. Both come out of `INITIAL_SIGMA` and `scale`, either of which
    /// could be retuned by someone who never reads this file. If the gap
    /// closes, the placement period is protecting against something that no
    /// longer happens and this test should be the one that says so.
    #[test]
    fn a_brand_new_rating_is_too_wide_to_publish() {
        let settled = 2.0;
        assert_eq!(display(INITIAL_MU, INITIAL_SIGMA).confidence, 750);
        assert_eq!(display(INITIAL_MU, settled).confidence, 180);

        let swing = |sigma: f64| {
            let side = |id: &str| RatingSide {
                side_id: id.to_string(),
                players: vec![RatedPlayer {
                    competitor_id: id.to_string(),
                    rating: PlayerRating {
                        mu: INITIAL_MU,
                        sigma,
                    },
                }],
            };
            let updates = rate_sides(&[side("a"), side("b")], Some("a")).unwrap();
            updates[0].display_delta()
        };
        assert_eq!(swing(INITIAL_SIGMA), 79);
        assert_eq!(swing(settled), 9);
    }

    /// The gate itself. Five is the first count that publishes, and the
    /// boundary is the only interesting thing about the function.
    #[test]
    fn a_rating_is_shown_from_its_fifth_ranked_match() {
        for played in 0..PLACEMENT_MATCHES {
            assert!(!published(played), "published after only {played}");
        }
        assert!(published(PLACEMENT_MATCHES));
        assert!(published(PLACEMENT_MATCHES + 40));
    }
}
