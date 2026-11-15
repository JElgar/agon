//! Manage-paired-devices: list + revoke.
//!
//! `PairedDeviceRecord` (`USER#<uid>` / `PAIREDDEV#<device_sub>`) is written
//! once, in `device_pairing::claim_device_pairing_code`'s own transaction,
//! the moment a pairing is actually claimed — this module never creates one
//! itself, only reads and deletes. It exists purely for the account owner's
//! benefit (see it, revoke it); the real credential is the
//! `AUTH#<device_sub>` guard (`Pk::AuthGuard`), which `revoke_paired_device`
//! deletes in the same transaction as the record itself, so a revoked
//! device's token stops resolving immediately — no separate blocklist
//! needed, same as how every other `AUTH#<sub>` guard in this table works.

use aws_sdk_dynamodb::types::{Delete, TransactWriteItem};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, s};
use super::keys::{Pk, Sk};
use super::records::PairedDeviceRecord;

pub const TYPE_PAIRED_DEVICE: &str = "paired_device";

/// Generous cap on a single `list_paired_devices` page — unlike push-token
/// registration (`MAX_DEVICES_PER_USER`), nothing enforces this at write
/// time today: pairing a device takes a fresh QR scan/code and an
/// already-logged-in confirmation each time, which is enough friction that
/// a real account pairing anywhere near this many is implausible. Revisit
/// (an enforced cap + pagination) if that assumption ever stops holding.
const MAX_PAIRED_DEVICES_PER_USER: u32 = 50;

impl Dao {
    /// List every device paired to a user's account. Unpaginated — see
    /// [`MAX_PAIRED_DEVICES_PER_USER`]'s doc comment on why one page is
    /// expected to always be enough today.
    pub async fn list_paired_devices(&self, user_id: &str) -> DaoResult<Vec<PairedDeviceRecord>> {
        let page = self
            .query_page(
                self.client
                    .query()
                    .table_name(self.table())
                    .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                    .expression_attribute_names("#pk", ATTR_PK)
                    .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
                    .expression_attribute_values(":sk", s(Sk::paired_device_prefix())),
                None,
                MAX_PAIRED_DEVICES_PER_USER,
            )
            .await?;
        Ok(page.items)
    }

    /// Revoke a paired device: deletes its `PairedDeviceRecord` and its
    /// `AUTH#<device_sub>` guard atomically, so an already-minted device
    /// token stops resolving to a user the instant this returns. `NotFound`
    /// if this exact device isn't paired to this user (already revoked,
    /// never was, or belongs to someone else) — deliberately not
    /// idempotent, same reasoning as `delete_device` (push tokens): a
    /// revoke that silently no-ops on the wrong id is worth surfacing, not
    /// swallowing.
    pub async fn revoke_paired_device(&self, user_id: &str, device_sub: &str) -> DaoResult<()> {
        let delete_record = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::PairedDevice(device_sub.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // Unconditional — this only ever runs alongside `delete_record`
        // above in the same transaction, and that one's own condition is
        // what actually confirms this device belongs to `user_id` (the
        // record only exists under *this* user's partition if it does).
        // A stale/forged `device_sub` that isn't paired to this user simply
        // fails `delete_record`'s condition and rolls the whole transaction
        // back, so this never fires without that check having passed.
        let delete_guard = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::AuthGuard(device_sub.into()).to_string()))
            .key("SK", s(Sk::Guard.to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete_record).build())
            .transact_items(TransactWriteItem::builder().delete(delete_guard).build())
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::NotFound(
                format!("paired device {device_sub} for user {user_id}"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}
