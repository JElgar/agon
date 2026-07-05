//! Follow-graph operations (user→user and user→team), with atomic counter
//! maintenance and cursor-paginated listing.

use aws_sdk_dynamodb::types::{Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_GSI3PK, ATTR_PK, ItemBuilder, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{TeamFollowRecord, UserFollowRecord};

pub const TYPE_USER_FOLLOW: &str = "user_follow";
pub const TYPE_TEAM_FOLLOW: &str = "team_follow";

impl Dao {
    /// Follow a user. Idempotent: re-following is a no-op that does not
    /// double-count. Atomically: writes the edge (if absent), bumps the
    /// followee's `follower_count` and the follower's `following_count`.
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

    /// List a user's followers (the edge records), cursor-paginated.
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
                .expression_attribute_values(":sk", s(Sk::Follower(String::new()).prefix())),
            cursor,
            limit,
        )
        .await
    }

    /// List the users a given user follows, via GSI1 (`UFOLLOWING#<id>`).
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

    /// Follow a team. Idempotent. Bumps the team's `follower_count`.
    pub async fn follow_team(&self, follower_id: &str, team_id: &str, now: &str) -> DaoResult<()> {
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

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_edge).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(counter_delta(
                        self.table(),
                        &Pk::Team(team_id.into()),
                        "follower_count",
                        1,
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

    /// Unfollow a team. Idempotent.
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
                .expression_attribute_values(":sk", s(Sk::Follower(String::new()).prefix())),
            cursor,
            limit,
        )
        .await
    }

    /// List the teams a user follows, via GSI3.
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
