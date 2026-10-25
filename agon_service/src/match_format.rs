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
//! `crate::sports::cricket`'s event fold, since that's server-side scoring
//! math, not just client display.
//!
//! Each sport's own `XFormat` struct lives in `crate::sports::x` alongside
//! the rest of that sport's surface — this file only holds the generic
//! `MatchFormat` union `agon_sports!` assembles from them.

use poem_openapi::Union;

/// x-macro template for `agon_sports!` (see `crate::sports`'s doc comment):
/// builds `MatchFormat` from the shared sport list.
macro_rules! define_match_format {
    ($( $variant:ident {
        tag: $tag:literal,
        module: $module:ident,
        score: $score:ty,
        format: $format:ty,
        live_event: $live_event:ty,
        stats: $stats:ty $(,)?
    } ),+ $(,)?) => {
        #[derive(Union, Clone)]
        #[oai(one_of, discriminator_name = "sport")]
        pub enum MatchFormat {
            $( $variant($format), )+
        }
    };
}
crate::agon_sports!(define_match_format);
