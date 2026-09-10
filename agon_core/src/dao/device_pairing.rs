//! Device pairing: links a device that has no practical way to log in
//! directly (typed credentials, an OAuth browser flow — a Garmin watch,
//! initially) to a user's account, without the device ever handling the
//! user's real credentials.
//!
//! Flow: the already-authenticated client (phone/web) mints a short-lived,
//! single-use code (`create_device_pairing_code`) and shows it to the user;
//! the device submits that code once (`claim_device_pairing_code`), which
//! atomically marks the code used and reserves the device's own auth
//! identity — an `AUTH#<device_sub>` guard mapping straight to the code's
//! owning user id, exactly like a real login provider's `sub` (see
//! `keys::Pk::AuthGuard`'s doc comment). Every existing
//! `require_uid`/`get_user_id_by_sub` path resolves a device token with no
//! changes: the device is simply another identity on the same account. The
//! service layer (`agon_service::auth::DeviceTokenSigner`) then mints a
//! long-lived JWT for that `device_sub`.
//!
//! There is no DynamoDB TTL on the pairing-code item (the table has none
//! configured — see `docs/dynamodb-design.md`), so an expired, never-claimed
//! code is simply left behind as a small orphaned item rather than actually
//! removed. Fine for a prototype; a real deployment would want a scheduled
//! sweep (or a TTL attribute) to clean these up.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::{Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{AuthGuardRecord, DevicePairingRecord};
use super::user::TYPE_AUTH_GUARD;

pub const TYPE_DEVICE_PAIRING: &str = "device_pairing";

impl Dao {
    /// Create a pairing code. `Conflict` if the (randomly generated) code
    /// already exists — the caller should retry with a fresh code, same
    /// pattern as any other id-collision guard in this DAO.
    #[tracing::instrument(skip(self, pairing), fields(code = %pairing.code))]
    pub async fn create_device_pairing_code(&self, pairing: &DevicePairingRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::DevicePairing(pairing.code.clone()),
            &Sk::Meta,
            TYPE_DEVICE_PAIRING,
            pairing,
        )?;

        let result = self
            .client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(format!(
                "pairing code {} already exists",
                pairing.code
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a pairing code by its value. `None` if it doesn't exist.
    #[tracing::instrument(skip(self))]
    pub async fn get_device_pairing_code(
        &self,
        code: &str,
    ) -> DaoResult<Option<DevicePairingRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::DevicePairing(code.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Claim a pairing code: atomically marks it claimed and creates the
    /// `AUTH#<device_sub>` guard mapping its reserved device identity to the
    /// code's owning user. Returns the claimed record (notably its
    /// `device_sub`, for the caller to mint a device token) on success.
    ///
    /// `NotFound` if the code doesn't exist; `Conflict` if it's already been
    /// claimed or has expired (that first check, plus the up-front read
    /// itself, can be stale under a race — harmless, because the
    /// transaction's own `attribute_not_exists(claimed_at)` condition is
    /// what actually decides the outcome: two concurrent claims can both
    /// pass this read, but only one wins the transaction, and the loser gets
    /// the same `Conflict` it would have from a fresh read).
    #[tracing::instrument(skip(self))]
    pub async fn claim_device_pairing_code(
        &self,
        code: &str,
        now: &str,
    ) -> DaoResult<DevicePairingRecord> {
        let pairing = self
            .get_device_pairing_code(code)
            .await?
            .ok_or_else(|| DaoError::NotFound(format!("pairing code {code}")))?;

        if pairing.claimed_at.is_some() {
            return Err(DaoError::Conflict("pairing code already used".into()));
        }
        if pairing.expires_at.as_str() < now {
            return Err(DaoError::Conflict("pairing code expired".into()));
        }

        let claim_code = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::DevicePairing(code.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .update_expression("SET claimed_at = :now")
            .condition_expression(
                "attribute_exists(#pk) AND attribute_not_exists(claimed_at) AND expires_at > :now",
            )
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":now", s(now))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let auth_guard_item = to_item(
            &Pk::AuthGuard(pairing.device_sub.clone()),
            &Sk::Guard,
            TYPE_AUTH_GUARD,
            &AuthGuardRecord {
                user_id: pairing.user_id.clone(),
            },
        )?;
        let create_auth_guard = Put::builder()
            .table_name(self.table())
            .set_item(Some(auth_guard_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(claim_code).build())
            .transact_items(TransactWriteItem::builder().put(create_auth_guard).build())
            .send()
            .await;

        match result {
            Ok(_) => Ok(pairing),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                "pairing code already used or expired".into(),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), PutItemError::ConditionalCheckFailedException(_))
    )
}
