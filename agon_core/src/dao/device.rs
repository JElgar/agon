//! Device (push-token) registration: register, list, unregister.
//!
//! Devices live under the user partition (`USER#<uid>` / `DEVICE#<token>`).
//! Unlike most collections here there's no uniqueness guard or counter to
//! maintain — the token itself is the key, so re-registering the same token
//! (e.g. on every app open) is a plain overwrite, and unregistering a token
//! that's already gone is a harmless no-op.
//!
//! `list_devices` relies on a user's device count staying small (it reads one
//! unpaginated page), so `register_device` enforces [`MAX_DEVICES_PER_USER`]
//! as a cap on *new* tokens — re-registering an existing one never counts
//! against it. The two are tied to the same constant so `list_devices` can
//! never silently truncate a user's real device list. The cap check is
//! best-effort, not atomic (a `get` + a count `Query`, no transaction): two
//! concurrent registrations from a genuinely new user could both pass and
//! push the count one over. That's an acceptable race for a hygiene limit,
//! not a security boundary.

use aws_sdk_dynamodb::types::Select;

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
    /// the same token again just refreshes its fields. Rejects a genuinely
    /// new token once the user is already at [`MAX_DEVICES_PER_USER`].
    pub async fn register_device(
        &self,
        user_id: &str,
        push_token: &str,
        platform: DevicePlatform,
        now: &str,
    ) -> DaoResult<()> {
        let already_registered = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Device(push_token.into()).to_string()))
            .projection_expression(ATTR_PK) // existence check only
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?
            .item
            .is_some();

        if !already_registered
            && self
                .device_count_at_least(user_id, MAX_DEVICES_PER_USER)
                .await?
        {
            return Err(DaoError::Conflict(format!(
                "user {user_id} already has the maximum of {MAX_DEVICES_PER_USER} registered devices"
            )));
        }

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
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Whether a user has at least `limit` registered devices. Uses
    /// `Select::Count` (no item bodies transferred) with `Limit` capping the
    /// read at exactly `limit`, so this is cheap regardless of how large the
    /// real collection is.
    async fn device_count_at_least(&self, user_id: &str, limit: u32) -> DaoResult<bool> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::Device(String::new()).prefix()))
            .select(Select::Count)
            .limit(limit as i32)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(out.count >= limit as i32)
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

    /// Unregister a push token. A no-op if the token isn't registered (already
    /// removed, or belonged to another user).
    pub async fn delete_device(&self, user_id: &str, push_token: &str) -> DaoResult<()> {
        self.client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Device(push_token.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}
