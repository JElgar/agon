//! Ladders — which pool a result counts towards, and the single place a
//! ladder key is minted.
//!
//! One rating per account *per sport*, not one overall. Elo-family ratings
//! only mean anything within a connected component of the who-played-whom
//! graph: my number is comparable to yours because there's a chain of results
//! linking us. Someone who only plays cricket and someone who only plays
//! tennis share no such chain, so a single blended number would look directly
//! comparable while being nothing of the sort — worse than having no number,
//! because people act on it.

use std::fmt;

/// A sport, as the rating engine sees it.
///
/// A hand-kept mirror of `agon_service::MatchType`, the same arrangement (and
/// for the same reason) as `LiveEventPayloadRecord` and `MatchFormatRecord`
/// in `dao::records`: `agon_core` cannot depend on `agon_service`, and an
/// opaque `&str` would let a new sport slip silently into the wrong ladder.
/// **New sport = new variant on both sides**, plus an arm in
/// [`ladder_for`] deciding which pool it rates into.
///
/// The one gap that mirroring leaves — and it is worth being explicit about
/// it rather than implying a guarantee that doesn't exist — is that adding a
/// variant to `MatchType` alone will not fail to compile here. It fails in
/// `mapping::match_type_tag`, whose exhaustive match forces a tag string,
/// and that new tag then lands in [`Sport::Other`] below, which is
/// *unrated*. So the failure mode of forgetting this file is "the sport
/// quietly doesn't get a ladder", never "results land in the wrong pool".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sport {
    Tennis,
    Badminton,
    Squash,
    TableTennis,
    Football,
    Cricket,
    Netball,
    /// Fallback for sports not modelled explicitly — the tag `MatchType::Other`
    /// stores, and anything unrecognised. Deliberately **not rated**: see
    /// [`ladder_for`].
    Other,
}

impl Sport {
    /// Every sport, for callers that need to enumerate ladders (and for the
    /// tests that assert properties across all of them).
    pub const ALL: [Sport; 8] = [
        Sport::Tennis,
        Sport::Badminton,
        Sport::Squash,
        Sport::TableTennis,
        Sport::Football,
        Sport::Cricket,
        Sport::Netball,
        Sport::Other,
    ];

    /// Parse a stored `MatchRecord::match_type` tag.
    ///
    /// Unknown tags become [`Sport::Other`] rather than an error, matching
    /// `mapping::match_type_from_tag` — a sport this build doesn't know about
    /// must not fail a read, and (unlike on the API side) here it can't do any
    /// damage either, since `Other` doesn't rate.
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        match tag {
            "tennis" => Sport::Tennis,
            "badminton" => Sport::Badminton,
            "squash" => Sport::Squash,
            "table_tennis" => Sport::TableTennis,
            "football" => Sport::Football,
            "cricket" => Sport::Cricket,
            "netball" => Sport::Netball,
            _ => Sport::Other,
        }
    }

    /// The stored tag for this sport. The exact strings
    /// `mapping::match_type_tag` writes onto `MatchRecord::match_type`.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Sport::Tennis => "tennis",
            Sport::Badminton => "badminton",
            Sport::Squash => "squash",
            Sport::TableTennis => "table_tennis",
            Sport::Football => "football",
            Sport::Cricket => "cricket",
            Sport::Netball => "netball",
            Sport::Other => "other",
        }
    }
}

/// The key a rating is stored, compared and matchmade under — `"squash"`
/// today.
///
/// A newtype over a string rather than the closed `Sport` enum, because the
/// set of ladders is deliberately *open* where the set of sports is closed.
/// Within a sport some formats differ more than some sports differ from each
/// other — tennis singles vs doubles is a bigger skill gap than tennis vs
/// badminton — so a later split into `"tennis:doubles"` is likely. Keeping
/// the stored key a string makes that split **additive** (new ladders simply
/// start fresh) instead of a migration of every stored rating, history item
/// and search document.
///
/// Two invariants the newtype exists to hold:
///
/// - It is only ever minted by [`ladder_for`], so the "what pool does this
///   match count towards" decision lives in exactly one match arm.
/// - The value never contains `dao::keys::DELIMITER` (`#`), because a ladder
///   is a segment of the phase-2 sort key `RATING#<ladder>#<starts_at>#<mid>`
///   and a `#` inside it would break that key's round-trip. Hence `:` as the
///   sub-ladder separator, not `#`. Guarded by
///   `tests::ladder_keys_never_contain_the_key_delimiter`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ladder(String);

impl Ladder {
    /// The stored key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ladder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The ladder a result in `sport` counts towards, or `None` if it doesn't
/// count at all.
///
/// Phase 1 keys ladders on **sport alone**. Splitting singles from doubles
/// now would multiply sparsity before there is any data to justify it, and
/// the split stays cheap to make later (see [`Ladder`]) — the side player
/// counts it needs are already on the match. When that day comes this
/// function grows an argument; the call sites don't change shape.
///
/// [`Sport::Other`] is unrated, and that is a judgement worth stating: it is
/// not one sport, it's a bucket holding every sport we haven't modelled.
/// Rating it would pool a hockey result against a chess result under a name
/// implying they're comparable — precisely the disjoint-graph mistake that
/// argues for per-sport ladders in the first place, just committed inside a
/// single ladder instead of across them.
#[must_use]
pub fn ladder_for(sport: Sport) -> Option<Ladder> {
    match sport {
        Sport::Tennis
        | Sport::Badminton
        | Sport::Squash
        | Sport::TableTennis
        | Sport::Football
        | Sport::Cricket
        | Sport::Netball => Some(Ladder(sport.tag().to_string())),
        Sport::Other => None,
    }
}

/// [`ladder_for`], from a stored `MatchRecord::match_type` tag — the form
/// every caller in the DAO and worker actually holds. Exists so those callers
/// never have a reason to build a ladder string themselves.
#[must_use]
pub fn ladder_for_tag(tag: &str) -> Option<Ladder> {
    ladder_for(Sport::from_tag(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::keys::DELIMITER;

    /// A ladder is a segment of the phase-2 `RATING#<ladder>#<starts_at>#<mid>`
    /// sort key, which is parsed back by splitting on `#`. A ladder containing
    /// a `#` would silently produce an unparseable key — so the sub-ladder
    /// separator is `:` and this test holds every current and future ladder to
    /// that.
    #[test]
    fn ladder_keys_never_contain_the_key_delimiter() {
        for sport in Sport::ALL {
            if let Some(ladder) = ladder_for(sport) {
                assert!(
                    !ladder.as_str().contains(DELIMITER),
                    "ladder {ladder} contains the key delimiter"
                );
            }
        }
    }

    /// Every modelled sport rates; the catch-all does not. The `None` is the
    /// point — an unmodelled sport must not quietly share a pool with every
    /// other unmodelled sport.
    #[test]
    fn every_real_sport_has_a_ladder_and_other_has_none() {
        for sport in Sport::ALL {
            let ladder = ladder_for(sport);
            if sport == Sport::Other {
                assert_eq!(ladder, None, "`Other` must not be rated");
            } else {
                assert_eq!(
                    ladder.map(|l| l.as_str().to_string()),
                    Some(sport.tag().to_string())
                );
            }
        }
    }

    /// Ladders are minted from the *stored* tag, so the tag round-trip has to
    /// hold or a match would rate into a ladder its own record can't name.
    #[test]
    fn sport_tags_round_trip() {
        for sport in Sport::ALL {
            assert_eq!(Sport::from_tag(sport.tag()), sport, "{sport:?}");
        }
    }

    /// An unrecognised tag — a sport added by a newer build, or corrupt data —
    /// degrades to unrated rather than panicking or inventing a ladder.
    #[test]
    fn an_unknown_tag_is_unrated_rather_than_a_new_ladder() {
        assert_eq!(Sport::from_tag("kabaddi"), Sport::Other);
        assert_eq!(ladder_for_tag("kabaddi"), None);
        assert_eq!(ladder_for_tag(""), None);
    }

    /// Distinct sports never collide onto one ladder — the whole per-sport
    /// split would be silently undone if two tags mapped to the same key.
    #[test]
    fn ladders_are_distinct_per_sport() {
        let mut seen = std::collections::HashSet::new();
        for sport in Sport::ALL {
            if let Some(ladder) = ladder_for(sport) {
                assert!(seen.insert(ladder.clone()), "duplicate ladder {ladder}");
            }
        }
        assert_eq!(
            seen.len(),
            Sport::ALL.len() - 1,
            "one per sport, minus `Other`"
        );
    }
}
