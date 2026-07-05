//! Invitation operations: create, get (by id / by token), inbox, respond,
//! revoke. Invitations are standalone entities projected into GSI1 (inbox) and
//! GSI2 (token lookup).

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_GSI2PK, ATTR_PK, ItemBuilder, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::InvitationRecord;

pub const TYPE_INVITATION: &str = "invitation";

impl Dao {
    /// Create an invitation. Projects to GSI1 (`UINV#<inviteeUserId>`) if it
    /// targets a known user, and to GSI2 (`TOKEN#<token>`) if it has a token.
    /// `Conflict` if the invitation id already exists.
    pub async fn create_invitation(&self, inv: &InvitationRecord) -> DaoResult<()> {
        let base = to_item(
            &Pk::Invitation(inv.id.clone()),
            &Sk::Meta,
            TYPE_INVITATION,
            inv,
        )?;

        let mut builder = ItemBuilder::new(base);
        if let Some(uid) = &inv.invited_user_id {
            // Inbox: sort by status then time so a client can filter by status.
            builder = builder.gsi1(
                format!("UINV#{uid}"),
                format!("{}#{}", inv.status, inv.invited_at),
            );
        }
        if let Some(token) = &inv.invite_token {
            builder = builder.gsi2(format!("TOKEN#{token}"), "#".to_string());
        }
        let item = builder.build();

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
                "invitation {} already exists",
                inv.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch an invitation by id. `None` if absent.
    pub async fn get_invitation(&self, invitation_id: &str) -> DaoResult<Option<InvitationRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Invitation(invitation_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Look up an invitation by its bearer token, via GSI2. `None` if no match.
    pub async fn get_invitation_by_token(
        &self,
        token: &str,
    ) -> DaoResult<Option<InvitationRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .index_name("GSI2")
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_GSI2PK)
            .expression_attribute_values(":pk", s(format!("TOKEN#{token}")))
            .limit(1)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        match out.items.and_then(|mut items| items.pop()) {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// List a user's invitation inbox via GSI1, optionally filtered to one
    /// status. Cursor-paginated.
    pub async fn list_user_invitations(
        &self,
        user_id: &str,
        status: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<InvitationRecord>> {
        let mut query = self
            .client
            .query()
            .table_name(self.table())
            .index_name("GSI1")
            .expression_attribute_names("#pk", ATTR_GSI1PK)
            .expression_attribute_values(":pk", s(format!("UINV#{user_id}")));

        query = match status {
            // GSI1SK is `<status>#<invited_at>`, so a status filter is a prefix.
            Some(st) => query
                .key_condition_expression("#pk = :pk AND begins_with(GSI1SK, :sk)")
                .expression_attribute_values(":sk", s(format!("{st}#"))),
            None => query.key_condition_expression("#pk = :pk"),
        };

        self.query_page(query, cursor, limit).await
    }

    /// Record a response to an invitation (accept/decline): set status +
    /// `responded_at`, and keep the GSI1 inbox sort key (`<status>#<invited_at>`)
    /// consistent with the new status. `NotFound` if the invitation is absent.
    ///
    /// Note: this updates only the invitation entity. Side effects of
    /// acceptance (linking an external member to a user, adding to a roster,
    /// notifications, feed) are orchestrated by the caller / async workers.
    pub async fn respond_to_invitation(
        &self,
        invitation_id: &str,
        status: &str,
        responded_at: &str,
        invited_at: &str,
        has_user_inbox: bool,
    ) -> DaoResult<()> {
        let mut update = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Invitation(invitation_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":status", s(status))
            .expression_attribute_values(":responded_at", s(responded_at));

        // Keep the inbox GSI1 sort key aligned with the new status so the
        // status-filtered inbox query stays correct after a response.
        if has_user_inbox {
            update = update
                .update_expression(
                    "SET #status = :status, responded_at = :responded_at, GSI1SK = :gsi1sk",
                )
                .expression_attribute_values(":gsi1sk", s(format!("{status}#{invited_at}")));
        } else {
            update =
                update.update_expression("SET #status = :status, responded_at = :responded_at");
        }

        match update.send().await {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("invitation {invitation_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Delete (revoke) an invitation. Idempotent.
    pub async fn delete_invitation(&self, invitation_id: &str) -> DaoResult<()> {
        self.client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Invitation(invitation_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}

fn is_put_conditional_failure(
    err: &SdkError<aws_sdk_dynamodb::operation::put_item::PutItemError>,
) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(
                se.err(),
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(_)
            )
    )
}

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}
