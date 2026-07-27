//! Device (push-token) registration: register, list, unregister.
//!
//! Devices live under the user partition (`USER#<uid>` / `DEVICE#<token>`).
//! Re-registering an existing token (e.g. on every app open) is a plain
//! refresh, not a new device, and never touches the counter below.
//! Unregistering a token that isn't registered to the caller is a 404
//! ([`DaoError::NotFound`]), not a silent no-op — see `delete_device`.
//!
//! `list_devices` relies on a user's device count staying small (it reads one
//! unpaginated page), so `register_device` enforces [`MAX_DEVICES_PER_USER`]
//! as a hard cap on *new* tokens, tracked via a `device_count` counter on the
//! user's profile item (same pattern as `follower_count`/`unread_count`
//! elsewhere). The cap check and the increment happen in a single
//! conditional `Update` inside one transaction with the device write itself
//! — there's no read-then-write gap for two concurrent registrations to race
//! through, unlike a separate "count, then decide" step would have.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{DevicePlatform, DeviceRecord};

pub const TYPE_DEVICE: &str = "device";

/// Max registered push tokens per user. Generous enough for every real
/// device someone would own (a few phones/tablets/browsers), while keeping
/// `list_devices`'s single unpaginated `Query` page provably sufficient.
pub const MAX_DEVICES_PER_USER: u32 = 20;

impl Dao {
    /// Register (or re-register) a push token for a user. Idempotent: writing
    /// the same token again just refreshes its fields, no counter change.
    /// Rejects a genuinely new token once the user is already at
    /// [`MAX_DEVICES_PER_USER`].
    pub async fn register_device(
        &self,
        user_id: &str,
        push_token: &str,
        platform: DevicePlatform,
        now: &str,
    ) -> DaoResult<()> {
        let record = DeviceRecord {
            user_id: user_id.to_string(),
            push_token: push_token.to_string(),
            platform,
            created_at: now.to_string(),
        };
        let item = to_item(
            &Pk::User(user_id.into()),
            &Sk::Device(push_token.into()),
            TYPE_DEVICE,
            &record,
        )?;

        // Attempt: create the device item (only if it doesn't already exist)
        // and bump the counter (only if under cap) atomically. Which of the
        // two conditions failed — if either did — is read back from the
        // cancellation reasons below, so both branches stay race-free: we
        // never act on a separate, possibly-stale read of either fact.
        let put_new_device = Put::builder()
            .table_name(self.table())
            .set_item(Some(item.clone()))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_new_device).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(device_count_increment(self.table(), user_id)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if item_condition_failed(&e, 0) => {
                // The device item already existed — this is a refresh, not a
                // new registration. Plain upsert, counter untouched.
                self.client
                    .put_item()
                    .table_name(self.table())
                    .set_item(Some(item))
                    .send()
                    .await
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                Ok(())
            }
            Err(e) if item_condition_failed(&e, 1) => Err(DaoError::Conflict(format!(
                "user {user_id} already has the maximum of {MAX_DEVICES_PER_USER} registered devices"
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List every device registered for a user. Unpaginated — capped at
    /// [`MAX_DEVICES_PER_USER`] by `register_device`, so one page always
    /// covers the whole collection.
    pub async fn list_devices(&self, user_id: &str) -> DaoResult<Vec<DeviceRecord>> {
        let page = self
            .query_page(
                self.client
                    .query()
                    .table_name(self.table())
                    .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                    .expression_attribute_names("#pk", ATTR_PK)
                    .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
                    .expression_attribute_values(":sk", s(Sk::Device(String::new()).prefix())),
                None,
                MAX_DEVICES_PER_USER,
            )
            .await?;
        Ok(page.items)
    }

    /// Unregister a push token. Errors with [`DaoError::NotFound`] if the
    /// token isn't registered to this user (already removed, never
    /// registered, or belongs to someone else) — not idempotent, deliberately:
    /// unlike a social edge (follow/unfollow), "unregister this exact token"
    /// is a delete-by-id, and a caller unregistering something that was never
    /// there is worth surfacing, not swallowing. The counter is only
    /// decremented when a device was actually removed, atomically with that
    /// removal.
    pub async fn delete_device(&self, user_id: &str, push_token: &str) -> DaoResult<()> {
        let delete_device = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Device(push_token.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete_device).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(device_count_delta(self.table(), user_id, -1)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::NotFound(
                format!("device {push_token} for user {user_id}"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

/// An `Update` that atomically increments the user's `device_count` — but
/// only if it's currently below [`MAX_DEVICES_PER_USER`]. The check and the
/// increment are one DynamoDB operation, so there's no gap between "read the
/// count" and "write the increment" for a second concurrent registration to
/// land in. `attribute_not_exists` covers a profile that's never had a
/// device registered (no `device_count` attribute yet, i.e. implicitly 0).
fn device_count_increment(table: &str, user_id: &str) -> DaoResult<Update> {
    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
        .key("SK", s(Sk::Profile.to_string()))
        .update_expression("ADD device_count :one")
        .condition_expression("attribute_not_exists(device_count) OR device_count < :max")
        .expression_attribute_values(":one", AttributeValue::N("1".into()))
        .expression_attribute_values(":max", AttributeValue::N(MAX_DEVICES_PER_USER.to_string()))
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))
}

/// An `Update` that unconditionally adds `delta` to the user's
/// `device_count`. Safe to leave unconditional itself because it's always
/// paired, in the same transaction, with a `Delete` guarded by
/// `attribute_exists` — the whole transaction (and so this decrement) only
/// ever commits when a device was actually removed.
fn device_count_delta(table: &str, user_id: &str, delta: i64) -> DaoResult<Update> {
    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
        .key("SK", s(Sk::Profile.to_string()))
        .update_expression("ADD device_count :d")
        .expression_attribute_values(":d", AttributeValue::N(delta.to_string()))
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))
}

/// Whether the transact-item at `index` (0-based, matching the order items
/// were added via `transact_items`) failed its own `ConditionExpression`, for
/// a cancelled `TransactWriteItems`. `false` for any other kind of failure
/// (network, throttling, ...) — those should propagate as real errors, not
/// get misread as "the condition failed".
fn item_condition_failed(err: &SdkError<TransactWriteItemsError>, index: usize) -> bool {
    match err {
        SdkError::ServiceError(se) => match se.err() {
            TransactWriteItemsError::TransactionCanceledException(e) => {
                e.cancellation_reasons().get(index).and_then(|r| r.code())
                    == Some("ConditionalCheckFailed")
            }
            _ => false,
        },
        _ => false,
    }
}
