//! Notification operations: write, list (newest first), unread count, mark read.
//!
//! Notifications live under the user partition (`USER#<uid>` / `NOTIF#<ts>#<id>`).
//! Writes are normally produced by the async stream worker, not synchronously in
//! request handlers. The unread badge count is a counter on the user's profile
//! item, kept in step with the notifications' `is_read` flags.

use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_PK, ItemBuilder, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::NotificationRecord;

pub const TYPE_NOTIFICATION: &str = "notification";

impl Dao {
    /// Append a notification for a user and bump their `unread_count`, atomically.
    ///
    /// Addressed by id (`NOTIF#<id>`); time ordering for the feed is via GSI1
    /// (`UNOTIFS#<uid>` / `<createdAt>#<id>`).
    ///
    /// **Idempotent by id**: the Put is guarded with `attribute_not_exists`, so a
    /// redelivered stream event (the async worker generates notifications with a
    /// deterministic id per source event) is a no-op and does not double-count
    /// the unread badge. See docs/async-design.md §3.
    pub async fn create_notification(&self, notif: &NotificationRecord) -> DaoResult<()> {
        let item = ItemBuilder::new(to_item(
            &Pk::User(notif.user_id.clone()),
            &Sk::Notification(notif.id.clone()),
            TYPE_NOTIFICATION,
            notif,
        )?)
        .gsi1(
            format!("UNOTIFS#{}", notif.user_id),
            format!("{}#{}", notif.created_at, notif.id),
        )
        .build();
        let put = Put::builder()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.unread_delta(&notif.user_id, 1)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            // Notification with this id already exists → duplicate delivery, no-op.
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List a user's notifications, newest first, via GSI1.
    pub async fn list_notifications(
        &self,
        user_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<NotificationRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("UNOTIFS#{user_id}")))
                .scan_index_forward(false), // newest first
            cursor,
            limit,
        )
        .await
    }

    /// The unread notification count for the bell badge (read off the profile
    /// counter). Zero if the user or counter is absent.
    pub async fn unread_notification_count(&self, user_id: &str) -> DaoResult<u64> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Profile.to_string()))
            .projection_expression("unread_count")
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let count = out
            .item
            .and_then(|i| i.get("unread_count").cloned())
            .and_then(|v| match v {
                AttributeValue::N(n) => n.parse::<u64>().ok(),
                _ => None,
            })
            .unwrap_or(0);
        Ok(count)
    }

    /// Mark a single notification read (addressed by id). Only decrements
    /// `unread_count` if the notification was actually unread (conditional), so
    /// the counter can't drift below zero on repeated calls.
    pub async fn mark_notification_read(
        &self,
        user_id: &str,
        notification_id: &str,
    ) -> DaoResult<()> {
        // Flip is_read false→true only if currently unread.
        let mark = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(
                "SK",
                s(Sk::Notification(notification_id.into()).to_string()),
            )
            .update_expression("SET is_read = :t")
            .condition_expression("attribute_exists(#pk) AND is_read = :f")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":t", AttributeValue::Bool(true))
            .expression_attribute_values(":f", AttributeValue::Bool(false))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(mark).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.unread_delta(user_id, -1)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            // Already read (or missing) → the conditional flip failed; no-op.
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Reset the unread counter to zero (the "mark all read" fast path). Note:
    /// this zeroes the badge counter; flipping every notification's `is_read`
    /// flag is left to the async worker (a bulk update over the collection) so
    /// the request stays O(1).
    pub async fn mark_all_notifications_read(&self, user_id: &str) -> DaoResult<()> {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Profile.to_string()))
            .update_expression("SET unread_count = :zero")
            .expression_attribute_values(":zero", AttributeValue::N("0".into()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// An `Update` adding `delta` to the profile's `unread_count`.
    fn unread_delta(&self, user_id: &str, delta: i64) -> DaoResult<Update> {
        Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Profile.to_string()))
            .update_expression("ADD unread_count :d")
            .expression_attribute_values(":d", AttributeValue::N(delta.to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))
    }
}
