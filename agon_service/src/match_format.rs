//! Sport-specific match format/rules — half length, overs limit, penalty
//! runs, and so on. Optional on a match: `None` means no format was
//! configured, and clients fall back to their own sensible per-sport
//! defaults rather than every match being required to specify one.
//!
//! Phase 1: mostly descriptive. Nothing here is enforced by the live-scoring
//! API yet — going over a configured overs limit doesn't block further
//! deliveries, and a no-ball's configured penalty isn't applied
//! automatically. Live-scoring clients use it to prefill sensible defaults
//! and show progress against the configured limit (e.g. "14.2/20 overs").
//! Actually enforcing it (free hits, auto-suggesting innings/half end,
//! extra-time and penalty-shootout flows) is intentionally out of scope for
//! now and would build on top of this. `wide_is_extra_ball` and
//! `no_ball_is_extra_ball` are the exceptions, alongside `balls_per_over`:
//! they drive whether a wide/no-ball advances the over in
//! `detailed_score::cricket::CricketInnings::from_deliveries`, since that's
//! server-side scoring math, not just client display.

use poem_openapi::{Object, Union};

#[derive(Union, Clone)]
#[oai(one_of, discriminator_name = "sport")]
pub enum MatchFormat {
    Football(FootballFormat),
    Cricket(CricketFormat),
    Netball(NetballFormat),
}

#[derive(Object, Clone)]
pub struct FootballFormat {
    /// Minutes per half, e.g. 45.
    pub half_length_minutes: u32,
    /// Number of halves — normally 2.
    pub num_halves: u32,
    /// Whether extra time is played if the match is level after normal time.
    pub extra_time: bool,
    /// Minutes per extra-time half, if `extra_time` is set.
    pub extra_time_half_length_minutes: Option<u32>,
    /// Whether a penalty shootout follows if still level.
    pub penalties: bool,
}

#[derive(Object, Clone)]
pub struct CricketFormat {
    /// Overs per innings; `None` = unlimited (e.g. a declaration format).
    pub overs_per_innings: Option<u32>,
    /// Innings per side — 1 (limited-overs) or 2 (first-class/test-style).
    pub innings_per_side: u32,
    /// Legal deliveries per over — 6 for almost everything, 5 for The
    /// Hundred. Unlike the rest of this struct, this one *is* load-bearing
    /// on the server: it drives the overs-bowled math in
    /// `detailed_score::cricket::CricketInnings::from_deliveries` (and so
    /// the live-scoring fold that builds on it), not just client display.
    pub balls_per_over: u32,
    /// Runs awarded for a no-ball's mandatory penalty (excludes any runs off
    /// the bat, which are recorded separately).
    pub no_ball_penalty_runs: u32,
    /// Runs awarded for a wide.
    pub wide_penalty_runs: u32,
    /// Whether a wide is re-bowled as an extra delivery (the standard rule —
    /// `true`) or simply counts as one of the over's legal balls alongside its
    /// penalty runs (a casual/social variant some sides play to keep overs
    /// moving).
    pub wide_is_extra_ball: bool,
    /// Whether a no-ball is re-bowled as an extra delivery (`true`, the
    /// standard rule) or counts as a legal ball, same trade-off as
    /// `wide_is_extra_ball`. Independent of `free_hit_after_no_ball`: Test
    /// cricket, for instance, plays no-balls as extra balls with no free hit
    /// at all (free hits are a modern limited-overs addition).
    pub no_ball_is_extra_ball: bool,
    /// Whether the delivery after a no-ball is a free hit.
    pub free_hit_after_no_ball: bool,
}

#[derive(Object, Clone)]
pub struct NetballFormat {
    /// Quarters per match — normally 4; some social leagues play 2 longer
    /// halves instead, so this isn't hardcoded to 4.
    pub num_quarters: u32,
    /// Minutes per quarter, e.g. 15 for a standard game or 12 for Fast5.
    pub quarter_length_minutes: u32,
    /// Whether a goal from the two-point zone counts double (Fast5/Netball
    /// Superleague Power Play rules). `false` for standard scoring, where
    /// every goal is worth one regardless of where it's shot from.
    pub two_point_zone: bool,
    /// Whether an extra period is played (typically "first to score" /
    /// golden goal) if the match is level after full time.
    pub extra_time: bool,
}
