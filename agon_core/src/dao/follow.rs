//! Follow-graph operations (user→user and user→team), with atomic counter
//! maintenance and cursor-paginated listing.

use std::collections::{HashMap, HashSet};

use aws_sdk_dynamodb::types::{Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_GSI3PK, ATTR_PK, ATTR_SK, ItemBuilder, item_pk, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{TeamFollowRecord, UserFollowRecord};

pub const TYPE_USER_FOLLOW: &str = "user_follow";
pub const TYPE_TEAM_FOLLOW: &str = "team_follow";

impl Dao {
    /// Follow a user. Idempotent: re-following is a no-op that does not
    /// double-count. Atomically: writes the edge (if absent), bumps the
    /// followee's `follower_count` and the follower's `following_count`.
    #[tracing::instrument(skip(self))]
    pub async fn follow_user(
        &self,
        follower_id: &str,
        followee_id: &str,
        now: &str,
    ) -> DaoResult<()> {
        if follower_id == followee_id {
            return Err(DaoError::Conflict("cannot follow yourself".into()));
        }

        let edge = UserFollowRecord {
            followee_id: followee_id.into(),
            follower_id: follower_id.into(),
            created_at: now.into(),
        };
        // Edge lives under the followee; projected to GSI1 for "following" list.
        let edge_item = ItemBuilder::new(to_item(
            &Pk::User(followee_id.into()),
            &Sk::Follower(follower_id.into()),
            TYPE_USER_FOLLOW,
            &edge,
        )?)
        .gsi1(
            format!("UFOLLOWING#{follower_id}"),
            Pk::User(followee_id.into()).to_string(),
        )
        .build();

        let put_edge = Put::builder()
            .table_name(self.table())
            .set_item(Some(edge_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_edge).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::User(followee_id.into()),
                        "follower_count",
                        1,
                    )?)
                    .build(),
            )
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::User(follower_id.into()),
                        "following_count",
                        1,
                    )?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            // Edge already exists → already following; treat as success (idempotent).
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Unfollow a user. Idempotent: only decrements when an edge actually
    /// existed, so repeated unfollows don't drive counts negative.
    #[tracing::instrument(skip(self))]
    pub async fn unfollow_user(&self, follower_id: &str, followee_id: &str) -> DaoResult<()> {
        let delete_edge = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(followee_id.into()).to_string()))
            .key("SK", s(Sk::Follower(follower_id.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete_edge).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::User(followee_id.into()),
                        "follower_count",
                        -1,
                    )?)
                    .build(),
            )
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::User(follower_id.into()),
                        "following_count",
                        -1,
                    )?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            // No edge existed → nothing to undo; treat as success (idempotent).
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// True if `follower_id` follows `followee_id` (drives `is_followed_by_me`).
    #[tracing::instrument(skip(self))]
    pub async fn is_following_user(&self, follower_id: &str, followee_id: &str) -> DaoResult<bool> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(followee_id.into()).to_string()))
            .key("SK", s(Sk::Follower(follower_id.into()).to_string()))
            .projection_expression(ATTR_PK) // existence check only
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(out.item.is_some())
    }

    /// Of `followee_ids`, which does `follower_id` follow? Returns the subset as
    /// a set, for populating `is_followed_by_me` across a page of users in one
    /// round-trip instead of one `is_following_user` call each.
    ///
    /// Each follow edge is an exact-key existence check (`USER#<followee>` /
    /// `FOLLOWER#<follower>`), so these collapse into a single `BatchGetItem`
    /// (see [`batch_get_all`] for the unprocessed-key retry); we project just the
    /// `PK` since we only need existence, then recover which followees came back.
    /// `follower_id` itself is skipped (you never follow yourself). Callers must
    /// pass at most `BATCH_GET_MAX` ids — a single, service-capped page.
    ///
    /// [`batch_get_all`]: Dao::batch_get_all
    #[tracing::instrument(skip(self))]
    pub async fn batch_is_following_users(
        &self,
        follower_id: &str,
        followee_ids: &[String],
    ) -> DaoResult<HashSet<String>> {
        let mut seen = HashSet::new();
        let keys: Vec<_> = followee_ids
            .iter()
            .filter(|id| id.as_str() != follower_id && seen.insert((*id).clone()))
            .map(|followee| {
                HashMap::from([
                    (
                        ATTR_PK.to_string(),
                        s(Pk::User(followee.clone()).to_string()),
                    ),
                    (
                        ATTR_SK.to_string(),
                        s(Sk::Follower(follower_id.to_string()).to_string()),
                    ),
                ])
            })
            .collect();

        let items = self.batch_get_all(keys, Some(ATTR_PK)).await?;
        let mut following = HashSet::with_capacity(items.len());
        for item in items {
            if let Pk::User(followee) = item_pk(&item)? {
                following.insert(followee);
            }
        }
        Ok(following)
    }

    /// Whether `follower_id` follows the team `team_id`. Existence check on the
    /// `TEAM#<team>` / `FOLLOWER#<follower>` edge.
    #[tracing::instrument(skip(self))]
    pub async fn is_following_team(&self, follower_id: &str, team_id: &str) -> DaoResult<bool> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Team(team_id.into()).to_string()))
            .key("SK", s(Sk::Follower(follower_id.into()).to_string()))
            .projection_expression(ATTR_PK) // existence check only
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(out.item.is_some())
    }

    /// List a user's followers (the edge records), cursor-paginated.
    #[tracing::instrument(skip(self))]
    pub async fn list_user_followers(
        &self,
        user_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<UserFollowRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
                .expression_attribute_values(":sk", s(Sk::follower_prefix())),
            cursor,
            limit,
        )
        .await
    }

    /// List the users a given user follows, via GSI1 (`UFOLLOWING#<id>`).
    #[tracing::instrument(skip(self))]
    pub async fn list_user_following(
        &self,
        follower_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<UserFollowRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("UFOLLOWING#{follower_id}"))),
            cursor,
            limit,
        )
        .await
    }

    /// Build the writes that make `follower_id` a follower of `team_id`: an
    /// edge put (conditioned on absence, so it doesn't double-count a repeat)
    /// plus the team's `follower_count` bump. Doesn't send anything — a
    /// caller folds these into its own membership transaction so "join the
    /// team" and "follow the team" land atomically, rather than as a
    /// separate best-effort call after the fact (see `Dao::create_team`,
    /// `Dao::add_team_member`, `Dao::accept_invitation_items`). Membership
    /// always implies following: a team's matches should reach a member's
    /// own feed (and fan-out audience) without them having to separately
    /// opt in. Callers that might already be following (anyone but a brand
    /// new team's own creation) should check `is_following_team` first and
    /// skip these items entirely rather than relying on the condition to
    /// fail — a failed condition cancels the *whole* transaction, including
    /// the membership write it's bundled with.
    pub(super) fn follow_team_items(
        &self,
        follower_id: &str,
        team_id: &str,
        now: &str,
    ) -> DaoResult<Vec<TransactWriteItem>> {
        let edge = TeamFollowRecord {
            team_id: team_id.into(),
            follower_id: follower_id.into(),
            created_at: now.into(),
        };
        // Edge under the team; projected to GSI3 for "teams I follow".
        let edge_item = ItemBuilder::new(to_item(
            &Pk::Team(team_id.into()),
            &Sk::Follower(follower_id.into()),
            TYPE_TEAM_FOLLOW,
            &edge,
        )?)
        .gsi3(
            format!("UFOLLOWS_TEAM#{follower_id}"),
            Pk::Team(team_id.into()).to_string(),
        )
        .build();

        let put_edge = Put::builder()
            .table_name(self.table())
            .set_item(Some(edge_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        Ok(vec![
            TransactWriteItem::builder().put(put_edge).build(),
            TransactWriteItem::builder()
                .update(counter_delta(
                    self.table(),
                    &Pk::Team(team_id.into()),
                    "follower_count",
                    1,
                )?)
                .build(),
        ])
    }

    /// Follow a team. Idempotent. Bumps the team's `follower_count`. For the
    /// explicit "follow this team" action — membership implies following
    /// too, but that's folded directly into the membership write instead of
    /// going through this method (see `follow_team_items`).
    #[tracing::instrument(skip(self))]
    pub async fn follow_team(&self, follower_id: &str, team_id: &str, now: &str) -> DaoResult<()> {
        let items = self.follow_team_items(follower_id, team_id, now)?;
        let mut tx = self.client.transact_write_items();
        for item in items {
            tx = tx.transact_items(item);
        }

        match tx.send().await {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Unfollow a team. Idempotent.
    #[tracing::instrument(skip(self))]
    pub async fn unfollow_team(&self, follower_id: &str, team_id: &str) -> DaoResult<()> {
        let delete_edge = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Team(team_id.into()).to_string()))
            .key("SK", s(Sk::Follower(follower_id.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete_edge).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::Team(team_id.into()),
                        "follower_count",
                        -1,
                    )?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List a team's followers, cursor-paginated.
    #[tracing::instrument(skip(self))]
    pub async fn list_team_followers(
        &self,
        team_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<TeamFollowRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::Team(team_id.into()).to_string()))
                .expression_attribute_values(":sk", s(Sk::follower_prefix())),
            cursor,
            limit,
        )
        .await
    }

    /// List the teams a user follows, via GSI3.
    #[tracing::instrument(skip(self))]
    pub async fn list_followed_teams(
        &self,
        follower_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<TeamFollowRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI3")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI3PK)
                .expression_attribute_values(":pk", s(format!("UFOLLOWS_TEAM#{follower_id}"))),
            cursor,
            limit,
        )
        .await
    }
}

/// Build an `Update` that atomically adds `delta` to a counter on the `#PROFILE`
/// / `#META` singleton of the given partition. Uses `ADD`, which treats a
/// missing attribute as 0.
fn counter_delta(table: &str, pk: &Pk, counter: &str, delta: i64) -> DaoResult<Update> {
    // The counter lives on the profile item for users, meta item for teams.
    let sk = match pk {
        Pk::User(_) => Sk::Profile,
        _ => Sk::Meta,
    };
    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(pk.to_string()))
        .key("SK", s(sk.to_string()))
        .update_expression("ADD #c :d")
        .expression_attribute_names("#c", counter)
        .expression_attribute_values(
            ":d",
            aws_sdk_dynamodb::types::AttributeValue::N(delta.to_string()),
        )
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))
}
