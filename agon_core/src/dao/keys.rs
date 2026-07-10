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
    /// The static prefix keyword for this variant (without the delimiter).
    pub fn prefix(&self) -> &'static str {
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

    /// Per-sport stats for a user. `STATS#<sport>`
    Stats(String),
    /// A follower edge (who follows this user/team). `FOLLOWER#<followerUid>`
    Follower(String),
    /// A team membership. `MEMBER#<membershipId>`
    Member(String),
    /// A match side. `SIDE#<sideId>`
    Side(String),
    /// A match player. `PLAYER#<playerId>`
    Player(String),
    /// A match's detailed score, keyed by sport. `DETAIL#<sport>`
    Detail(String),
    /// A like on a match. `LIKE#<uid>`
    Like(String),

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
    /// A fan-out feed entry, ordered by match start time. `FEED#<starts_at>#<mid>`
    /// (only ever listed, never addressed by id — keeps ts in the key).
    Feed { starts_at: String, match_id: String },
}

impl Sk {
    /// The static prefix keyword for this variant (without the delimiter). Use
    /// with `begins_with` / `between` to query an item collection, e.g. all of a
    /// match's comments: `begins_with(SK, Sk::comment_prefix())`.
    pub fn prefix(&self) -> &'static str {
        match self {
            Sk::Profile => "#PROFILE",
            Sk::Meta => "#META",
            Sk::Guard => "#GUARD",
            Sk::Stats(_) => "STATS",
            Sk::Follower(_) => "FOLLOWER",
            Sk::Member(_) => "MEMBER",
            Sk::Side(_) => "SIDE",
            Sk::Player(_) => "PLAYER",
            Sk::Detail(_) => "DETAIL",
            Sk::Like(_) => "LIKE",
            Sk::ScoreSubmission(_) => "SCORESUB",
            Sk::Comment(_) => "COMMENT",
            Sk::Reply(_) => "REPLY",
            Sk::Notification(_) => "NOTIF",
            Sk::Feed { .. } => "FEED",
        }
    }
}

impl fmt::Display for Sk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Marker keys (the prefix is the whole key).
            Sk::Profile | Sk::Meta | Sk::Guard => write!(f, "{}", self.prefix()),

            // Single-value keys.
            Sk::Stats(v)
            | Sk::Follower(v)
            | Sk::Member(v)
            | Sk::Side(v)
            | Sk::Player(v)
            | Sk::Detail(v)
            | Sk::Like(v)
            | Sk::ScoreSubmission(v)
            | Sk::Comment(v)
            | Sk::Reply(v)
            | Sk::Notification(v) => write!(f, "{}{}{}", self.prefix(), DELIMITER, v),

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
            "STATS" => Ok(Sk::Stats(rest.into())),
            "FOLLOWER" => Ok(Sk::Follower(rest.into())),
            "MEMBER" => Ok(Sk::Member(rest.into())),
            "SIDE" => Ok(Sk::Side(rest.into())),
            "PLAYER" => Ok(Sk::Player(rest.into())),
            "DETAIL" => Ok(Sk::Detail(rest.into())),
            "LIKE" => Ok(Sk::Like(rest.into())),
            "SCORESUB" => Ok(Sk::ScoreSubmission(rest.into())),
            "COMMENT" => Ok(Sk::Comment(rest.into())),
            "REPLY" => Ok(Sk::Reply(rest.into())),
            "NOTIF" => Ok(Sk::Notification(rest.into())),
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
    fn sk_single_value_variants_roundtrip() {
        sk_roundtrip(Sk::Stats("tennis".into()), "STATS#tennis");
        sk_roundtrip(Sk::Follower("u2".into()), "FOLLOWER#u2");
        sk_roundtrip(Sk::Member("mem1".into()), "MEMBER#mem1");
        sk_roundtrip(Sk::Side("side_red".into()), "SIDE#side_red");
        sk_roundtrip(Sk::Player("p1".into()), "PLAYER#p1");
        sk_roundtrip(Sk::Detail("cricket".into()), "DETAIL#cricket");
        sk_roundtrip(Sk::Like("u3".into()), "LIKE#u3");
        // Id-addressed (time-ordered) items now use id-only base SKs.
        sk_roundtrip(Sk::ScoreSubmission("s1".into()), "SCORESUB#s1");
        sk_roundtrip(Sk::Comment("c1".into()), "COMMENT#c1");
        sk_roundtrip(Sk::Reply("r1".into()), "REPLY#r1");
        sk_roundtrip(Sk::Notification("n1".into()), "NOTIF#n1");
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
        // The prefix is what a begins_with query would use for a collection.
        assert_eq!(Sk::Comment("y".into()).prefix(), "COMMENT");
        assert_eq!(Sk::Follower("x".into()).prefix(), "FOLLOWER");
        assert_eq!(Pk::Match("m1".into()).prefix(), "MATCH");
    }
}
