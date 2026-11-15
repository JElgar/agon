//! The entity index (GSI4): every match, user, team and tournament, listed by
//! type, newest first.
//!
//! | Attribute | Value |
//! |---|---|
//! | `GSI4PK` | `ENTITY#<type>`, e.g. `ENTITY#match` |
//! | `GSI4SK` | `<created_at>#<id>` (ISO-8601, so it sorts by creation time) |
//!
//! It exists so offline jobs (the search and stats backfills) and admin lists
//! can enumerate a record type without a full-table `Scan`, and without
//! depending on a search index that might itself be what's being rebuilt.
//!
//! **Keys only.** The index projects nothing but keys, so DynamoDB writes to it
//! only when an item is created, deleted or has its index keys changed. Likes,
//! comments and edits rewrite the base item but never cost an index write.
//! Readers get ids back and fetch the items themselves.
//!
//! **Written on create only.** The four create paths (`create_match`,
//! `create_user`, `create_team`, `create_tournament`) set the keys. Every later
//! write to those items is an `UpdateItem`, which leaves the keys in place.
//! Items written before the index existed get their keys from a one-off
//! migration script, kept off `main` on the `claude/entity-index-migration`
//! branch (`cargo run -p agon_services_core --example migrate_entity_index`).
//!
//! **One partition per type.** A single `ENTITY#match` partition takes about
//! 1,000 index writes a second, and index writes only happen on create, so
//! that's 1,000 new matches a second. Spread it over `ENTITY#match#<n>` if it
//! ever gets close.

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI4PK, item_pk, item_sk, s};
use super::keys::{Pk, Sk};
use super::page::{Page, decode_cursor, encode_cursor};

/// The GSI name in the table definition.
pub const ENTITY_INDEX: &str = "GSI4";

/// A record type the entity index lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Match,
    User,
    Team,
    Tournament,
}

impl EntityKind {
    pub const ALL: [EntityKind; 4] = [
        EntityKind::Match,
        EntityKind::User,
        EntityKind::Team,
        EntityKind::Tournament,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            EntityKind::Match => "match",
            EntityKind::User => "user",
            EntityKind::Team => "team",
            EntityKind::Tournament => "tournament",
        }
    }

    /// `GSI4PK` for this type.
    pub fn index_pk(self) -> String {
        format!("ENTITY#{}", self.tag())
    }

    /// The main-table key of this type's indexed item.
    pub fn item_key(self, id: &str) -> (Pk, Sk) {
        match self {
            EntityKind::Match => (Pk::Match(id.into()), Sk::Meta),
            EntityKind::User => (Pk::User(id.into()), Sk::Profile),
            EntityKind::Team => (Pk::Team(id.into()), Sk::Meta),
            EntityKind::Tournament => (Pk::Tournament(id.into()), Sk::Meta),
        }
    }

    /// The kind and id of an indexed item's main-table key, or `None` for any
    /// other item.
    pub fn of_key(pk: &Pk, sk: &Sk) -> Option<(EntityKind, String)> {
        match (pk, sk) {
            (Pk::Match(id), Sk::Meta) => Some((EntityKind::Match, id.clone())),
            (Pk::User(id), Sk::Profile) => Some((EntityKind::User, id.clone())),
            (Pk::Team(id), Sk::Meta) => Some((EntityKind::Team, id.clone())),
            (Pk::Tournament(id), Sk::Meta) => Some((EntityKind::Tournament, id.clone())),
            _ => None,
        }
    }
}

/// `GSI4SK`: creation time then id, so the index sorts by creation time and ids
/// break ties.
pub fn index_sk(created_at: &str, id: &str) -> String {
    format!("{created_at}#{id}")
}

impl Dao {
    /// One page of ids of `kind`, newest first (or oldest first), from the entity
    /// index. Fetch the items themselves with the usual getters or batch gets.
    #[tracing::instrument(skip(self))]
    pub async fn list_entity_ids(
        &self,
        kind: EntityKind,
        newest_first: bool,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<String>> {
        let start_key = cursor.map(decode_cursor).transpose()?;
        let out = self
            .client
            .query()
            .table_name(self.table())
            .index_name(ENTITY_INDEX)
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_GSI4PK)
            .expression_attribute_values(":pk", s(kind.index_pk()))
            .scan_index_forward(!newest_first)
            .limit(limit as i32)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut items = Vec::new();
        for item in out.items.unwrap_or_default() {
            if let Some((k, id)) = EntityKind::of_key(&item_pk(&item)?, &item_sk(&item)?)
                && k == kind
            {
                items.push(id);
            }
        }
        let next_cursor = out.last_evaluated_key.map(encode_cursor).transpose()?;
        Ok(Page { items, next_cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_for_every_kind() {
        for kind in EntityKind::ALL {
            let (pk, sk) = kind.item_key("abc");
            assert_eq!(
                EntityKind::of_key(&pk, &sk),
                Some((kind, "abc".to_string()))
            );
            assert_eq!(kind.index_pk(), format!("ENTITY#{}", kind.tag()));
        }
    }

    #[test]
    fn other_items_are_not_indexed() {
        assert_eq!(
            EntityKind::of_key(&Pk::Match("m".into()), &Sk::Player("p".into())),
            None
        );
        assert_eq!(EntityKind::of_key(&Pk::User("u".into()), &Sk::Meta), None);
    }

    #[test]
    fn sort_key_orders_by_creation_time() {
        let older = index_sk("2026-01-01T00:00:00Z", "zzz");
        let newer = index_sk("2026-06-01T00:00:00Z", "aaa");
        assert!(older < newer);
    }
}
