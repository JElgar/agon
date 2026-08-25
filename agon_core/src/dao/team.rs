//! Team operations: create, get (meta + members aggregate), update, member
//! add/remove, and "my teams".

use std::collections::HashMap;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::delete_item::DeleteItemError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_PK, ATTR_SK, ItemBuilder, from_item, item_sk, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{TeamMemberRecord, TeamRecord};

pub const TYPE_TEAM: &str = "team";
pub const TYPE_TEAM_MEMBER: &str = "team_member";

/// A team plus its members, assembled from one item-collection query.
#[derive(Debug)]
pub struct TeamAggregate {
    pub team: TeamRecord,
    pub members: Vec<TeamMemberRecord>,
}

impl Dao {
    /// Create a team and its creator's membership in one transaction. The
    /// creator becomes an `admin` member. `Conflict` if the team id already
    /// exists.
    #[tracing::instrument(skip(self, team, creator), fields(team_id = %team.id))]
    pub async fn create_team(
        &self,
        team: &TeamRecord,
        creator: &TeamMemberRecord,
    ) -> DaoResult<()> {
        use aws_sdk_dynamodb::types::{Put, TransactWriteItem};

        let meta_item = to_item(&Pk::Team(team.id.clone()), &Sk::Meta, TYPE_TEAM, team)?;
        let member_item = self.team_member_item(&team.id, creator)?;

        let put_meta = Put::builder()
            .table_name(self.table())
            .set_item(Some(meta_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let put_member = Put::builder()
            .table_name(self.table())
            .set_item(Some(member_item))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_meta).build())
            .transact_items(TransactWriteItem::builder().put(put_member).build())
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("team {} already exists", team.id),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a team's meta only (no members).
    #[tracing::instrument(skip(self))]
    pub async fn get_team_meta(&self, team_id: &str) -> DaoResult<Option<TeamRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Team(team_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Fetch multiple teams' meta (no members) in one batch read, keyed by
    /// team id. An id with no team simply has no entry in the result — mirrors
    /// `batch_get_users`. Used to resolve match-side team names for a whole
    /// feed/list page in one round trip instead of one `get_team_meta` per
    /// side.
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_team_metas(
        &self,
        team_ids: &[String],
    ) -> DaoResult<HashMap<String, TeamRecord>> {
        // De-dup — BatchGetItem rejects duplicate keys in one request.
        let mut seen = std::collections::HashSet::new();
        let keys: Vec<_> = team_ids
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .map(|id| {
                HashMap::from([
                    (ATTR_PK.to_string(), s(Pk::Team(id.clone()).to_string())),
                    (ATTR_SK.to_string(), s(Sk::Meta.to_string())),
                ])
            })
            .collect();

        let items = self.batch_get_all(keys, None).await?;
        let mut out = HashMap::with_capacity(items.len());
        for item in items {
            let record: TeamRecord = from_item(item)?;
            out.insert(record.id.clone(), record);
        }
        Ok(out)
    }

    /// Fetch the full team aggregate (meta + all members) in a single query on
    /// the `TEAM#<id>` partition, splitting the collection by SK prefix.
    /// `None` if the team's meta item is absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_team(&self, team_id: &str) -> DaoResult<Option<TeamAggregate>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Team(team_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut team: Option<TeamRecord> = None;
        let mut members: Vec<TeamMemberRecord> = Vec::new();

        for item in out.items.unwrap_or_default() {
            match item_sk(&item)? {
                Sk::Meta => team = Some(from_item(item)?),
                Sk::Member(_) => members.push(from_item(item)?),
                // Follower edges also live here; ignore for the aggregate.
                _ => {}
            }
        }

        Ok(team.map(|team| TeamAggregate { team, members }))
    }

    /// Update a team's mutable fields — `name` and/or `logo_url`.
    /// Both optional and independent, like `Dao::update_user_profile`:
    /// `logo_url` is a double `Option` — outer `None` leaves the logo
    /// untouched, `Some(None)` clears it, `Some(Some(url))` sets it.
    /// A no-op call (both `None`) skips the write entirely. `NotFound` if the
    /// team doesn't exist.
    #[tracing::instrument(skip(self))]
    pub async fn update_team(
        &self,
        team_id: &str,
        name: Option<&str>,
        logo_url: Option<Option<&str>>,
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
        if let Some(logo) = logo_url {
            match logo {
                Some(url) => {
                    set_parts.push("#logo = :logo".into());
                    names.insert("#logo".into(), "logo_url".into());
                    values.insert(":logo".into(), s(url));
                }
                // Explicit clear.
                None => {
                    remove_parts.push("#logo".into());
                    names.insert("#logo".into(), "logo_url".into());
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

        // Guard existence via the PK, which every team meta item has.
        names.insert("#pk".into(), ATTR_PK.into());

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Team(team_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
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
                Err(DaoError::NotFound(format!("team {team_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Add (or overwrite) a single team member. Used for both single adds and
    /// the fan-out of a bulk invite (call per member).
    #[tracing::instrument(skip(self, member), fields(membership_id = %member.membership_id))]
    pub async fn put_team_member(&self, team_id: &str, member: &TeamMemberRecord) -> DaoResult<()> {
        let item = self.team_member_item(team_id, member)?;
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Remove a member from a team by membership id. Delete-by-id, not a
    /// toggle, so errors with `DaoError::NotFound` if the membership doesn't
    /// exist rather than silently succeeding.
    #[tracing::instrument(skip(self))]
    pub async fn remove_team_member(&self, team_id: &str, membership_id: &str) -> DaoResult<()> {
        let result = self
            .client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Team(team_id.into()).to_string()))
            .key("SK", s(Sk::Member(membership_id.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_delete_conditional_failure(&e) => Err(DaoError::NotFound(format!(
                "membership {membership_id} on team {team_id}"
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List the teams a user is a member of, via GSI1 (`UTEAMS#<userId>`).
    #[tracing::instrument(skip(self))]
    pub async fn list_user_teams(
        &self,
        user_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<TeamMemberRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("UTEAMS#{user_id}"))),
            cursor,
            limit,
        )
        .await
    }

    /// Build a team-member item, projecting members with a linked user into GSI1
    /// (`UTEAMS#<userId>`) so the user can list their teams. External-only
    /// members (no `user_id`) are not projected — they don't have a "my teams".
    pub(super) fn team_member_item(
        &self,
        team_id: &str,
        member: &TeamMemberRecord,
    ) -> DaoResult<std::collections::HashMap<String, AttributeValue>> {
        let base = to_item(
            &Pk::Team(team_id.into()),
            &Sk::Member(member.membership_id.clone()),
            TYPE_TEAM_MEMBER,
            member,
        )?;
        let item = match &member.user_id {
            Some(uid) => ItemBuilder::new(base)
                .gsi1(
                    format!("UTEAMS#{uid}"),
                    Pk::Team(team_id.into()).to_string(),
                )
                .build(),
            None => base,
        };
        Ok(item)
    }
}

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}

fn is_delete_conditional_failure(err: &SdkError<DeleteItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), DeleteItemError::ConditionalCheckFailedException(_))
    )
}
