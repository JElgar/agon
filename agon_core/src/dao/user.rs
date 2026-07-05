//! User operations: create (with unique-email guard), get, update.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::UserRecord;

pub const TYPE_USER: &str = "user";
pub const TYPE_EMAIL_GUARD: &str = "email_guard";

impl Dao {
    /// Create a user profile, atomically reserving the (lowercased) email.
    ///
    /// Uses a transaction of two conditional puts: the profile item and an
    /// `EMAIL#<email>` guard item, each requiring `attribute_not_exists(PK)`. If
    /// the email is taken (or the id collides) the whole transaction fails and we
    /// return `Conflict`.
    pub async fn create_user(&self, user: &UserRecord) -> DaoResult<()> {
        let profile_item = to_item(&Pk::User(user.id.clone()), &Sk::Profile, TYPE_USER, user)?;

        // The guard item only needs to exist; it stores the owning user id so the
        // guard can be traced back / released on email change.
        let guard_pk = Pk::email_guard(&user.email);
        let mut guard_item = to_item(&guard_pk, &Sk::Guard, TYPE_EMAIL_GUARD, &())?;
        guard_item.insert("user_id".into(), s(user.id.clone()));

        let put_profile = Put::builder()
            .table_name(self.table())
            .set_item(Some(profile_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let put_guard = Put::builder()
            .table_name(self.table())
            .set_item(Some(guard_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_profile).build())
            .transact_items(TransactWriteItem::builder().put(put_guard).build())
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(DaoError::Conflict("email or user id already exists".into()))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a user profile by id. `None` if no such user.
    pub async fn get_user(&self, user_id: &str) -> DaoResult<Option<UserRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Profile.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Update a user's mutable profile fields (name, profile image). Email
    /// changes are handled separately (they must re-run the guard) and are not
    /// covered here. Fails with `NotFound` if the user doesn't exist.
    pub async fn update_user_profile(
        &self,
        user_id: &str,
        name: Option<&str>,
        profile_image_url: Option<Option<&str>>,
    ) -> DaoResult<()> {
        let mut set_parts: Vec<String> = Vec::new();
        let mut remove_parts: Vec<String> = Vec::new();
        let mut names: std::collections::HashMap<String, String> = Default::default();
        let mut values: std::collections::HashMap<String, AttributeValue> = Default::default();

        if let Some(name) = name {
            set_parts.push("#name = :name".into());
            names.insert("#name".into(), "name".into());
            values.insert(":name".into(), s(name));
        }
        if let Some(image) = profile_image_url {
            match image {
                Some(url) => {
                    set_parts.push("#img = :img".into());
                    names.insert("#img".into(), "profile_image_url".into());
                    values.insert(":img".into(), s(url));
                }
                // Explicit clear.
                None => {
                    remove_parts.push("#img".into());
                    names.insert("#img".into(), "profile_image_url".into());
                }
            }
        }

        if set_parts.is_empty() && remove_parts.is_empty() {
            return Ok(()); // nothing to change
        }

        let mut expr = String::new();
        if !set_parts.is_empty() {
            expr.push_str("SET ");
            expr.push_str(&set_parts.join(", "));
        }
        if !remove_parts.is_empty() {
            if !expr.is_empty() {
                expr.push(' ');
            }
            expr.push_str("REMOVE ");
            expr.push_str(&remove_parts.join(", "));
        }

        // Guard existence via the PK, which every profile item has.
        names.insert("#pk".into(), ATTR_PK.into());

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Profile.to_string()))
            .update_expression(expr)
            .condition_expression("attribute_exists(#pk)")
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(if values.is_empty() {
                None
            } else {
                Some(values)
            })
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("user {user_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

/// True if an `UpdateItem` failed its condition expression.
fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}
