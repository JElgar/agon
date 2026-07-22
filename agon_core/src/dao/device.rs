//! Device (push-token) registration: register, list, unregister.
//!
//! Devices live under the user partition (`USER#<uid>` / `DEVICE#<token>`).
//! Unlike most collections here there's no uniqueness guard or counter to
//! maintain — the token itself is the key, so re-registering the same token
//! (e.g. on every app open) is a plain overwrite, and unregistering a token
//! that's already gone is a harmless no-op. Device counts per user are small,
//! so `list_devices` returns everything in one `Query` rather than paginating.

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{DevicePlatform, DeviceRecord};

pub const TYPE_DEVICE: &str = "device";

impl Dao {
    /// Register (or re-register) a push token for a user. Idempotent: writing
    /// the same token again just refreshes its fields.
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
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// List every device registered for a user (unpaginated — a user's device
    /// count is small).
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
                50,
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
