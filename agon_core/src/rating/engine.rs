//! The rating engine — a thin, deterministic wrapper over Weng-Lin.
//!
//! Weng-Lin (published as *A Bayesian Approximation Method for Online
//! Ranking*, and known online as OpenSkill) models each player as a Gaussian
//! belief `μ ± σ` and a team as the sum of its players' beliefs, so a team
//! result attributes credit back to individuals automatically. That is why
//! one engine covers 1v1 squash *and* 11-a-side football: a 1v1 is just a
//! two-side match with one player per side.
//!
//! It was chosen over the alternatives on capability and licence. Elo can't
//! express teams or confidence at all; Glicko-2 is the best-validated 1v1
//! system there is but is strictly two-player, which rules out football,
//! cricket and netball — three of the seven sports, and where most of this
//! codebase's effort has gone. TrueSkill does everything Weng-Lin does and is
//! ruled out purely on licensing: it is patented until 2029 and Microsoft
//! permits only Xbox Live titles and non-commercial projects.
//!
//! ## One code path for every shape of match
//!
//! `skillratings` offers `weng_lin` (1v1), `weng_lin_two_teams` and
//! `weng_lin_multi_team`. Everything here goes through `weng_lin_multi_team`,
//! including 1v1. The three are the same maths — for two teams the
//! multi-team loop reduces term-for-term to the two-team closed form, and for
//! one player per team that reduces again to the 1v1 form — so picking a
//! function per match shape would buy a rounding-level difference in exchange
//! for three branches that can each rot separately. The equivalence is not
//! assumed:
//! `tests::the_uniform_multi_team_path_matches_the_crates_dedicated_paths`
//! checks it against the crate's own published vectors.
//!
//! ## Determinism
//!
//! Ratings must be reproducible: repair replays a player's history and has to
//! land on the numbers the incremental path produced. Floating-point
//! summation is order-dependent, and the sides it starts from live in a
//! `HashMap` on `MatchRecord`, whose iteration order is not stable even
//! within one process. [`group_by_side`] therefore sorts sides by `side_id`
//! and players by `user_id` before any arithmetic happens — and
//! [`rate_sides`] sorts again itself rather than trusting that it was handed
//! sorted input, because it is public and `RatingSide`'s fields are public
//! too, so "the caller sorted" is a convention a caller can break without
//! the compiler noticing. Every rating this module produces is a function of
//! the participant *set*, never of the order it was handed in.

use std::collections::{BTreeMap, BTreeSet};

use skillratings::MultiTeamOutcome;
use skillratings::weng_lin::{WengLinConfig, WengLinRating, weng_lin_multi_team};
use thiserror::Error;

use super::scale::{DisplayRating, display, display_rating};

/// Where every player starts. Weng-Lin's own `μ₀`; `scale` maps it to a
/// displayed 1500.
pub const INITIAL_MU: f64 = 25.0;

/// The uncertainty a player starts with — `μ₀/3`, i.e. "could plausibly be
/// anywhere", which displays as ±750.
pub const INITIAL_SIGMA: f64 = 25.0 / 3.0;

/// The skill-class width: the `μ` gap that buys a ~67% win rate.
///
/// Pinned to the crate's own default (`25/6`) rather than read from
/// `WengLinConfig::default()`, and that indirection is the point. A rating
/// system has to be reproducible against history written months earlier, so
/// a dependency bump must never be able to silently move every stored
/// rating; if upstream retunes its default,
/// `tests::the_pinned_config_still_matches_the_crate_default` fails and the
/// change becomes a decision rather than a surprise.
///
/// Also coupled to `scale::SCALE`: `BETA × SCALE` is the ~125 display points
/// the band widths are derived from. Retuning one means retuning the other.
pub const BETA: f64 = 25.0 / 6.0;

/// Lower clamp on the *variance multiplier* inside the uncertainty update —
/// not on `σ` itself, which is the reading the name invites. Upstream
/// computes `σ_after = σ_before · sqrt(max(1 − (σp²/σt²)·Δ, tol))`, so this
/// floors the bracketed term: the effective clamp is `σ_before / 1000` per
/// update, not an absolute `1e-6`. Worth stating plainly because anyone
/// retuning this to reach a particular `σ` floor would otherwise set a value
/// about six orders of magnitude away from what they meant, and the pinned-
/// config test only checks it still equals upstream's default — it would not
/// catch the misunderstanding. The crate's default.
pub const UNCERTAINTY_TOLERANCE: f64 = 0.000_001;

/// The config every rating in this system is computed with.
///
/// Deliberately not public: `WengLinConfig` is a third-party type, and
/// keeping it inside the module means swapping the engine later is a change
/// to this file rather than to every caller. The constants above are the
/// public surface.
fn config() -> WengLinConfig {
    WengLinConfig {
        beta: BETA,
        uncertainty_tolerance: UNCERTAINTY_TOLERANCE,
    }
}

/// One player's rating on one ladder: the Gaussian belief about their skill.
///
/// Our own type rather than a re-exported `WengLinRating` so the algorithm
/// stays an implementation detail of this module — records, the API and the
/// worker all speak `mu`/`sigma`, and nothing outside `rating/` names
/// `skillratings`. It also gives the fields the names the design and the
/// stored record use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerRating {
    /// The mean estimate of skill.
    pub mu: f64,
    /// The standard deviation — how unsure we are. Shrinks with every
    /// result; the pipeline inflates it again on inactivity.
    pub sigma: f64,
}

impl Default for PlayerRating {
    fn default() -> Self {
        Self {
            mu: INITIAL_MU,
            sigma: INITIAL_SIGMA,
        }
    }
}

impl PlayerRating {
    /// The numbers a player sees for this rating.
    #[must_use]
    pub fn display(&self) -> DisplayRating {
        display(self.mu, self.sigma)
    }
}

impl From<PlayerRating> for WengLinRating {
    fn from(rating: PlayerRating) -> Self {
        WengLinRating {
            rating: rating.mu,
            uncertainty: rating.sigma,
        }
    }
}

impl From<WengLinRating> for PlayerRating {
    fn from(rating: WengLinRating) -> Self {
        PlayerRating {
            mu: rating.rating,
            sigma: rating.uncertainty,
        }
    }
}

/// Every rated player's current rating on one ladder, by user id.
///
/// A `BTreeMap` rather than a `HashMap` purely so anything that iterates it
/// (replay, tests, log lines) is ordered — see the module doc on
/// determinism.
pub type RatingTable = BTreeMap<String, PlayerRating>;

/// One player's participation in a match: who they are and which side they
/// played for.
///
/// `user_id` is not optional. A match with an unlinked guest on any side is
/// not rated *at all* — an unlinked player has no rating to contribute and
/// none to receive, so rating around them would quietly credit their side's
/// result to whoever else was on it. That check belongs with the records
/// that can answer it (phase 2's eligibility gate), which is why by the time
/// participants reach here every one of them is a real account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchParticipant {
    pub user_id: String,
    pub side_id: String,
}

/// A participant with their rating going into the match resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct RatedPlayer {
    pub user_id: String,
    pub rating: PlayerRating,
}

/// One side of a match as a rating group.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingSide {
    pub side_id: String,
    /// Sorted by `user_id` — see the module doc on determinism.
    pub players: Vec<RatedPlayer>,
}

/// What a match did to one player's rating.
///
/// Owns its `user_id` rather than borrowing from the participants it came
/// from: the update outlives the projection in every real caller (it gets
/// written to three items and handed to a workflow), and a lifetime here
/// would spread through every signature that carries one for the sake of a
/// few short allocations per match.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingUpdate {
    pub user_id: String,
    pub before: PlayerRating,
    pub after: PlayerRating,
}

impl RatingUpdate {
    /// The movement as the player sees it — the "+18" on a match card.
    ///
    /// The difference of the two *rounded* display ratings, not a rounded
    /// difference, so that the delta always reconciles with the before and
    /// after numbers shown beside it.
    #[must_use]
    pub fn display_delta(&self) -> i32 {
        display_rating(self.after.mu).round() as i32 - display_rating(self.before.mu).round() as i32
    }
}

/// Why a match could not be rated.
///
/// Every variant is a data problem rather than a user error, and every one of
/// them is something `skillratings` would otherwise absorb silently:
/// `weng_lin_multi_team` returns its inputs unchanged for an empty side, and
/// an unrecognised winner would simply leave every side on the same rank —
/// i.e. score the match as a draw. Silence is the wrong answer for both; a
/// match that can't be rated correctly should fail loudly and be repaired,
/// not rate wrongly.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RatingError {
    /// Fewer than two sides — nothing to compare against.
    #[error("a rated match needs at least two sides, got {0}")]
    NotEnoughSides(usize),
    /// A side with no players on it.
    #[error("side `{0}` has no players")]
    EmptySide(String),
    /// One account on more than one side (or twice on one side) — the
    /// updates would disagree about their own rating.
    #[error("user `{0}` appears more than once in the same match")]
    DuplicatePlayer(String),
    /// `winner_side_id` names a side that isn't in the match.
    #[error("winning side `{0}` is not one of the match's sides")]
    UnknownWinnerSide(String),
}

/// Project a match's participants into one rating group per side, resolving
/// each player's current rating.
///
/// Sport-agnostic by construction: it only knows about sides and user ids, so
/// squash, cricket and an N-way ladder night all project the same way.
///
/// A player absent from `ratings` gets [`PlayerRating::default`] — their
/// first ever match on this ladder. That is a silent default on purpose:
/// "unrated" and "rated at exactly the starting value with maximum σ" are the
/// same state in this model, and forcing callers to seed the table first
/// would just move the same default somewhere less visible.
#[must_use]
pub fn group_by_side(participants: &[MatchParticipant], ratings: &RatingTable) -> Vec<RatingSide> {
    let mut by_side: BTreeMap<&str, Vec<RatedPlayer>> = BTreeMap::new();
    for participant in participants {
        by_side
            .entry(&participant.side_id)
            .or_default()
            .push(RatedPlayer {
                user_id: participant.user_id.clone(),
                rating: ratings
                    .get(&participant.user_id)
                    .copied()
                    .unwrap_or_default(),
            });
    }

    by_side
        .into_iter()
        .map(|(side_id, mut players)| {
            players.sort_by(|a, b| a.user_id.cmp(&b.user_id));
            RatingSide {
                side_id: side_id.to_string(),
                players,
            }
        })
        .collect()
}

/// Rate one match: apply its result to every participant's rating.
///
/// `winner_side_id` is `None` for a draw — every side ties. With more than
/// two sides a single winner means "one side first, all others tied for
/// second", which is exactly what a stored `confirmed_score.winner_side_id`
/// can express; full placings (1st/2nd/3rd) would need per-side ranks the
/// score record doesn't carry today, and this is where they would plug in.
///
/// Returns one update per participant, in the sides' own deterministic order.
pub fn rate_sides(
    sides: &[RatingSide],
    winner_side_id: Option<&str>,
) -> Result<Vec<RatingUpdate>, RatingError> {
    if sides.len() < 2 {
        return Err(RatingError::NotEnoughSides(sides.len()));
    }
    if let Some(empty) = sides.iter().find(|side| side.players.is_empty()) {
        return Err(RatingError::EmptySide(empty.side_id.clone()));
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for player in sides.iter().flat_map(|side| &side.players) {
        if !seen.insert(&player.user_id) {
            return Err(RatingError::DuplicatePlayer(player.user_id.clone()));
        }
    }
    if let Some(winner) = winner_side_id
        && !sides.iter().any(|side| side.side_id == winner)
    {
        return Err(RatingError::UnknownWinnerSide(winner.to_string()));
    }

    // Defensive ordering, not tidiness. The arithmetic below is
    // floating-point and therefore order-dependent: reversing the side order
    // or a side's player order moves results in the last bits.
    // `group_by_side` already sorts, but this function is `pub` and
    // `RatingSide`'s fields are too, so a caller that assembles sides itself
    // is not covered by that — and the repair path, reconstructing sides
    // from stored history items, is exactly such a caller. Replay equality
    // is asserted bit-for-bit, so a few ULPs is the difference between
    // "converged, nothing to do" and "rewrite every rating on every run".
    // Sorting here makes the module's determinism guarantee true by
    // construction rather than by convention.
    let mut ordered: Vec<&RatingSide> = sides.iter().collect();
    ordered.sort_by(|a, b| a.side_id.cmp(&b.side_id));
    let ordered_players: Vec<Vec<&RatedPlayer>> = ordered
        .iter()
        .map(|side| {
            let mut players: Vec<&RatedPlayer> = side.players.iter().collect();
            players.sort_by(|a, b| a.user_id.cmp(&b.user_id));
            players
        })
        .collect();

    let groups: Vec<Vec<WengLinRating>> = ordered_players
        .iter()
        .map(|players| players.iter().map(|p| p.rating.into()).collect())
        .collect();
    // Rank 1 for the winner, 2 for everybody else; all 1 for a draw. Equal
    // ranks are how Weng-Lin expresses a tie.
    let ranked: Vec<(&[WengLinRating], MultiTeamOutcome)> = ordered
        .iter()
        .zip(&groups)
        .map(|(side, group)| {
            let rank = match winner_side_id {
                Some(winner) if side.side_id != winner => 2,
                _ => 1,
            };
            (group.as_slice(), MultiTeamOutcome::new(rank))
        })
        .collect();

    let rated = weng_lin_multi_team(&ranked, &config());

    Ok(ordered_players
        .iter()
        .zip(rated)
        .flat_map(|(players, new_group)| {
            players
                .iter()
                .zip(new_group)
                .map(|(player, after)| RatingUpdate {
                    user_id: player.user_id.clone(),
                    before: player.rating,
                    after: after.into(),
                })
                .collect::<Vec<_>>()
        })
        .collect())
}

/// [`group_by_side`] then [`rate_sides`] — the whole engine in one call, and
/// what the worker and the repair workflow both use.
pub fn rate_match(
    participants: &[MatchParticipant],
    winner_side_id: Option<&str>,
    ratings: &RatingTable,
) -> Result<Vec<RatingUpdate>, RatingError> {
    rate_sides(&group_by_side(participants, ratings), winner_side_id)
}

/// Fold a match's results back into a rating table.
///
/// Trivial, and public anyway: it is what makes rating a live match and
/// replaying stored history *the same code*, which is the only reason the
/// two can be expected to agree (see
/// `tests::replaying_from_scratch_matches_incremental_rating`).
pub fn apply(updates: &[RatingUpdate], ratings: &mut RatingTable) {
    for update in updates {
        ratings.insert(update.user_id.clone(), update.after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skillratings::Outcomes;
    use skillratings::weng_lin::{weng_lin, weng_lin_two_teams};

    /// How close two `f64` ratings have to be to count as the same number.
    /// Loose enough to absorb the reassociation of a `mul_add` between two
    /// arrangements of the same arithmetic, tight enough that any real
    /// difference in the maths shows up.
    const EPSILON: f64 = 1e-12;

    fn assert_close(actual: f64, expected: f64, what: &str) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{what}: expected {expected}, got {actual}"
        );
    }

    fn participant(user_id: &str, side_id: &str) -> MatchParticipant {
        MatchParticipant {
            user_id: user_id.to_string(),
            side_id: side_id.to_string(),
        }
    }

    fn rating(mu: f64, sigma: f64) -> PlayerRating {
        PlayerRating { mu, sigma }
    }

    fn rated_player(user_id: &str, rating: PlayerRating) -> RatedPlayer {
        RatedPlayer {
            user_id: user_id.to_string(),
            rating,
        }
    }

    /// A 1v1: `a` on side `home`, `b` on side `away`.
    fn singles() -> Vec<MatchParticipant> {
        vec![participant("a", "home"), participant("b", "away")]
    }

    fn table(entries: &[(&str, PlayerRating)]) -> RatingTable {
        entries
            .iter()
            .map(|(id, rating)| ((*id).to_string(), *rating))
            .collect()
    }

    fn after(updates: &[RatingUpdate], user_id: &str) -> PlayerRating {
        updates
            .iter()
            .find(|u| u.user_id == user_id)
            .unwrap_or_else(|| panic!("no update for {user_id}"))
            .after
    }

    /// The engine's output has to match the algorithm as published, not just
    /// be self-consistent. These are `skillratings`' own documented vectors
    /// for two default players where the first wins — reproduced here through
    /// *our* uniform multi-team path, which is the thing under test.
    #[test]
    fn known_vector_one_v_one_matches_the_published_weng_lin_result() {
        let updates = rate_match(&singles(), Some("home"), &RatingTable::new())
            .expect("a two-side match is rateable");

        assert_close(after(&updates, "a").mu, 27.635_231_383_473_65, "winner mu");
        assert_close(
            after(&updates, "a").sigma,
            8.065_506_316_323_548,
            "winner sigma",
        );
        assert_close(after(&updates, "b").mu, 22.364_768_616_526_35, "loser mu");
        assert_close(
            after(&updates, "b").sigma,
            8.065_506_316_323_548,
            "loser sigma",
        );
    }

    /// Between two equally-uncertain sides the update is zero-sum in `μ` —
    /// the winner takes exactly what the loser gives up. Worth pinning
    /// because it is the property people assume Elo-like ratings have, and
    /// Weng-Lin only has it under that condition (the shift is weighted by
    /// each side's `σ²`, so a confident player beating an unproven one moves
    /// far less than the unproven one loses).
    #[test]
    fn between_equally_certain_players_the_update_is_zero_sum() {
        let updates = rate_match(&singles(), Some("home"), &RatingTable::new()).unwrap();
        let total = after(&updates, "a").mu + after(&updates, "b").mu;
        assert_close(total, 2.0 * INITIAL_MU, "total mu");
    }

    /// The module runs 1v1 and team matches through `weng_lin_multi_team`
    /// rather than the crate's dedicated `weng_lin` / `weng_lin_two_teams`
    /// entry points, on the claim that all three are the same maths. This is
    /// that claim, checked directly — if a future crate version specialises
    /// one of them, this fails instead of the ratings quietly drifting.
    #[test]
    fn the_uniform_multi_team_path_matches_the_crates_dedicated_paths() {
        // 1v1, mismatched players, all three outcomes.
        let strong = rating(42.0, 1.3);
        let weak = PlayerRating::default();
        for (winner, outcome) in [
            (Some("home"), Outcomes::WIN),
            (Some("away"), Outcomes::LOSS),
            (None, Outcomes::DRAW),
        ] {
            let ours =
                rate_match(&singles(), winner, &table(&[("a", strong), ("b", weak)])).unwrap();
            let (theirs_a, theirs_b) = weng_lin(&strong.into(), &weak.into(), &outcome, &config());

            assert_close(after(&ours, "a").mu, theirs_a.rating, "1v1 mu one");
            assert_close(
                after(&ours, "a").sigma,
                theirs_a.uncertainty,
                "1v1 sigma one",
            );
            assert_close(after(&ours, "b").mu, theirs_b.rating, "1v1 mu two");
            assert_close(
                after(&ours, "b").sigma,
                theirs_b.uncertainty,
                "1v1 sigma two",
            );
        }

        // 3v3, the crate's own documented team example.
        let home = [
            PlayerRating::default(),
            rating(30.0, 1.2),
            rating(21.0, 6.5),
        ];
        let away = [
            PlayerRating::default(),
            rating(41.0, 1.4),
            rating(19.2, 4.3),
        ];
        let participants: Vec<_> = ["h0", "h1", "h2"]
            .iter()
            .map(|id| participant(id, "home"))
            .chain(["a0", "a1", "a2"].iter().map(|id| participant(id, "away")))
            .collect();
        let ratings = table(&[
            ("h0", home[0]),
            ("h1", home[1]),
            ("h2", home[2]),
            ("a0", away[0]),
            ("a1", away[1]),
            ("a2", away[2]),
        ]);

        let ours = rate_match(&participants, Some("home"), &ratings).unwrap();
        let home_native: Vec<WengLinRating> = home.iter().map(|r| (*r).into()).collect();
        let away_native: Vec<WengLinRating> = away.iter().map(|r| (*r).into()).collect();
        let (theirs_home, theirs_away) =
            weng_lin_two_teams(&home_native, &away_native, &Outcomes::WIN, &config());

        for (index, id) in ["h0", "h1", "h2"].iter().enumerate() {
            assert_close(after(&ours, id).mu, theirs_home[index].rating, "team mu");
            assert_close(
                after(&ours, id).sigma,
                theirs_home[index].uncertainty,
                "team sigma",
            );
        }
        for (index, id) in ["a0", "a1", "a2"].iter().enumerate() {
            assert_close(after(&ours, id).mu, theirs_away[index].rating, "team mu");
            assert_close(
                after(&ours, id).sigma,
                theirs_away[index].uncertainty,
                "team sigma",
            );
        }
    }

    /// A dependency bump must not be able to move every rating in the
    /// database silently. `BETA`/`UNCERTAINTY_TOLERANCE` are pinned locally
    /// precisely so that upstream retuning its defaults shows up here as a
    /// failing test — and a deliberate decision about whether to follow —
    /// rather than as history that no longer replays.
    #[test]
    fn the_pinned_config_still_matches_the_crate_default() {
        let upstream = WengLinConfig::default();
        assert_eq!(config().beta, upstream.beta);
        assert_eq!(
            config().uncertainty_tolerance,
            upstream.uncertainty_tolerance
        );
    }

    // -----------------------------------------------------------------
    // Replay invariance — the property the repair design rests on.
    // -----------------------------------------------------------------

    /// One historical match, as replay sees it: who played where, and who
    /// won. Deliberately carries no ratings — replay pulls each player's
    /// value from the table as it walks forward, which is the entire point.
    struct Historic {
        participants: Vec<MatchParticipant>,
        winner_side_id: Option<&'static str>,
    }

    /// A mixed history: 1v1s, a 2v2, a three-way, wins and draws, with
    /// players recurring across matches so each one's rating going into a
    /// match depends on the ones before it.
    fn history() -> Vec<Historic> {
        vec![
            Historic {
                participants: vec![participant("ana", "red"), participant("ben", "blue")],
                winner_side_id: Some("red"),
            },
            Historic {
                participants: vec![participant("ben", "red"), participant("cleo", "blue")],
                winner_side_id: None,
            },
            Historic {
                participants: vec![
                    participant("ana", "red"),
                    participant("dev", "red"),
                    participant("ben", "blue"),
                    participant("cleo", "blue"),
                ],
                winner_side_id: Some("blue"),
            },
            Historic {
                participants: vec![
                    participant("ana", "red"),
                    participant("ben", "blue"),
                    participant("cleo", "green"),
                ],
                winner_side_id: Some("green"),
            },
            Historic {
                participants: vec![participant("dev", "red"), participant("ana", "blue")],
                winner_side_id: Some("red"),
            },
        ]
    }

    /// Rate `matches` in order from `start`, folding each result back in.
    /// The same two calls the worker makes per match and the repair workflow
    /// makes per replayed match — there is deliberately no second
    /// implementation to diverge from.
    fn replay(matches: &[Historic], start: RatingTable) -> RatingTable {
        let mut ratings = start;
        for historic in matches {
            let updates =
                rate_match(&historic.participants, historic.winner_side_id, &ratings).unwrap();
            apply(&updates, &mut ratings);
        }
        ratings
    }

    /// **The property the whole repair design rests on.** Repair replays a
    /// player's history from the point something changed and overwrites their
    /// rating with the result. That is only sound if replaying produces
    /// exactly what rating the matches one at a time produced — not
    /// approximately, exactly, since any drift would compound every time a
    /// re-score fires.
    ///
    /// It holds because the engine is a pure function of (ratings in,
    /// participants, winner) and orders its own inputs, so there is no state
    /// or iteration order for an incremental run to accumulate that a replay
    /// wouldn't.
    #[test]
    fn replaying_from_scratch_matches_incremental_rating() {
        let matches = history();

        // Incremental: one match at a time, carrying the table forward, as
        // the confirmation handler does.
        let mut incremental = RatingTable::new();
        for historic in &matches {
            let updates = rate_match(
                &historic.participants,
                historic.winner_side_id,
                &incremental,
            )
            .unwrap();
            apply(&updates, &mut incremental);
        }

        // From scratch: everyone back to default, the whole history again.
        let replayed = replay(&matches, RatingTable::new());

        assert_eq!(
            incremental, replayed,
            "replayed ratings must be bit-for-bit identical to incremental ones"
        );
        assert_eq!(incremental.len(), 4, "every participant ended up rated");
    }

    /// Repair doesn't replay from the beginning of time — it resumes from the
    /// last checkpoint before the affected match. So a replay of the tail,
    /// starting from the state as of that point, has to reach the same place
    /// as a replay of the whole thing.
    #[test]
    fn replaying_from_a_checkpoint_matches_a_full_replay() {
        let matches = history();
        let full = replay(&matches, RatingTable::new());

        for checkpoint in 0..=matches.len() {
            let prefix = replay(&matches[..checkpoint], RatingTable::new());
            let resumed = replay(&matches[checkpoint..], prefix);
            assert_eq!(resumed, full, "resuming after match {checkpoint} diverged");
        }
    }

    /// Sides arrive in a `HashMap` on `MatchRecord` and players in whatever
    /// order a query returned them, so the engine must not be able to see
    /// that order. If it could, two replays of the same history could
    /// disagree in the last decimal place — and repair compares for equality.
    #[test]
    fn the_result_does_not_depend_on_the_order_participants_arrive_in() {
        let ratings = table(&[
            ("h0", rating(28.0, 4.0)),
            ("h1", rating(21.5, 7.0)),
            ("a0", rating(25.0, 2.0)),
            ("a1", rating(31.0, 6.25)),
        ]);
        let forwards = vec![
            participant("h0", "home"),
            participant("h1", "home"),
            participant("a0", "away"),
            participant("a1", "away"),
        ];
        let shuffled = vec![
            participant("a1", "away"),
            participant("h1", "home"),
            participant("a0", "away"),
            participant("h0", "home"),
        ];

        let mut one = rate_match(&forwards, Some("away"), &ratings).unwrap();
        let mut two = rate_match(&shuffled, Some("away"), &ratings).unwrap();
        one.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        two.sort_by(|a, b| a.user_id.cmp(&b.user_id));

        assert_eq!(one, two, "participant order leaked into the result");
    }

    /// The same guarantee one level down, at the public `rate_sides` entry
    /// point. `rate_match` gets its ordering from `group_by_side`, so it
    /// would pass this even if `rate_sides` trusted its caller — but
    /// `rate_sides` is public and `RatingSide`'s fields are public, so the
    /// repair path can and will assemble sides itself from stored history
    /// items. Weng-Lin's arithmetic is floating-point and genuinely
    /// order-sensitive in the last bits; replay equality is asserted
    /// bit-for-bit, so an unsorted caller would make repair believe every
    /// rating had drifted and rewrite them on every single run.
    #[test]
    fn rate_sides_sorts_its_own_input_rather_than_trusting_the_caller() {
        // Deterministic LCG rather than a fixed hand-picked case. A single
        // 2v2 is not enough: order-sensitivity here is last-bit
        // floating-point behaviour that only *some* rating combinations
        // exhibit, so one case passes whether or not the sort exists — a
        // green test that guards nothing. Sweeping many shapes makes the
        // failure reliable. Fixed seed, so it is reproducible, not flaky.
        let mut seed = 0x2026_0902_u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((seed >> 33) as f64) / ((1u64 << 31) as f64)
        };

        for case in 0..250 {
            let side_count = 2 + (case % 3);
            let sides: Vec<RatingSide> = (0..side_count)
                .map(|s| RatingSide {
                    side_id: format!("side{s}"),
                    players: (0..2 + (case + s) % 3)
                        .map(|p| {
                            rated_player(
                                &format!("s{s}p{p}"),
                                rating(18.0 + next() * 14.0, 1.5 + next() * 7.0),
                            )
                        })
                        .collect(),
                })
                .collect();

            let reversed: Vec<RatingSide> = sides
                .iter()
                .rev()
                .map(|side| RatingSide {
                    side_id: side.side_id.clone(),
                    players: side.players.iter().rev().cloned().collect(),
                })
                .collect();

            let winner = Some("side0");
            let mut forwards = rate_sides(&sides, winner).unwrap();
            let mut backwards = rate_sides(&reversed, winner).unwrap();
            forwards.sort_by(|a, b| a.user_id.cmp(&b.user_id));
            backwards.sort_by(|a, b| a.user_id.cmp(&b.user_id));

            assert_eq!(
                forwards, backwards,
                "case {case}: side/player order leaked into rate_sides' arithmetic"
            );
        }
    }

    // -----------------------------------------------------------------
    // Draws, rosters, and multi-side matches.
    // -----------------------------------------------------------------

    /// A draw between identical players says nothing about who is better, so
    /// `μ` must not move at all — but it is still evidence that they are
    /// close, so `σ` must shrink. Getting this wrong (treating a draw as half
    /// a win, the way plain Elo has to) would drift both players every time
    /// they tied.
    #[test]
    fn a_draw_between_equals_moves_no_rating_but_still_buys_confidence() {
        let updates = rate_match(&singles(), None, &RatingTable::new()).unwrap();
        for update in &updates {
            assert_close(update.after.mu, INITIAL_MU, "drawn mu");
            assert!(
                update.after.sigma < update.before.sigma,
                "a draw is still evidence: sigma should shrink"
            );
        }
    }

    /// A draw against a stronger opponent is a good result and must be scored
    /// as one — the underdog gains, the favourite loses ground.
    #[test]
    fn a_draw_against_a_stronger_opponent_favours_the_underdog() {
        let ratings = table(&[("a", rating(35.0, 3.0)), ("b", rating(20.0, 3.0))]);
        let updates = rate_match(&singles(), None, &ratings).unwrap();

        assert!(after(&updates, "a").mu < 35.0, "the favourite should drop");
        assert!(after(&updates, "b").mu > 20.0, "the underdog should gain");
    }

    /// Uneven sides (3v2 — someone's mate didn't show) are rated on the same
    /// path as everything else, and the model handles them by construction: a
    /// side's strength is the *sum* of its players' `μ`, so the short side is
    /// the underdog and is rewarded accordingly for winning.
    ///
    /// Worth being explicit that additive strength is the semantics, not an
    /// accident. It is exactly right for 11-a-side football down to 10 men,
    /// and it is why a 3v2 kickabout between equals treats the pair as
    /// heavy underdogs.
    #[test]
    fn an_outnumbered_side_gains_more_from_a_win_than_an_even_one_would() {
        let three_v_two = vec![
            participant("h0", "home"),
            participant("h1", "home"),
            participant("h2", "home"),
            participant("a0", "away"),
            participant("a1", "away"),
        ];
        let two_v_two = vec![
            participant("h0", "home"),
            participant("h1", "home"),
            participant("a0", "away"),
            participant("a1", "away"),
        ];

        let outnumbered = rate_match(&three_v_two, Some("away"), &RatingTable::new()).unwrap();
        let even = rate_match(&two_v_two, Some("away"), &RatingTable::new()).unwrap();

        let outnumbered_gain = after(&outnumbered, "a0").mu - INITIAL_MU;
        let even_gain = after(&even, "a0").mu - INITIAL_MU;
        assert!(
            outnumbered_gain > even_gain,
            "beating a bigger side should be worth more: {outnumbered_gain} vs {even_gain}"
        );

        assert_eq!(outnumbered.len(), 5, "every player on both sides is rated");
        for id in ["h0", "h1", "h2"] {
            assert!(after(&outnumbered, id).mu < INITIAL_MU, "{id} lost");
        }
    }

    /// A free-for-all with more than two sides — a ladder night, a three-way
    /// round robin logged as one match. One winner, everyone else tied for
    /// second, which is all a single `winner_side_id` can express.
    #[test]
    fn a_multi_side_match_ranks_the_winner_first_and_ties_the_rest() {
        let participants = vec![
            participant("a", "one"),
            participant("b", "two"),
            participant("c", "three"),
        ];
        let updates = rate_match(&participants, Some("two"), &RatingTable::new()).unwrap();

        assert!(after(&updates, "b").mu > INITIAL_MU, "the winner gains");
        assert!(after(&updates, "a").mu < INITIAL_MU, "the rest lose");
        assert_close(
            after(&updates, "a").mu,
            after(&updates, "c").mu,
            "sides tied for second must be treated identically",
        );
    }

    /// An unproven player (large `σ`) moves further on the same result than a
    /// settled one — the provisional-rating behaviour that makes early games
    /// converge quickly without wrecking established opponents' ratings.
    #[test]
    fn an_uncertain_player_moves_further_than_a_settled_one() {
        let unproven = rate_match(
            &singles(),
            Some("home"),
            &table(&[("a", rating(25.0, 8.0)), ("b", rating(25.0, 8.0))]),
        )
        .unwrap();
        let settled = rate_match(
            &singles(),
            Some("home"),
            &table(&[("a", rating(25.0, 1.0)), ("b", rating(25.0, 1.0))]),
        )
        .unwrap();

        assert!(
            after(&unproven, "a").mu - 25.0 > after(&settled, "a").mu - 25.0,
            "uncertainty should buy a bigger swing"
        );
    }

    /// Evidence only ever narrows the belief: no single match may widen a
    /// player's `σ`. (Inactivity does, but that is the pipeline's job, not
    /// the engine's.) A regression here would make ratings drift apart
    /// forever instead of converging.
    #[test]
    fn rating_a_match_never_widens_a_players_uncertainty() {
        let matches = history();
        let mut ratings = RatingTable::new();
        for historic in &matches {
            let updates =
                rate_match(&historic.participants, historic.winner_side_id, &ratings).unwrap();
            for update in &updates {
                assert!(
                    update.after.sigma <= update.before.sigma,
                    "{} widened: {} -> {}",
                    update.user_id,
                    update.before.sigma,
                    update.after.sigma
                );
            }
            apply(&updates, &mut ratings);
        }
    }

    /// The number on the match card. Derived from the same rounded display
    /// values shown either side of it, so "1500 → 1518, +18" always adds up.
    #[test]
    fn the_display_delta_reconciles_with_the_displayed_ratings() {
        let updates = rate_match(&singles(), Some("home"), &RatingTable::new()).unwrap();
        for update in &updates {
            let before = update.before.display().rating;
            let after = update.after.display().rating;
            assert_eq!(update.display_delta(), after - before);
        }
        let winner = updates.iter().find(|u| u.user_id == "a").unwrap();
        assert_eq!(winner.display_delta(), 79, "a first win off 1500");
    }

    // -----------------------------------------------------------------
    // Projection.
    // -----------------------------------------------------------------

    /// The projection is sport-agnostic: it knows about sides and user ids
    /// and nothing else, which is why one engine covers squash and football.
    /// It also resolves ratings, defaulting a player who has never been rated
    /// on this ladder.
    #[test]
    fn group_by_side_groups_participants_and_resolves_their_ratings() {
        let ratings = table(&[("h1", rating(30.0, 2.0))]);
        let sides = group_by_side(
            &[
                participant("a0", "away"),
                participant("h1", "home"),
                participant("h0", "home"),
            ],
            &ratings,
        );

        assert_eq!(sides.len(), 2);
        assert_eq!(sides[0].side_id, "away", "sides sort by id");
        assert_eq!(sides[1].side_id, "home");
        assert_eq!(
            sides[1]
                .players
                .iter()
                .map(|p| p.user_id.as_str())
                .collect::<Vec<_>>(),
            ["h0", "h1"],
            "players sort by id within a side"
        );
        assert_eq!(
            sides[1].players[0].rating,
            PlayerRating::default(),
            "a player with no rating yet starts at the default"
        );
        assert_eq!(sides[1].players[1].rating, rating(30.0, 2.0));
    }

    // -----------------------------------------------------------------
    // Rejections. Every one of these is something `skillratings` would
    // otherwise absorb without complaint.
    // -----------------------------------------------------------------

    /// A one-sided match has nothing to compare against. `weng_lin_multi_team`
    /// would hand the inputs straight back, so the match would look rated and
    /// have moved nothing.
    #[test]
    fn a_match_with_fewer_than_two_sides_is_rejected() {
        let single_side = vec![participant("a", "home"), participant("b", "home")];
        assert_eq!(
            rate_match(&single_side, Some("home"), &RatingTable::new()),
            Err(RatingError::NotEnoughSides(1))
        );
        assert_eq!(
            rate_match(&[], None, &RatingTable::new()),
            Err(RatingError::NotEnoughSides(0))
        );
    }

    /// An empty side makes `weng_lin_multi_team` return every team unchanged —
    /// silently, for *all* sides, not just the empty one. Rejecting keeps that
    /// from being mistaken for "this match didn't affect anyone's rating".
    #[test]
    fn an_empty_side_is_rejected_rather_than_silently_rating_nothing() {
        let sides = vec![
            RatingSide {
                side_id: "home".into(),
                players: vec![RatedPlayer {
                    user_id: "a".into(),
                    rating: PlayerRating::default(),
                }],
            },
            RatingSide {
                side_id: "away".into(),
                players: vec![],
            },
        ];
        assert_eq!(
            rate_sides(&sides, Some("home")),
            Err(RatingError::EmptySide("away".into()))
        );
    }

    /// One account on both sides would get two updates from one match, and
    /// whichever the caller wrote last would win — a coin flip baked into
    /// stored data.
    #[test]
    fn a_player_appearing_twice_is_rejected() {
        let both_sides = vec![
            participant("a", "home"),
            participant("a", "away"),
            participant("b", "away"),
        ];
        assert_eq!(
            rate_match(&both_sides, Some("home"), &RatingTable::new()),
            Err(RatingError::DuplicatePlayer("a".into()))
        );

        let twice_on_one_side = vec![
            participant("a", "home"),
            participant("a", "home"),
            participant("b", "away"),
        ];
        assert_eq!(
            rate_match(&twice_on_one_side, None, &RatingTable::new()),
            Err(RatingError::DuplicatePlayer("a".into()))
        );
    }

    /// A `winner_side_id` naming a side that isn't playing would leave every
    /// side on rank 1 — i.e. quietly score a decisive match as a draw. Almost
    /// certainly a stale id after a roster edit, and exactly the kind of thing
    /// repair exists to fix, so it has to be loud.
    #[test]
    fn an_unknown_winning_side_is_rejected_rather_than_scored_as_a_draw() {
        assert_eq!(
            rate_match(&singles(), Some("ghost"), &RatingTable::new()),
            Err(RatingError::UnknownWinnerSide("ghost".into()))
        );
    }
}
