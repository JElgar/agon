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
            | Sk::StatContribution(v) => write!(f, "{}{}{}", self.prefix(), DELIMITER, v),

            // Zero-padded so lexicographic order matches numeric seq order.
            Sk::LiveEvent(seq) => write!(f, "LIVEEVT{DELIMITER}{seq:010}"),

            // Feed entries keep the timestamp in the key (list-only).
            Sk::Feed {
                starts_at,
                match_id,
            } => {
                write!(f, "FEED{DELIMITER}{starts_at}{DELIMITER}{match_id}")
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
        let prefixes = [
            Sk::follower_prefix(),
            Sk::member_prefix(),
            Sk::side_prefix(),
            Sk::player_prefix(),
            Sk::like_prefix(),
            Sk::live_event_prefix(),
            Sk::stat_contribution_prefix(),
            Sk::feed_prefix(),
            "SCORESUB#".to_string(),
        ];
        for (i, a) in prefixes.iter().enumerate() {
            for (j, b) in prefixes.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a.as_str()), "{a:?} is a prefix of {b:?}");
                }
            }
        }
    }
}
