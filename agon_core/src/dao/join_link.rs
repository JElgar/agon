//! Join-link operations: create, get (by id / by token), list (by match),
//! revoke. Standalone entities, same shape as `invitation.rs` — see
//! `JoinLinkRecord`'s doc comment for why. Projects to GSI1 for the
//! list-by-match query and to GSI2 for token lookup (a distinct
//! `JOINLINK_TOKEN#` prefix from `InvitationRecord`'s `TOKEN#`, so the two
//! entities never collide despite sharing GSI2).

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_GSI2PK, ATTR_PK, ItemBuilder, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{InvitationContextRecord, JoinLinkRecord};

pub const TYPE_JOIN_LINK: &str = "join_link";

impl Dao {
    /// Build the item map for a join link, applying the GSI projections:
    /// GSI1 (`MATCHJOINLINKS#<matchId>` / `<created_at>#<id>`) for a match-join
    /// link, so a match's links can be listed newest-first; GSI2
    /// (`JOINLINK_TOKEN#<token>`) always, for token lookup.
    fn join_link_item(&self, link: &JoinLinkRecord) -> DaoResult<super::item::Item> {
        let base = to_item(&Pk::JoinLink(link.id.clone()), &Sk::Meta, TYPE_JOIN_LINK, link)?;

        let mut builder = ItemBuilder::new(base);
        match &link.context {
            InvitationContextRecord::Match { match_id, .. } => {
                builder = builder.gsi1(
                    format!("MATCHJOINLINKS#{match_id}"),
                    format!("{}#{}", link.created_at, link.id),
                );
            }
            InvitationContextRecord::Team { .. } => {
                // No list-by-team query yet — a team join-link feature would
                // add its own GSI1 projection here, mirroring the match case.
            }
        }
        builder = builder.gsi2(format!("JOINLINK_TOKEN#{}", link.token), "#".to_string());
        Ok(builder.build())
    }

    /// Create a join link. `Conflict` if the link id already exists.
    #[tracing::instrument(skip(self, link), fields(join_link_id = %link.id))]
    pub async fn create_join_link(&self, link: &JoinLinkRecord) -> DaoResult<()> {
        let item = self.join_link_item(link)?;

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
                "join link {} already exists",
                link.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a join link by id. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_join_link(&self, join_link_id: &str) -> DaoResult<Option<JoinLinkRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::JoinLink(join_link_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Look up a join link by its bearer token, via GSI2. `None` if no match.
    #[tracing::instrument(skip(self))]
    pub async fn get_join_link_by_token(&self, token: &str) -> DaoResult<Option<JoinLinkRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .index_name("GSI2")
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_GSI2PK)
            .expression_attribute_values(":pk", s(format!("JOINLINK_TOKEN#{token}")))
            .limit(1)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        match out.items.and_then(|mut items| items.pop()) {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// List a match's join links, newest first, via GSI1. Cursor-paginated.
    #[tracing::instrument(skip(self))]
    pub async fn list_match_join_links(
        &self,
        match_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<JoinLinkRecord>> {
        let query = self
            .client
            .query()
            .table_name(self.table())
            .index_name("GSI1")
            .scan_index_forward(false)
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_GSI1PK)
            .expression_attribute_values(":pk", s(format!("MATCHJOINLINKS#{match_id}")));

        self.query_page(query, cursor, limit).await
    }

    /// Revoke a join link (soft — sets `revoked_at`, doesn't delete, so past
    /// joiners' `joined_via` provenance stays resolvable). `NotFound` if the
    /// link doesn't exist.
    #[tracing::instrument(skip(self))]
    pub async fn revoke_join_link(&self, join_link_id: &str, revoked_at: &str) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::JoinLink(join_link_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .update_expression("SET revoked_at = :revoked_at")
            .expression_attribute_values(":revoked_at", s(revoked_at))
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("join link {join_link_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
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
