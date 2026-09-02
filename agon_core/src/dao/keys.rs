//! Typed partition (PK) and sort (SK) keys for the single-table design.
//!
//! Every item in the `agon` table is addressed by a `Pk` + `Sk`. These enums are
//! the one place that knows the on-the-wire key strings: everything else builds
//! keys via the enum variants and never hand-writes `"USER#..."`. Each enum
//! round-trips: `Display` formats to the stored string and `FromStr` parses it
//! back.
//!
//! Key grammar: segments are joined by `#`. Marker keys (no value) are written
//! `#MARKER` (e.g. `#PROFILE`). Prefixed keys are `PREFIX#<value>` and compound
//! keys `PREFIX#<a>#<b>`. `#` is a safe delimiter because our values — base64url
//! ids (`-`/`_`, no `#`) and ISO-8601 timestamps (`:`/`-`/`.`/`Z`, no `#`) — never
//! contain it.
//!
//! NOTE (deviation from docs/dynamodb-design.md §3): the feed item SK is given a
//! constant `FEED#` prefix here (`FEED#<starts_at>#<mid>`) rather than the
//! prefix-less `<starts_at>#<mid>` in the doc. A constant prefix does not change
//! sort order within the `UFEED#<uid>` partition (all items share it, so they
//! still order by `starts_at`), but it makes the key round-trippable like every
//! other key. Range queries use `FEED#<from>` .. `FEED#<to>`.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// The character separating key segments.
pub const DELIMITER: char = '#';

/// Closes a rating-history range query at the top of one ladder's keyspace —
/// see [`Sk::rating_history_end`].
///
/// `~` (0x7E) is the highest printable ASCII character, so it sorts above
/// every character that can appear in an ISO-8601 timestamp (digits, `-`,
/// `:`, `.`, `T`, `Z` — top out at `Z`, 0x5A) and above every character in a
/// base64url id (top out at `z`, 0x7A). DynamoDB orders strings by their
/// UTF-8 bytes, so that ordering is the one the query actually uses.
///
/// Deliberately not `char::MAX`: U+10FFFF would sort above non-ASCII values
/// too, but it is a Unicode noncharacter, and pinning a correctness argument
/// on a value that some intermediary might normalise away is not worth the
/// generality when every value in these keys is ASCII by construction.
pub const RATING_RANGE_SENTINEL: char = '~';

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("key is empty")]
    Empty,
    #[error("key `{0}` is malformed")]
    Malformed(String),
    #[error("unknown key prefix `{0}`")]
    UnknownPrefix(String),
}

// ---------------------------------------------------------------------------
// Partition key
// ---------------------------------------------------------------------------

/// Partition key. Identifies which item collection an item belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pk {
    /// A user and everything hanging off them (profile, stats, followers,
    /// notifications). `USER#<uid>`
    User(String),
    /// Email uniqueness guard. `EMAIL#<lowercased-email>`
    EmailGuard(String),
    /// Auth-identity mapping: the IdP `sub` claim → our internal user id.
    /// `AUTH#<sub>`. Decouples the user's stable internal id from the auth
    /// provider's subject so the provider can change without rewriting every
    /// `USER#`/`UFEED#`/`FOLLOWER#` key (only these guards get rewritten).
    AuthGuard(String),
    /// A team and its members/followers. `TEAM#<tid>`
    Team(String),
    /// A match and its sides/players/score/likes/top-level comments. `MATCH#<mid>`
    Match(String),
    /// A user's fan-out feed. `UFEED#<viewerUid>`
    UserFeed(String),
    /// An invitation. `INVITATION#<invId>`
    Invitation(String),
    /// An uploadable asset. `ASSET#<assetId>`
    Asset(String),
}

impl Pk {
    /// The static prefix keyword for this variant, without the delimiter —
    /// what `Display` builds the real key from. Every `Pk` query in this DAO
    /// is an exact-match partition key (`#pk = :pk`), never a `begins_with`
    /// range scan, so unlike `Sk::prefix()` this has no query-collision
    /// concern to guard against — kept private purely because nothing
    /// outside `Display` needs it.
    fn prefix(&self) -> &'static str {
        match self {
            Pk::User(_) => "USER",
            Pk::EmailGuard(_) => "EMAIL",
            Pk::AuthGuard(_) => "AUTH",
            Pk::Team(_) => "TEAM",
            Pk::Match(_) => "MATCH",
            Pk::UserFeed(_) => "UFEED",
            Pk::Invitation(_) => "INVITATION",
            Pk::Asset(_) => "ASSET",
        }
    }

    /// Build an email guard PK, normalizing the email to lowercase (the guard is
    /// stored lowercased so uniqueness is case-insensitive).
    pub fn email_guard(email: &str) -> Self {
        Pk::EmailGuard(email.to_lowercase())
    }
}

impl fmt::Display for Pk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Pk::User(v)
            | Pk::EmailGuard(v)
            | Pk::AuthGuard(v)
            | Pk::Team(v)
            | Pk::Match(v)
            | Pk::UserFeed(v)
            | Pk::Invitation(v)
            | Pk::Asset(v) => v,
        };
        write!(f, "{}{}{}", self.prefix(), DELIMITER, value)
    }
}

impl FromStr for Pk {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(KeyError::Empty);
        }
        let (prefix, value) = s
            .split_once(DELIMITER)
            .ok_or_else(|| KeyError::Malformed(s.into()))?;
        match prefix {
            "USER" => Ok(Pk::User(value.into())),
            "EMAIL" => Ok(Pk::EmailGuard(value.into())),
            "AUTH" => Ok(Pk::AuthGuard(value.into())),
            "TEAM" => Ok(Pk::Team(value.into())),
            "MATCH" => Ok(Pk::Match(value.into())),
            "UFEED" => Ok(Pk::UserFeed(value.into())),
            "INVITATION" => Ok(Pk::Invitation(value.into())),
            "ASSET" => Ok(Pk::Asset(value.into())),
            other => Err(KeyError::UnknownPrefix(other.into())),
        }
    }
}

// ---------------------------------------------------------------------------
// Sort key
// ---------------------------------------------------------------------------

/// Sort key. Distinguishes items within a partition and orders item collections.
/// Overloaded across entities — the same SK variant is reused wherever the shape
/// fits (e.g. `Follower` under both `USER#` and `TEAM#`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sk {
    /// User profile item. `#PROFILE`
    Profile,
    /// Singleton meta item for a team/match/invitation/asset. `#META`
    Meta,
    /// Uniqueness guard marker (e.g. under an email guard PK). `#GUARD`
    Guard,

    /// A follower edge (who follows this user/team). `FOLLOWER#<followerUid>`
    Follower(String),
    /// A team membership. `MEMBER#<membershipId>`
    Member(String),
    /// A match side. `SIDE#<sideId>`
    Side(String),
    /// A match player. `PLAYER#<playerId>`
    Player(String),
    /// A match's live-scoring score record, keyed by sport — the score as
    /// derived from the event log, live or finished, but never itself the
    /// agreed result. Named `LIVESCORE#<sport>` rather than `SCORE#` to keep
    /// that distinct from `confirmed_score`/`pending_score` on the match's
    /// `#META` item, which *is* the agreed-or-awaiting-agreement result (see
    /// `ScoreSubmission`'s confirm/dispute flow).
    Score(String),
    /// A like on a match. `LIKE#<uid>`
    Like(String),

    /// A live-scoring event, in append order — the source of truth for live
    /// scoring. `LIVEEVT#<10-digit zero-padded seq>`; zero-padding keeps
    /// lexicographic order equal to numeric order. Never mutated once written;
    /// corrections are later events (a `void` payload referencing an earlier
    /// seq), not edits.
    LiveEvent(u32),

    /// A score submission. `SCORESUB#<subId>` — addressed by id; time ordering
    /// is via GSI1 (`MSUBMISSIONS#<matchId>` / `<ts>#<subId>`).
    ScoreSubmission(String),
    /// A top-level comment on a match. `COMMENT#<cid>` — addressed by id; time
    /// ordering is via GSI1 (`MCOMMENTS#<matchId>` / `<ts>#<cid>`).
    Comment(String),
    /// A reply to a top-level comment, in the match partition. `REPLY#<rid>` —
    /// addressed by id; per-parent time ordering is via GSI1
    /// (`CREPLIES#<parentId>` / `<ts>#<rid>`).
    Reply(String),
    /// A notification. `NOTIF#<nid>` — addressed by id; time ordering is via
    /// GSI1 (`UNOTIFS#<uid>` / `<ts>#<nid>`).
    Notification(String),
    /// A registered push destination, in the user partition. `DEVICE#<token>`
    /// — the FCM registration token is itself the key value, so re-registering
    /// the same token is a natural upsert (no separate id layer needed).
    Device(String),
    /// Records what a match contributed to one participant's per-sport stats
    /// (`{ played, won }`), in the match's partition. `STATCONTRIB#<userId>`.
    /// The async stats reconciler diffs the desired contribution (from the
    /// match's current state) against this stored one and applies the delta to
    /// the user's profile item (`stats.<sport>`) in the same transaction — so
    /// re-scores, roster changes and cancellations all self-correct, and
    /// redelivery is a no-op (same state → zero delta).
    StatContribution(String),
    /// A fan-out feed entry, ordered by match start time. `FEED#<starts_at>#<mid>`
    /// (only ever listed, never addressed by id — keeps ts in the key).
    Feed { starts_at: String, match_id: String },

    /// Records what a match currently contributes to one participant's
    /// rating, in the match's partition. `RATINGCONTRIB#<participantId>` —
    /// the direct analogue of [`Sk::StatContribution`], and what makes
    /// re-rating idempotent under at-least-once redelivery: the handler
    /// diffs the contribution the match's current state implies against this
    /// stored one, and a redelivery of an unchanged match finds them equal
    /// and writes nothing.
    ///
    /// One deliberate deviation from `STATCONTRIB#<userId>` worth flagging,
    /// because it looks like an inconsistency: the id here is a user id *or*
    /// a team id, since teams carry ratings of their own (see
    /// `TeamRecord::ratings`). Which one it is lives in the record's
    /// `owner_kind`, not the key — keeping it out of the key is what lets
    /// "every participant's contribution for this match" stay a single
    /// `begins_with(SK, RATINGCONTRIB#)` query instead of two.
    RatingContribution(String),

    /// One match's effect on one participant's rating, in that participant's
    /// own partition (`USER#<uid>` or `TEAM#<tid>`).
    /// `RATING#<ladder>#<played_at>#<matchId>`.
    ///
    /// `played_at` is the match's `starts_at`, and the name is the point:
    /// matches are rated in the order they were *played*, never the order
    /// they were confirmed, so a Monday game confirmed after a Wednesday one
    /// still lands in Monday's place. Because the key sorts on it, this item
    /// collection is simultaneously the replay source for repair and the
    /// rating-over-time chart — one write, two access patterns.
    ///
    /// The `ladder` segment leads so that one ladder's history is a
    /// contiguous range (see [`Sk::rating_prefix`]). It can never contain
    /// `#`: `rating::Ladder` uses `:` as its sub-ladder separator
    /// (`"tennis:doubles"`) precisely so that `#` stays free as the key
    /// delimiter here. Guarded from this side by
    /// `tests::rating_keys_round_trip_for_every_ladder`, and from the other
    /// by `rating::ladder::tests::ladder_keys_never_contain_the_key_delimiter`.
    Rating {
        ladder: String,
        played_at: String,
        match_id: String,
    },
}

impl Sk {
    /// The static prefix keyword for this variant, without the delimiter —
    /// what `Display` builds the real key from. Not meant for range queries:
    /// a bare keyword can be a literal string-prefix of a different variant's
    /// keyword (e.g. `SCORE` of `SCORESUB`), so `begins_with(SK, ...)` on one
    /// can silently also match items of the other. Use one of the `*_prefix()`
    /// functions below for that instead — each bakes in the delimiter, so it
    /// only ever matches its own variant's items.
    fn prefix(&self) -> &'static str {
        match self {
            Sk::Profile => "#PROFILE",
            Sk::Meta => "#META",
            Sk::Guard => "#GUARD",
            Sk::Follower(_) => "FOLLOWER",
            Sk::Member(_) => "MEMBER",
            Sk::Side(_) => "SIDE",
            Sk::Player(_) => "PLAYER",
            Sk::Score(_) => "LIVESCORE",
            Sk::Like(_) => "LIKE",
            Sk::LiveEvent(_) => "LIVEEVT",
            Sk::ScoreSubmission(_) => "SCORESUB",
            Sk::Comment(_) => "COMMENT",
            Sk::Reply(_) => "REPLY",
            Sk::Notification(_) => "NOTIF",
            Sk::Device(_) => "DEVICE",
            Sk::StatContribution(_) => "STATCONTRIB",
            Sk::Feed { .. } => "FEED",
            Sk::RatingContribution(_) => "RATINGCONTRIB",
            Sk::Rating { .. } => "RATING",
        }
    }

    // -----------------------------------------------------------------
    // Range-query prefixes: one named function per variant that's ever the
    // target of a `begins_with(SK, ...)` / `between` collection query. Each
    // includes the delimiter, so (unlike the bare `prefix()` above) it can
    // never accidentally match a different variant's items too. Add a new
    // one here — never call `Sk::Variant(dummy).prefix()` at a query call
    // site — the next time a query needs one.
    // -----------------------------------------------------------------

    /// Lists a user or team's followers, or a user's own following list:
    /// `FOLLOWER#` (the same prefix either way — see `Sk::Follower`'s doc
    /// comment on why one variant covers both).
    pub fn follower_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Follower(String::new()).prefix())
    }

    /// Lists a user's registered devices: `DEVICE#`.
    pub fn device_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Device(String::new()).prefix())
    }

    /// Lists a team's members: `MEMBER#`.
    pub fn member_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Member(String::new()).prefix())
    }

    /// Lists a match's sides: `SIDE#`.
    pub fn side_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Side(String::new()).prefix())
    }

    /// Lists a match's players: `PLAYER#`.
    pub fn player_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Player(String::new()).prefix())
    }

    /// Lists a match's likes: `LIKE#`.
    pub fn like_prefix() -> String {
        format!("{}{DELIMITER}", Sk::Like(String::new()).prefix())
    }

    /// Lists a match's live-scoring event log: `LIVEEVT#`.
    pub fn live_event_prefix() -> String {
        format!("{}{DELIMITER}", Sk::LiveEvent(0).prefix())
    }

    /// Lists a user or team's stat contributions: `STATCONTRIB#`.
    pub fn stat_contribution_prefix() -> String {
        format!(
            "{}{DELIMITER}",
            Sk::StatContribution(String::new()).prefix()
        )
    }

    /// Lists a viewer's feed: `FEED#`.
    pub fn feed_prefix() -> String {
        format!(
            "{}{DELIMITER}",
            Sk::Feed {
                starts_at: String::new(),
                match_id: String::new(),
            }
            .prefix()
        )
    }

    /// Lists a match's rating contributions: `RATINGCONTRIB#`.
    pub fn rating_contribution_prefix() -> String {
        format!(
            "{}{DELIMITER}",
            Sk::RatingContribution(String::new()).prefix()
        )
    }

    /// The stem every ladder's rating-history prefix extends: `RATING#`.
    ///
    /// Private on purpose — no caller wants it, because a query over *all*
    /// of an account's ladders at once isn't an access pattern we have (the
    /// chart and the replay are both per-ladder). It exists so the three
    /// range helpers below name the keyword once, and so
    /// `no_range_query_prefix_is_a_prefix_of_another` can guard the stem
    /// rather than one arbitrarily-chosen ladder: every ladder prefix
    /// extends this, so a collision with the stem is a collision with all of
    /// them.
    fn rating_stem() -> String {
        format!(
            "{}{DELIMITER}",
            Sk::Rating {
                ladder: String::new(),
                played_at: String::new(),
                match_id: String::new(),
            }
            .prefix()
        )
    }

    /// Lists one ladder's rating history in a user's or team's partition:
    /// `RATING#<ladder>#`. Also the inclusive lower bound of the `BETWEEN`
    /// form built by [`Sk::rating_history_from`] /
    /// [`Sk::rating_history_end`].
    pub fn rating_prefix(ladder: &str) -> String {
        format!("{}{ladder}{DELIMITER}", Sk::rating_stem())
    }

    /// The inclusive lower bound for "this ladder's history from `played_at`
    /// onwards" — `RATING#<ladder>#<played_at>`.
    ///
    /// A key at exactly `played_at` is `RATING#<ladder>#<played_at>#<mid>`,
    /// of which this is a proper string prefix and therefore smaller, so the
    /// bound includes every match played at that instant rather than
    /// straddling them. That matters: the repair replay resumes from a
    /// checkpointed `played_at`, and dropping a same-instant match would
    /// silently lose it from the replay.
    pub fn rating_history_from(ladder: &str, played_at: &str) -> String {
        format!("{}{played_at}", Sk::rating_prefix(ladder))
    }

    /// The inclusive upper bound closing either lower bound above into a
    /// `SK BETWEEN :low AND :high`.
    ///
    /// A range query, not `begins_with`, because DynamoDB permits only one
    /// sort-key condition and repair needs "this ladder, from here onwards" —
    /// which is a range. `begins_with` alone can't express the lower bound
    /// and `SK >= :low` alone can't express the upper: it would run off the
    /// end of `RATING#…` into whatever sort key is added to the user
    /// partition next, which is exactly the class of silent bug
    /// `no_range_query_prefix_is_a_prefix_of_another` exists to prevent.
    ///
    /// The bound is the ladder's prefix followed by
    /// [`RATING_RANGE_SENTINEL`]. Two things make that correct, both tested:
    ///
    /// - It sorts above every key *in* the ladder, because the sentinel is
    ///   above every character an ISO-8601 `played_at` can start with.
    /// - It excludes every key of a *different* ladder, including one this
    ///   ladder's name is a prefix of (`"tennis"` vs `"tennis:doubles"`).
    ///   Where the two names diverge, the character is either above `#` —
    ///   putting the other ladder's keys above this bound — or below it,
    ///   putting them below the lower bound. `#` itself is the one character
    ///   that would break the argument, and a ladder can never contain it.
    pub fn rating_history_end(ladder: &str) -> String {
        format!("{}{RATING_RANGE_SENTINEL}", Sk::rating_prefix(ladder))
    }
}

impl fmt::Display for Sk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Marker keys (the prefix is the whole key).
            Sk::Profile | Sk::Meta | Sk::Guard => write!(f, "{}", self.prefix()),

            // Single-value keys.
            Sk::Follower(v)
            | Sk::Member(v)
            | Sk::Side(v)
            | Sk::Player(v)
            | Sk::Score(v)
            | Sk::Like(v)
            | Sk::ScoreSubmission(v)
            | Sk::Comment(v)
            | Sk::Reply(v)
            | Sk::Notification(v)
            | Sk::Device(v)
            | Sk::StatContribution(v)
            | Sk::RatingContribution(v) => write!(f, "{}{}{}", self.prefix(), DELIMITER, v),

            // Zero-padded so lexicographic order matches numeric seq order.
            Sk::LiveEvent(seq) => write!(f, "LIVEEVT{DELIMITER}{seq:010}"),

            // Feed entries keep the timestamp in the key (list-only).
            Sk::Feed {
                starts_at,
                match_id,
            } => {
                write!(f, "FEED{DELIMITER}{starts_at}{DELIMITER}{match_id}")
            }

            // Rating history keeps the ladder and the match's start time in
            // the key (list-only, ordered by when the match was played).
            Sk::Rating {
                ladder,
                played_at,
                match_id,
            } => {
                write!(
                    f,
                    "RATING{DELIMITER}{ladder}{DELIMITER}{played_at}{DELIMITER}{match_id}"
                )
            }
        }
    }
}

impl FromStr for Sk {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(KeyError::Empty);
        }
        // Marker keys.
        match s {
            "#PROFILE" => return Ok(Sk::Profile),
            "#META" => return Ok(Sk::Meta),
            "#GUARD" => return Ok(Sk::Guard),
            _ => {}
        }

        let (prefix, rest) = s
            .split_once(DELIMITER)
            .ok_or_else(|| KeyError::Malformed(s.into()))?;

        // Splits a compound `<a>#<b>` remainder into two segments.
        let two = |rest: &str| -> Result<(String, String), KeyError> {
            rest.split_once(DELIMITER)
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .ok_or_else(|| KeyError::Malformed(s.into()))
        };

        // Splits a compound `<a>#<b>#<c>` remainder into three segments. The
        // last one absorbs any further delimiters, exactly like `two` above —
        // no value we put in a trailing segment contains one, so this only
        // ever affects how a corrupt key is reported, and round-tripping such
        // a key still yields the same string.
        let three = |rest: &str| -> Result<(String, String, String), KeyError> {
            let mut parts = rest.splitn(3, DELIMITER);
            match (parts.next(), parts.next(), parts.next()) {
                (Some(a), Some(b), Some(c)) => Ok((a.into(), b.into(), c.into())),
                _ => Err(KeyError::Malformed(s.into())),
            }
        };

        match prefix {
            "FOLLOWER" => Ok(Sk::Follower(rest.into())),
            "MEMBER" => Ok(Sk::Member(rest.into())),
            "SIDE" => Ok(Sk::Side(rest.into())),
            "PLAYER" => Ok(Sk::Player(rest.into())),
            "LIVESCORE" => Ok(Sk::Score(rest.into())),
            "LIKE" => Ok(Sk::Like(rest.into())),
            "LIVEEVT" => rest
                .parse::<u32>()
                .map(Sk::LiveEvent)
                .map_err(|_| KeyError::Malformed(s.into())),
            "SCORESUB" => Ok(Sk::ScoreSubmission(rest.into())),
            "COMMENT" => Ok(Sk::Comment(rest.into())),
            "REPLY" => Ok(Sk::Reply(rest.into())),
            "NOTIF" => Ok(Sk::Notification(rest.into())),
            "DEVICE" => Ok(Sk::Device(rest.into())),
            "STATCONTRIB" => Ok(Sk::StatContribution(rest.into())),
            "FEED" => {
                let (starts_at, match_id) = two(rest)?;
                Ok(Sk::Feed {
                    starts_at,
                    match_id,
                })
            }
            "RATINGCONTRIB" => Ok(Sk::RatingContribution(rest.into())),
            "RATING" => {
                let (ladder, played_at, match_id) = three(rest)?;
                Ok(Sk::Rating {
                    ladder,
                    played_at,
                    match_id,
                })
            }
            other => Err(KeyError::UnknownPrefix(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk_roundtrip(pk: Pk, expected: &str) {
        assert_eq!(pk.to_string(), expected, "format");
        assert_eq!(expected.parse::<Pk>().unwrap(), pk, "parse");
    }

    fn sk_roundtrip(sk: Sk, expected: &str) {
        assert_eq!(sk.to_string(), expected, "format");
        assert_eq!(expected.parse::<Sk>().unwrap(), sk, "parse");
    }

    #[test]
    fn pk_variants_roundtrip() {
        pk_roundtrip(Pk::User("u1".into()), "USER#u1");
        pk_roundtrip(
            Pk::EmailGuard("sofia@example.com".into()),
            "EMAIL#sofia@example.com",
        );
        pk_roundtrip(Pk::AuthGuard("sub-abc-123".into()), "AUTH#sub-abc-123");
        pk_roundtrip(Pk::Team("t1".into()), "TEAM#t1");
        pk_roundtrip(Pk::Match("m1".into()), "MATCH#m1");
        pk_roundtrip(Pk::UserFeed("u1".into()), "UFEED#u1");
        pk_roundtrip(Pk::Invitation("i1".into()), "INVITATION#i1");
        pk_roundtrip(Pk::Asset("a1".into()), "ASSET#a1");
    }

    #[test]
    fn email_guard_pk_is_lowercased() {
        assert_eq!(
            Pk::email_guard("Sofia@Example.com").to_string(),
            "EMAIL#sofia@example.com"
        );
    }

    #[test]
    fn sk_marker_variants_roundtrip() {
        sk_roundtrip(Sk::Profile, "#PROFILE");
        sk_roundtrip(Sk::Meta, "#META");
        sk_roundtrip(Sk::Guard, "#GUARD");
    }

    #[test]
    fn sk_live_event_roundtrips_zero_padded() {
        sk_roundtrip(Sk::LiveEvent(0), "LIVEEVT#0000000000");
        sk_roundtrip(Sk::LiveEvent(42), "LIVEEVT#0000000042");
        sk_roundtrip(Sk::LiveEvent(4_294_967_295), "LIVEEVT#4294967295");
    }

    #[test]
    fn sk_live_event_order_matches_numeric_order() {
        let a = Sk::LiveEvent(9).to_string();
        let b = Sk::LiveEvent(10).to_string();
        assert!(a < b, "{a} should sort before {b}");
    }

    #[test]
    fn sk_single_value_variants_roundtrip() {
        sk_roundtrip(Sk::Follower("u2".into()), "FOLLOWER#u2");
        sk_roundtrip(Sk::Member("mem1".into()), "MEMBER#mem1");
        sk_roundtrip(Sk::Side("side_red".into()), "SIDE#side_red");
        sk_roundtrip(Sk::Player("p1".into()), "PLAYER#p1");
        sk_roundtrip(Sk::Score("cricket".into()), "LIVESCORE#cricket");
        sk_roundtrip(Sk::Like("u3".into()), "LIKE#u3");
        // Id-addressed (time-ordered) items now use id-only base SKs.
        sk_roundtrip(Sk::ScoreSubmission("s1".into()), "SCORESUB#s1");
        sk_roundtrip(Sk::Comment("c1".into()), "COMMENT#c1");
        sk_roundtrip(Sk::Reply("r1".into()), "REPLY#r1");
        sk_roundtrip(Sk::Notification("n1".into()), "NOTIF#n1");
        sk_roundtrip(Sk::Device("token-abc".into()), "DEVICE#token-abc");
        sk_roundtrip(Sk::StatContribution("u4".into()), "STATCONTRIB#u4");
    }

    #[test]
    fn sk_feed_variant_roundtrips() {
        let ts = "2026-06-01T10:00:00Z";
        sk_roundtrip(
            Sk::Feed {
                starts_at: ts.into(),
                match_id: "m1".into(),
            },
            "FEED#2026-06-01T10:00:00Z#m1",
        );
    }

    #[test]
    fn sk_rating_variants_roundtrip() {
        sk_roundtrip(Sk::RatingContribution("u5".into()), "RATINGCONTRIB#u5");
        sk_roundtrip(
            Sk::Rating {
                ladder: "squash".into(),
                played_at: "2026-06-01T10:00:00Z".into(),
                match_id: "m1".into(),
            },
            "RATING#squash#2026-06-01T10:00:00Z#m1",
        );
    }

    /// The `:` in a sub-ladder key (`"tennis:doubles"`) is not decoration:
    /// `rating::Ladder` picked it over `#` so that a ladder can be a segment
    /// of this sort key without breaking the round-trip. That contract spans
    /// two modules, so it is asserted from both ends — `rating::ladder`
    /// checks no ladder contains `#`, and this checks that every ladder it
    /// can mint, plus the sub-ladder split that is most likely to happen
    /// next, survives a format/parse cycle with its segments intact.
    #[test]
    fn rating_keys_round_trip_for_every_ladder() {
        use crate::rating::{Sport, ladder_for};

        let ladders: Vec<String> = Sport::ALL
            .into_iter()
            .filter_map(ladder_for)
            .map(|l| l.as_str().to_string())
            // Not mintable today; the split Part 2.5 calls most likely, and
            // the whole reason the separator is `:`.
            .chain(std::iter::once("tennis:doubles".to_string()))
            .collect();

        for ladder in ladders {
            let sk = Sk::Rating {
                ladder: ladder.clone(),
                played_at: "2026-06-01T10:00:00.000Z".into(),
                match_id: "m-Ab_1".into(),
            };
            assert_eq!(sk.to_string().parse::<Sk>().unwrap(), sk, "{ladder}");
        }
    }

    /// The `BETWEEN` bounds must cover exactly one ladder's history: every
    /// key in it, and nothing else in the partition. The interesting case is
    /// a ladder whose name is a string prefix of another's — `"tennis"` and
    /// `"tennis:doubles"` — because that is the shape a future singles/
    /// doubles split creates, and getting it wrong would silently blend two
    /// ladders' histories into one replay.
    #[test]
    fn rating_history_bounds_cover_exactly_one_ladder() {
        let key = |ladder: &str, played_at: &str| {
            Sk::Rating {
                ladder: ladder.into(),
                played_at: played_at.into(),
                match_id: "m1".into(),
            }
            .to_string()
        };

        let low = Sk::rating_prefix("tennis");
        let high = Sk::rating_history_end("tennis");
        assert!(low < high, "{low} must sort below {high}");

        for played_at in [
            "1970-01-01T00:00:00Z",
            "2026-06-01T10:00:00.000Z",
            "9999-12-31T23:59:59Z",
        ] {
            let k = key("tennis", played_at);
            assert!(low <= k && k <= high, "{k} must fall inside {low}..={high}");
        }

        // Neighbouring ladders, on both sides of the divergence character.
        for other in ["tennis:doubles", "table_tennis", "squash", "netball"] {
            let k = key(other, "2026-06-01T10:00:00.000Z");
            assert!(
                k < low || k > high,
                "{other}'s history ({k}) must fall outside tennis's {low}..={high}"
            );
        }

        // And nothing from another item kind in the same partition.
        for other in [
            Sk::Profile.to_string(),
            Sk::Follower("u1".into()).to_string(),
            Sk::Notification("n1".into()).to_string(),
            Sk::Device("t1".into()).to_string(),
            Sk::RatingContribution("u1".into()).to_string(),
        ] {
            assert!(
                other < low || other > high,
                "{other} must fall outside {low}..={high}"
            );
        }
    }

    /// The resume bound is inclusive of matches played at exactly that
    /// instant. A repair replay checkpoints a `played_at` and pages from it;
    /// an exclusive bound would drop every match sharing that timestamp —
    /// which for a club night running four courts at 19:00 is most of them.
    #[test]
    fn rating_history_from_includes_matches_at_that_instant() {
        let at = "2026-06-01T19:00:00.000Z";
        let low = Sk::rating_history_from("squash", at);
        let high = Sk::rating_history_end("squash");
        for match_id in ["", "aaa", "zzz", "m-Ab_1"] {
            let k = Sk::Rating {
                ladder: "squash".into(),
                played_at: at.into(),
                match_id: match_id.into(),
            }
            .to_string();
            assert!(low <= k && k <= high, "{k} must fall inside {low}..={high}");
        }
        // ...and excludes anything played before it.
        let earlier = Sk::Rating {
            ladder: "squash".into(),
            played_at: "2026-06-01T18:59:59.999Z".into(),
            match_id: "m1".into(),
        }
        .to_string();
        assert!(earlier < low, "{earlier} must fall below {low}");
    }

    #[test]
    fn errors_are_reported() {
        assert_eq!("".parse::<Pk>(), Err(KeyError::Empty));
        assert_eq!(
            "NOPREFIX".parse::<Pk>(),
            Err(KeyError::Malformed("NOPREFIX".into()))
        );
        assert_eq!(
            "WAT#x".parse::<Pk>(),
            Err(KeyError::UnknownPrefix("WAT".into()))
        );
        // Feed SK missing its second segment.
        assert_eq!(
            "FEED#only-ts".parse::<Sk>(),
            Err(KeyError::Malformed("FEED#only-ts".into()))
        );
        assert_eq!(
            "BOGUS#a#b".parse::<Sk>(),
            Err(KeyError::UnknownPrefix("BOGUS".into()))
        );
        // Rating SK missing its third segment.
        assert_eq!(
            "RATING#squash#2026-06-01T10:00:00Z".parse::<Sk>(),
            Err(KeyError::Malformed(
                "RATING#squash#2026-06-01T10:00:00Z".into()
            ))
        );
    }

    #[test]
    fn prefix_helpers_support_range_queries() {
        // Each `*_prefix()` includes the delimiter — what a `begins_with`
        // query actually needs, unlike the bare `prefix()` a `Display` impl
        // builds off (see `no_range_query_prefix_is_a_prefix_of_another`).
        assert_eq!(Sk::follower_prefix(), "FOLLOWER#");
        assert_eq!(Sk::member_prefix(), "MEMBER#");
        assert_eq!(Sk::side_prefix(), "SIDE#");
        assert_eq!(Sk::player_prefix(), "PLAYER#");
        assert_eq!(Sk::like_prefix(), "LIKE#");
        assert_eq!(Sk::live_event_prefix(), "LIVEEVT#");
        assert_eq!(Sk::stat_contribution_prefix(), "STATCONTRIB#");
        assert_eq!(Sk::feed_prefix(), "FEED#");
        assert_eq!(Sk::rating_contribution_prefix(), "RATINGCONTRIB#");
        assert_eq!(Sk::rating_prefix("squash"), "RATING#squash#");
        assert_eq!(
            Sk::rating_history_from("squash", "2026-06-01T10:00:00Z"),
            "RATING#squash#2026-06-01T10:00:00Z"
        );
        assert_eq!(Sk::rating_history_end("squash"), "RATING#squash#~");
    }

    #[test]
    fn no_range_query_prefix_is_a_prefix_of_another() {
        // A `begins_with(SK, prefix)` query must only ever match its own
        // variant's items. This is exactly the bug that motivated
        // `LIVESCORE#` over `SCORE#` for `Sk::Score`: `SCORE#` would have
        // been a literal string-prefix of `SCORESUB#`, so a range query
        // meant to list live-scoring records would also match score
        // submissions. `SCORESUB#` has no `*_prefix()` of its own today
        // (score submissions are addressed by id / listed via GSI1) but is
        // included here as a literal precisely so it keeps guarding future
        // additions, not just the ones that already have a function.
        //
        // `Sk::rating_prefix` is ladder-parameterised, so what goes in the
        // list is its stem (`RATING#`) rather than one arbitrary ladder:
        // every ladder prefix extends the stem, so guarding the stem guards
        // all of them at once — and putting both in would trivially fail,
        // the stem being a prefix of each. The assertion below the loop
        // pins the helper to the stem so that stays true. `RATING#` versus
        // `RATINGCONTRIB#` is the near-miss this pairing is really here for:
        // the two differ only at the delimiter, exactly as `SCORE#` and
        // `SCORESUB#` did.
        let prefixes = [
            Sk::follower_prefix(),
            Sk::member_prefix(),
            Sk::side_prefix(),
            Sk::player_prefix(),
            Sk::like_prefix(),
            Sk::live_event_prefix(),
            Sk::stat_contribution_prefix(),
            Sk::feed_prefix(),
            Sk::rating_contribution_prefix(),
            Sk::rating_stem(),
            "SCORESUB#".to_string(),
        ];
        for (i, a) in prefixes.iter().enumerate() {
            for (j, b) in prefixes.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a.as_str()), "{a:?} is a prefix of {b:?}");
                }
            }
        }

        let stem = Sk::rating_stem();
        for ladder in ["squash", "tennis", "tennis:doubles"] {
            for prefix in [
                Sk::rating_prefix(ladder),
                Sk::rating_history_from(ladder, "2026-06-01T10:00:00Z"),
                Sk::rating_history_end(ladder),
            ] {
                assert!(
                    prefix.starts_with(&stem),
                    "{prefix:?} must extend the guarded stem {stem:?}"
                );
            }
        }
    }
}
