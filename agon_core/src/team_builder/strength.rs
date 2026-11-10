//! Player strength estimates for the team builder.
//!
//! Every player in a pool becomes an [`Estimate`]: `μ` (best guess, on the
//! rating engine's native Weng-Lin scale, so it adds up into team strength
//! exactly the way the engine adds players up) and `σ` (how unsure we are).
//! Three sources feed it, trusted in this order:
//!
//! 1. **Ranked rating** on the sport's ladder. Trusted progressively: its
//!    weight is `min(ranked_games, RATING_TRUST_MATCHES) / RATING_TRUST_MATCHES`,
//!    so it carries the estimate alone from 10 ranked games on.
//! 2. **Win rate** across every game in the sport, ranked or friendly, for
//!    the rest of the weight. Shrunk toward 50% by `WR_PRIOR_GAMES` phantom
//!    wins and losses so a 2–0 record isn't read as elite, then mapped onto
//!    the μ scale around the pool's rated players. Its `σ` never drops below
//!    [`WR_SIGMA_FLOOR`]: in a team sport your teammates win games too, so a
//!    win rate can never be as sure as a settled rating.
//! 3. **Nothing** (a new account, or a guest with no account): the pool's
//!    average, with the engine's starting uncertainty. A stranger at a
//!    Thursday five-a-side is most likely near Thursday five-a-side level,
//!    not near the global default.
//!
//! An organiser can override any player for one draw ([`Level`]): Newer or
//! Stronger moves `μ` to this pool's lower or upper quartile, keeping `σ`,
//! so an overridden unknown is still spread out like any other unknown.
//!
//! A rating still in placement (`rating::published` false) feeds the
//! estimate like any other, but is tagged [`Signal::Placement`] so the API
//! layer can withhold the number. Nothing here decides what is shown.

use crate::rating::{BETA, INITIAL_MU, INITIAL_SIGMA, PLACEMENT_MATCHES};

/// Ranked games after which the rating alone carries the estimate.
pub const RATING_TRUST_MATCHES: u64 = 10;

/// Phantom wins *and* phantom losses added to every win rate before use.
pub const WR_PRIOR_GAMES: f64 = 3.0;

/// Native μ per unit of log-odds of the (shrunk) win rate. 2.5 μ is 75
/// display points: a 61% adjusted win rate reads as about +34 over the
/// pool's rated average, an 86% one as about +134. Deliberately damped
/// against what the same edge would mean in a rating, for the teammate
/// reason above.
pub const C_WR: f64 = 2.5;

/// Games at which a win-rate estimate's `σ` has shrunk to `1/√2` of
/// [`INITIAL_SIGMA`].
pub const WR_SIGMA_HALF_GAMES: f64 = 10.0;

/// The tightest a win-rate-only estimate ever gets.
pub const WR_SIGMA_FLOOR: f64 = 5.5;

/// Games of any kind below which a win-rate-only player still counts as
/// "little history".
pub const LITTLE_HISTORY_GAMES: u64 = 5;

/// A player's rating on the relevant ladder, as stored on their profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatingInput {
    pub mu: f64,
    pub sigma: f64,
    pub matches_rated: u64,
}

/// Lifetime record in the sport — `GenericSportStatsRecord`'s counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinRecord {
    pub played: u64,
    pub wins: u64,
    pub draws: u64,
    pub losses: u64,
}

/// Everything known about one pool member.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    /// `None` when they have no rating on this sport's ladder (or the sport
    /// has no ladder at all).
    pub rating: Option<RatingInput>,
    pub record: WinRecord,
    /// A roster entry with no account behind it. Has no history by
    /// definition, but is labelled differently from a new account.
    pub guest: bool,
    /// The organiser's override for this draw, if any.
    pub level: Option<Level>,
}

/// Organiser override, relative to this pool. "As expected" is no override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Newer,
    Stronger,
}

/// Where an estimate mostly came from — what the UI labels the player with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `RATING_TRUST_MATCHES`+ ranked games: rating only.
    Rated,
    /// Published rating (placed), but under `RATING_TRUST_MATCHES` ranked
    /// games: rating and win rate blended.
    Blended,
    /// Rated, but still in placement. The number must not be shown.
    Placement,
    /// Games played, none ranked: win rate only.
    WinRate,
    /// An account with no games in this sport.
    New,
    /// No account.
    Guest,
}

/// The win rate behind an estimate, for the "how was this estimated" sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WinRateInfo {
    /// `(wins + ½·draws) / played`.
    pub raw: f64,
    /// After shrinking toward 50%.
    pub adjusted: f64,
    pub played: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub mu: f64,
    pub sigma: f64,
    pub signal: Signal,
    /// 1 = little history, 2 = some, 3 = solid. Coarse on purpose: it is the
    /// three dots beside a name, not a statistic.
    pub certainty: u8,
    /// Share of `mu` that came from the rating (the rest is win rate, or the
    /// pool average for someone with nothing).
    pub rating_weight: f64,
    pub ranked_games: u64,
    pub win_rate: Option<WinRateInfo>,
    pub level: Option<Level>,
}

/// Shrunk win rate.
fn adjusted_win_rate(r: &WinRecord) -> f64 {
    let points = r.wins as f64 + 0.5 * r.draws as f64;
    (points + WR_PRIOR_GAMES) / (r.played as f64 + 2.0 * WR_PRIOR_GAMES)
}

fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}

fn win_rate_sigma(played: u64) -> f64 {
    (INITIAL_SIGMA * (WR_SIGMA_HALF_GAMES / (played as f64 + WR_SIGMA_HALF_GAMES)).sqrt())
        .max(WR_SIGMA_FLOOR)
}

/// Estimate every member of a pool. Pool-relative, so it has to see the
/// whole pool at once: win rates are centred on the pool's rated players,
/// and players with nothing (and every [`Level`] override) are placed
/// relative to the pool's other estimates. Output order matches input.
#[must_use]
pub fn estimate_pool(pool: &[History]) -> Vec<Estimate> {
    // Centre for win rates: the mean rating of anyone in the pool who has
    // one, so "61% across all games" means "a bit above the rated players
    // here". Falls back to the engine's origin when nobody is rated.
    let rated: Vec<f64> = pool.iter().filter_map(|h| h.rating.map(|r| r.mu)).collect();
    let anchor = if rated.is_empty() {
        INITIAL_MU
    } else {
        rated.iter().sum::<f64>() / rated.len() as f64
    };

    // Pass 1: everyone with any history.
    let mut out: Vec<Option<Estimate>> = pool
        .iter()
        .map(|h| {
            if h.guest {
                return None;
            }
            let ranked = h.rating.map_or(0, |r| r.matches_rated);
            if ranked == 0 && h.record.played == 0 {
                return None;
            }
            let win_rate = (h.record.played > 0).then(|| WinRateInfo {
                raw: (h.record.wins as f64 + 0.5 * h.record.draws as f64) / h.record.played as f64,
                adjusted: adjusted_win_rate(&h.record),
                played: h.record.played,
            });
            let (mu_w, sigma_w) = match win_rate {
                Some(w) => (anchor + C_WR * logit(w.adjusted), win_rate_sigma(w.played)),
                None => (anchor, INITIAL_SIGMA),
            };
            let (mu, sigma, weight) = match h.rating {
                Some(r) if ranked > 0 => {
                    let w = ranked.min(RATING_TRUST_MATCHES) as f64 / RATING_TRUST_MATCHES as f64;
                    let mu = w * r.mu + (1.0 - w) * mu_w;
                    let var = w * r.sigma.powi(2) + (1.0 - w) * sigma_w.powi(2);
                    (mu, var.sqrt(), w)
                }
                _ => (mu_w, sigma_w, 0.0),
            };
            let signal = if ranked >= RATING_TRUST_MATCHES {
                Signal::Rated
            } else if ranked >= PLACEMENT_MATCHES {
                Signal::Blended
            } else if ranked > 0 {
                Signal::Placement
            } else {
                Signal::WinRate
            };
            let certainty = match signal {
                Signal::Rated => 3,
                Signal::WinRate if h.record.played < LITTLE_HISTORY_GAMES => 1,
                _ => 2,
            };
            Some(Estimate {
                mu,
                sigma,
                signal,
                certainty,
                rating_weight: weight,
                ranked_games: ranked,
                win_rate,
                level: None,
            })
        })
        .collect();

    // The pool's level, from everyone pass 1 could estimate.
    let mut known: Vec<f64> = out.iter().flatten().map(|e| e.mu).collect();
    known.sort_by(f64::total_cmp);
    let group_mean = if known.is_empty() {
        INITIAL_MU
    } else {
        known.iter().sum::<f64>() / known.len() as f64
    };
    // Quartiles need a few points to mean anything; below that, one BETA
    // (the engine's "one skill class") either side of the mean.
    let (low, high) = if known.len() >= 4 {
        (quantile(&known, 0.25), quantile(&known, 0.75))
    } else {
        (group_mean - BETA, group_mean + BETA)
    };

    // Pass 2: everyone with nothing gets the pool average; then overrides.
    pool.iter()
        .zip(out.iter_mut())
        .map(|(h, est)| {
            let mut e = est.take().unwrap_or(Estimate {
                mu: group_mean,
                sigma: INITIAL_SIGMA,
                signal: if h.guest { Signal::Guest } else { Signal::New },
                certainty: 1,
                rating_weight: 0.0,
                ranked_games: 0,
                win_rate: None,
                level: None,
            });
            if let Some(level) = h.level {
                e.mu = match level {
                    Level::Newer => low,
                    Level::Stronger => high,
                };
                e.level = Some(level);
            }
            e
        })
        .collect()
}

/// Linear-interpolated quantile of an ascending, non-empty slice.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rated(mu: f64, sigma: f64, matches_rated: u64, record: (u64, u64)) -> History {
        History {
            rating: Some(RatingInput {
                mu,
                sigma,
                matches_rated,
            }),
            record: WinRecord {
                played: record.0,
                wins: record.1,
                draws: 0,
                losses: record.0 - record.1,
            },
            ..History::default()
        }
    }

    fn unrated(played: u64, wins: u64) -> History {
        History {
            record: WinRecord {
                played,
                wins,
                draws: 0,
                losses: played - wins,
            },
            ..History::default()
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn rating_weight_ramps_to_full_at_ten_ranked_games() {
        let weights: Vec<f64> = [1, 4, 5, 9, 10, 40]
            .iter()
            .map(|&n| estimate_pool(&[rated(30.0, 4.0, n, (n, n / 2))])[0].rating_weight)
            .collect();
        assert!(close(weights[0], 0.1));
        assert!(close(weights[1], 0.4));
        assert!(close(weights[3], 0.9));
        assert!(close(weights[4], 1.0));
        assert!(close(weights[5], 1.0));
    }

    #[test]
    fn ten_plus_ranked_games_is_the_rating_exactly() {
        let e = estimate_pool(&[rated(31.0, 3.0, 12, (30, 25))])[0];
        assert!(close(e.mu, 31.0));
        assert!(close(e.sigma, 3.0));
        assert_eq!(e.signal, Signal::Rated);
        assert_eq!(e.certainty, 3);
    }

    #[test]
    fn signals_follow_ranked_games() {
        let pool = [
            rated(25.0, 5.0, 3, (12, 8)),
            rated(25.0, 5.0, 7, (15, 9)),
            unrated(22, 14),
            unrated(3, 2),
            History::default(),
            History {
                guest: true,
                ..History::default()
            },
        ];
        let signals: Vec<Signal> = estimate_pool(&pool).iter().map(|e| e.signal).collect();
        assert_eq!(
            signals,
            vec![
                Signal::Placement,
                Signal::Blended,
                Signal::WinRate,
                Signal::WinRate,
                Signal::New,
                Signal::Guest
            ]
        );
        let certainty: Vec<u8> = estimate_pool(&pool).iter().map(|e| e.certainty).collect();
        assert_eq!(certainty, vec![2, 2, 2, 1, 1, 1]);
    }

    #[test]
    fn small_records_are_pulled_toward_fifty_percent() {
        let two_nil = estimate_pool(&[unrated(2, 2)])[0].win_rate.unwrap();
        assert!(close(two_nil.raw, 1.0));
        assert!(close(two_nil.adjusted, 5.0 / 8.0));
        // 8-4 from 12 → 11/18 ≈ 61%, as on the strength sheet.
        let sam = estimate_pool(&[unrated(12, 8)])[0].win_rate.unwrap();
        assert!((sam.adjusted - 0.611).abs() < 0.001);
    }

    #[test]
    fn better_win_rate_means_higher_estimate_but_never_tighter_than_the_floor() {
        let pool = [unrated(40, 30), unrated(40, 20), unrated(40, 10)];
        let e = estimate_pool(&pool);
        assert!(e[0].mu > e[1].mu && e[1].mu > e[2].mu);
        assert!(
            close(e[1].mu, INITIAL_MU),
            "a 50% record sits on the anchor"
        );
        assert!(e.iter().all(|e| e.sigma >= WR_SIGMA_FLOOR));
    }

    #[test]
    fn win_rates_centre_on_the_pools_rated_players() {
        let pool = [rated(30.0, 3.0, 20, (20, 10)), unrated(30, 15)];
        let e = estimate_pool(&pool);
        assert!(close(e[1].mu, 30.0));
    }

    #[test]
    fn players_with_nothing_get_the_pool_average_and_full_uncertainty() {
        let pool = [
            rated(28.0, 3.0, 20, (20, 10)),
            rated(22.0, 3.0, 20, (20, 10)),
            History::default(),
            History {
                guest: true,
                ..History::default()
            },
        ];
        let e = estimate_pool(&pool);
        assert!(close(e[2].mu, 25.0));
        assert!(close(e[3].mu, 25.0));
        assert!(close(e[2].sigma, INITIAL_SIGMA));
    }

    #[test]
    fn empty_history_pool_falls_back_to_the_engine_origin() {
        let e = estimate_pool(&[History::default(), History::default()]);
        assert!(e.iter().all(|e| close(e.mu, INITIAL_MU)));
    }

    #[test]
    fn levels_move_to_the_pools_quartiles_and_keep_sigma() {
        let mut pool: Vec<History> = [20.0, 24.0, 26.0, 30.0]
            .iter()
            .map(|&mu| rated(mu, 3.0, 20, (20, 10)))
            .collect();
        pool.push(History {
            level: Some(Level::Stronger),
            ..History::default()
        });
        pool.push(History {
            level: Some(Level::Newer),
            guest: true,
            ..History::default()
        });
        let e = estimate_pool(&pool);
        assert!(close(e[4].mu, 27.0), "upper quartile of 20/24/26/30");
        assert!(close(e[5].mu, 23.0), "lower quartile");
        assert!(close(e[4].sigma, INITIAL_SIGMA));
        assert_eq!(e[5].signal, Signal::Guest);
        assert_eq!(e[4].level, Some(Level::Stronger));
    }

    #[test]
    fn levels_in_a_tiny_pool_use_one_beta_either_side() {
        let pool = [
            rated(25.0, 3.0, 20, (20, 10)),
            History {
                level: Some(Level::Stronger),
                ..History::default()
            },
        ];
        let e = estimate_pool(&pool);
        assert!(close(e[1].mu, 25.0 + BETA));
    }
}
