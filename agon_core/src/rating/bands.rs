//! Bands — the named tier shown next to a rating (`1620 · Advanced`).
//!
//! Three structural decisions here matter more than the numbers, which are
//! set crudely on purpose and expected to be retuned once there are real
//! users.
//!
//! **Bands are never stored.** They are derived at read time from the rating.
//! Retuning is then a constant change plus a deploy: every profile re-bands
//! instantly, nothing is backfilled, no migration runs. The corollary that is
//! easy to get wrong is that the Meilisearch documents must index the rating
//! *number*, never the band string — level filters are numeric range queries
//! built from [`BandScale::range`] at query time. Indexing a band would make
//! every retune a full reindex, quietly reintroducing the migration this
//! avoids.
//!
//! **Thresholds are a data table, not an `if` chain**, and they live only
//! here. The UI reads bands from the API rather than reimplementing the
//! cutoffs, so a retune is a service deploy rather than a coordinated
//! service + UI release.
//!
//! **Thresholds are absolute, not percentiles.** Percentile bands self-tune
//! and are always well distributed, but your band would then change when
//! *other people* improve — being demoted while playing well is a bad enough
//! experience to rule it out on its own. They also need per-sport population
//! statistics that a small user base makes far too noisy to trust. Absolute
//! thresholds mean "Advanced" means the same thing in 2026 and 2028. Revisit
//! at scale, not now.
//!
//! One rejected alternative worth recording: banding on the conservative
//! floor (`rating − confidence`) so bands feel "earned". Rejected because the
//! number and the band are shown *together* — a player seeing `1620 ·
//! Intermediate` would rightly ask which of the two is lying. The band is
//! therefore a pure function of the same number on screen, and uncertainty is
//! handled instead by not banding at all until there is evidence (see
//! [`Placement`]).

use super::ladder::Ladder;

/// How many rated matches on a ladder before a band is assigned. See
/// [`Placement::Unrated`].
pub const PLACEMENT_MATCHES: u32 = 5;

/// A skill tier. Ordered ascending, so "within one band of me" is a
/// comparison rather than a lookup.
///
/// Six because five is too coarse to feel like progression and eight leaves
/// bands unpopulated at this scale. Names are descriptive rather than
/// gamified (Bronze/Silver/Gold) to suit the semi-professional positioning —
/// a pure string swap if that reads wrong, since nothing persists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    Beginner,
    Improver,
    Intermediate,
    Advanced,
    Expert,
    Elite,
}

impl Band {
    /// Every band, ascending.
    pub const ALL: [Band; 6] = [
        Band::Beginner,
        Band::Improver,
        Band::Intermediate,
        Band::Advanced,
        Band::Expert,
        Band::Elite,
    ];

    /// The displayed name. Here rather than in the API layer so the string
    /// lives with the thresholds it describes — renaming a band is then one
    /// edit, not one per surface.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Band::Beginner => "Beginner",
            Band::Improver => "Improver",
            Band::Intermediate => "Intermediate",
            Band::Advanced => "Advanced",
            Band::Expert => "Expert",
            Band::Elite => "Elite",
        }
    }
}

/// One row of a band table: a band and the lowest display rating that earns
/// it. `min_rating: None` means open below — only ever the first row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandStep {
    pub band: Band,
    /// Inclusive lower bound, or `None` for the open-ended bottom band.
    pub min_rating: Option<i32>,
}

/// An ordered band table. Ascending by `min_rating`, first row open below,
/// last row open above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandScale {
    steps: &'static [BandStep],
}

/// The band table every ladder currently uses.
///
/// Six bands of 150 display points, with **1500 centred in the middle band**
/// — everyone starts at 1500, so it must not sit on a boundary where rating
/// noise would flip people back and forth between two tiers.
///
/// The width is chosen, not arbitrary. `β` maps to ~125 display points (a 67%
/// win rate — see `scale`), so a 150-point step means one band up is a ~70%
/// win rate for the stronger player (hard but genuinely winnable) and two
/// bands up is ~85%+ (a mismatch). That is what makes band distance usable
/// for matchmaking — "games within one band of you" — rather than decoration.
///
/// The ends are open on purpose: there is no floor or ceiling on skill.
pub const DEFAULT_BANDS: BandScale = BandScale {
    steps: &[
        BandStep {
            band: Band::Beginner,
            min_rating: None,
        },
        BandStep {
            band: Band::Improver,
            min_rating: Some(1275),
        },
        BandStep {
            band: Band::Intermediate,
            min_rating: Some(1425),
        },
        BandStep {
            band: Band::Advanced,
            min_rating: Some(1575),
        },
        BandStep {
            band: Band::Expert,
            min_rating: Some(1725),
        },
        BandStep {
            band: Band::Elite,
            min_rating: Some(1875),
        },
    ],
};

/// The band table for a ladder.
///
/// Every ladder shares [`DEFAULT_BANDS`] today — they all start at 1500 with
/// the same `β`, so the scales are structurally identical and per-sport
/// numbers would be invented rather than measured. Sports will develop
/// different spreads in practice, which is why this is a lookup taking a
/// ladder rather than a bare constant: tuning squash separately later becomes
/// a data change here, not a refactor of every call site.
#[must_use]
pub fn bands_for(_ladder: &Ladder) -> &'static BandScale {
    &DEFAULT_BANDS
}

/// Whether a player has enough history on a ladder to be given a band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Fewer than [`PLACEMENT_MATCHES`] rated matches on this ladder, so no
    /// band. A player with two games has a ±500 confidence interval; naming
    /// a tier off that is fake precision. The number itself can still be
    /// shown, with its `±` — it's the *tier* that would overclaim.
    ///
    /// Carries the countdown so the UI can render "3 more games to get
    /// placed" without reimplementing the constant.
    Unrated { matches_remaining: u32 },
    /// Placed, with the band earned by the displayed rating.
    Placed(Band),
}

impl BandScale {
    /// The rows of this table, ascending. Exposed so the search layer can
    /// build numeric range filters from the same data the band names come
    /// from.
    #[must_use]
    pub fn steps(&self) -> &'static [BandStep] {
        self.steps
    }

    /// The band for a **displayed** rating (the rounded integer on screen —
    /// see `scale::DisplayRating`), ignoring how much evidence is behind it.
    /// Use [`BandScale::placement`] unless you specifically want the band a
    /// number would earn.
    ///
    /// Banding the displayed integer rather than raw `μ` is deliberate:
    /// 1424.6 shows as 1425, and a player reading `1425 · Improver` when the
    /// table says 1425 is Intermediate would be looking at a bug.
    #[must_use]
    pub fn band(&self, rating: i32) -> Band {
        self.steps
            .iter()
            .rev()
            .find(|step| step.min_rating.is_none_or(|min| rating >= min))
            // Unreachable while the first row is open below, which
            // `tests::the_bottom_band_is_open_below` holds every table to. A
            // fallback rather than a panic: a malformed table should not be
            // able to take a profile read down.
            .map_or(Band::Beginner, |step| step.band)
    }

    /// The inclusive display-rating range for a band — `(lower, upper)`, with
    /// `None` meaning open at that end. `None` overall if this table doesn't
    /// contain the band.
    ///
    /// Returning an `Option` rather than a wide-open `(None, None)` on a
    /// miss is the safe direction for its main caller: a search filter built
    /// from an unbounded range would match every player rather than none.
    #[must_use]
    pub fn range(&self, band: Band) -> Option<(Option<i32>, Option<i32>)> {
        let index = self.steps.iter().position(|step| step.band == band)?;
        let lower = self.steps[index].min_rating;
        let upper = self
            .steps
            .get(index + 1)
            .and_then(|next| next.min_rating)
            .map(|next_min| next_min - 1);
        Some((lower, upper))
    }

    /// The band to show, given a displayed rating and how many rated matches
    /// on this ladder are behind it.
    ///
    /// The gate is games played rather than a `σ` ceiling, deliberately: it
    /// is the legible, actionable number ("3 more games"), and any σ
    /// threshold picked today would be a guess dressed up as precision. A σ
    /// ceiling is the principled refinement once there is real data to
    /// calibrate against.
    #[must_use]
    pub fn placement(&self, rating: i32, matches_rated: u32) -> Placement {
        if matches_rated < PLACEMENT_MATCHES {
            Placement::Unrated {
                matches_remaining: PLACEMENT_MATCHES - matches_rated,
            }
        } else {
            Placement::Placed(self.band(rating))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::ladder::{Sport, ladder_for};

    fn squash() -> Ladder {
        ladder_for(Sport::Squash).expect("squash is a rated sport")
    }

    /// The property the whole table is arranged around: everyone starts at
    /// 1500, so 1500 must sit at the **centre** of its band, as far from
    /// either boundary as possible. If it sat on one, ordinary rating noise
    /// after a single match would flip a new player between two tiers.
    ///
    /// Stated on the half-open interval `[1425, 1575)` the band really
    /// occupies — 1500 − 1425 == 1575 − 1500 — rather than on the inclusive
    /// display range `1425–1574`, whose integer midpoint is 1499.5.
    #[test]
    fn fifteen_hundred_is_centred_in_the_middle_band() {
        let scale = bands_for(&squash());
        assert_eq!(scale.band(1500), Band::Intermediate);

        let (lower, upper) = scale.range(Band::Intermediate).expect("in the table");
        let lower = lower.expect("the middle band is bounded below");
        let upper_exclusive = upper.expect("the middle band is bounded above") + 1;
        assert_eq!(1500 - lower, upper_exclusive - 1500, "1500 must be centred");
    }

    /// Every boundary, checked from both sides. Off-by-ones here are the
    /// classic failure of a threshold table, and they're invisible in
    /// production: a player one point below a cutoff simply sees the wrong
    /// tier.
    #[test]
    fn band_boundaries_are_inclusive_at_the_lower_edge() {
        let scale = bands_for(&squash());
        for (below, at, band) in [
            (1274, 1275, Band::Improver),
            (1424, 1425, Band::Intermediate),
            (1574, 1575, Band::Advanced),
            (1724, 1725, Band::Expert),
            (1874, 1875, Band::Elite),
        ] {
            assert_eq!(scale.band(at), band, "{at} should be {band:?}");
            assert!(
                scale.band(below) < band,
                "{below} should be below {band:?}, got {:?}",
                scale.band(below)
            );
        }
    }

    /// Skill has no floor or ceiling, so the outer bands must absorb
    /// everything — including the 750 floor a brand-new account's
    /// conservative rating sits at, and a hypothetical runaway.
    #[test]
    fn the_outer_bands_are_open_ended() {
        let scale = bands_for(&squash());
        assert_eq!(scale.band(750), Band::Beginner);
        assert_eq!(scale.band(0), Band::Beginner);
        assert_eq!(scale.band(-500), Band::Beginner);
        assert_eq!(scale.band(9_000), Band::Elite);

        assert_eq!(scale.range(Band::Beginner), Some((None, Some(1274))));
        assert_eq!(scale.range(Band::Elite), Some((Some(1875), None)));
    }

    /// `BandScale::band`'s no-panic fallback is only correct while the first
    /// row is genuinely open below — this is the invariant that lets it
    /// return a band for any `i32` at all.
    #[test]
    fn the_bottom_band_is_open_below() {
        let steps = DEFAULT_BANDS.steps();
        assert_eq!(steps[0].min_rating, None);
        assert!(
            steps[1..].iter().all(|step| step.min_rating.is_some()),
            "only the bottom row may be open below"
        );
    }

    /// The table is scanned from the top down, so it is only correct if it is
    /// sorted; and the band ordering has to agree with the rating ordering or
    /// "within one band of me" would compare the wrong way round.
    #[test]
    fn the_table_is_ascending_in_both_rating_and_band() {
        let steps = DEFAULT_BANDS.steps();
        for pair in steps.windows(2) {
            let (lower, higher) = (pair[0], pair[1]);
            assert!(
                lower.band < higher.band,
                "{:?} must sort below {:?}",
                lower.band,
                higher.band
            );
            if let (Some(a), Some(b)) = (lower.min_rating, higher.min_rating) {
                assert!(a < b, "thresholds must ascend: {a} then {b}");
            }
        }
        assert_eq!(steps.len(), Band::ALL.len(), "every band appears once");
    }

    /// The 150-point step is what gives band distance a meaning in win
    /// probability (one band up ≈ 70%), so it isn't free to drift — a retune
    /// that changes a width should be a conscious edit that fails this test
    /// first.
    #[test]
    fn every_bounded_band_is_one_hundred_and_fifty_points_wide() {
        for band in Band::ALL {
            let Some((Some(lower), Some(upper))) = DEFAULT_BANDS.range(band) else {
                continue; // the two open-ended ends
            };
            assert_eq!(upper - lower + 1, 150, "{band:?} is the wrong width");
        }
    }

    /// The bands are contiguous — no rating falls in a gap between two rows,
    /// which a `range`-derived search filter would silently exclude.
    #[test]
    fn the_bands_tile_the_whole_range_without_gaps() {
        let scale = &DEFAULT_BANDS;
        for rating in 1_200..1_950 {
            let band = scale.band(rating);
            let (lower, upper) = scale.range(band).expect("in the table");
            assert!(lower.is_none_or(|l| rating >= l), "{rating} below {band:?}");
            assert!(upper.is_none_or(|u| rating <= u), "{rating} above {band:?}");
        }
    }

    /// Until there are five rated matches on the ladder there is no band at
    /// all — a two-game player has a ±500 interval, and naming a tier off
    /// that is fake precision the UI would present as fact.
    #[test]
    fn a_player_is_unrated_until_five_rated_matches() {
        let scale = bands_for(&squash());
        for played in 0..PLACEMENT_MATCHES {
            assert_eq!(
                scale.placement(1500, played),
                Placement::Unrated {
                    matches_remaining: PLACEMENT_MATCHES - played
                },
                "{played} matches should still be unplaced"
            );
        }
        assert_eq!(
            scale.placement(1500, PLACEMENT_MATCHES),
            Placement::Placed(Band::Intermediate)
        );
        assert_eq!(
            scale.placement(1900, 40),
            Placement::Placed(Band::Elite),
            "a long history bands on the rating as normal"
        );
    }

    /// The gate is about evidence, not about the rating being extreme: an
    /// Elite-looking number from three games is still unplaced. This is the
    /// sandbagging-adjacent case where showing a tier would be most
    /// misleading.
    #[test]
    fn a_high_rating_with_no_history_is_still_unplaced() {
        assert_eq!(
            bands_for(&squash()).placement(1950, 3),
            Placement::Unrated {
                matches_remaining: 2
            }
        );
    }

    /// Per-ladder tuning is a future data change, so every ladder must go
    /// through `bands_for` and get the same answer today — if a call site
    /// hardcoded `DEFAULT_BANDS` instead, the later split would silently miss
    /// it.
    #[test]
    fn every_ladder_shares_the_default_table_for_now() {
        for sport in Sport::ALL {
            let Some(ladder) = ladder_for(sport) else {
                continue;
            };
            assert_eq!(*bands_for(&ladder), DEFAULT_BANDS, "{sport:?}");
        }
    }
}
