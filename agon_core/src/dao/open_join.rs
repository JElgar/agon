//! Open joining ("ringers"): a match's opt-in public-join policy
//! (`OpenJoinPolicyRecord`), per-side ringer settings, the cap-guarded
//! open-join write itself, and the optional approval queue
//! (`MatchJoinRequestRecord`) for a policy that requires one. See the design
//! doc for the full picture; this module covers only the DAO-level pieces:
//! eligibility (who *may* join) is evaluated by the caller (`agon_service`)
//! against a fresh read, never stored or baked into a query here.

use std::collections::HashMap;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{MatchJoinRequestRecord, MatchPlayerRecord, OpenJoinPolicyRecord};

pub const TYPE_MATCH_JOIN_REQUEST: &str = "match_join_request";

impl Dao {
    /// Set or clear a match's open-join policy (`MatchRecord::open_join`).
    /// `None` closes the match back to invite/link/team-only. `NotFound` if
    /// the match is absent.
    #[tracing::instrument(skip(self, policy))]
    pub async fn set_match_open_join_policy(
        &self,
        match_id: &str,
        policy: Option<&OpenJoinPolicyRecord>,
    ) -> DaoResult<()> {
        let mut request = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK);
        request = match policy {
            Some(p) => request
                .update_expression("SET open_join = :p")
                .expression_attribute_values(":p", to_attr(p)?),
            None => request.update_expression("REMOVE open_join"),
        };
        let result = request.send().await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("match {match_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Set one or more sides' ringer settings: `(side_id, ringers_enabled,
    /// max_ringer_spots)`. Kept separate from `update_match_join_settings`
    /// (team self-join's own settings call) the same way that call is
    /// separate from `update_match_meta` — a distinct concern, no atomicity
    /// lost by it being its own call. `NotFound` if the match is absent. A
    /// no-op for an empty slice.
    #[tracing::instrument(skip(self))]
    pub async fn update_side_ringer_settings(
        &self,
        match_id: &str,
        side_settings: &[(String, bool, Option<u32>)],
    ) -> DaoResult<()> {
        if side_settings.is_empty() {
            return Ok(());
        }

        let mut set: Vec<String> = Vec::new();
        let mut remove: Vec<String> = Vec::new();
        let mut names: HashMap<String, String> = HashMap::new();
        let mut values: HashMap<String, AttributeValue> = HashMap::new();
        names.insert("#pk".into(), ATTR_PK.into());

        for (i, (side_id, ringers_enabled, max_ringer_spots)) in side_settings.iter().enumerate() {
            let side_alias = format!("#s{i}");
            names.insert(side_alias.clone(), side_id.clone());

            let enabled_alias = format!(":re{i}");
            set.push(format!(
                "sides.{side_alias}.ringers_enabled = {enabled_alias}"
            ));
            values.insert(enabled_alias, AttributeValue::Bool(*ringers_enabled));

            match max_ringer_spots {
                Some(max) => {
                    let value_alias = format!(":mr{i}");
                    set.push(format!(
                        "sides.{side_alias}.max_ringer_spots = {value_alias}"
                    ));
                    values.insert(value_alias, AttributeValue::N(max.to_string()));
                }
                None => {
                    remove.push(format!("sides.{side_alias}.max_ringer_spots"));
                }
            }
        }

        let mut expr = String::new();
        if !set.is_empty() {
            expr.push_str("SET ");
            expr.push_str(&set.join(", "));
        }
        if !remove.is_empty() {
            if !expr.is_empty() {
                expr.push(' ');
            }
            expr.push_str("REMOVE ");
            expr.push_str(&remove.join(", "));
        }

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression(expr)
            .condition_expression("attribute_exists(#pk)")
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(Some(values))
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("match {match_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Join a match through its open-join policy directly — no approval
    /// step. The direct analogue of `Dao::join_match_tx`, but guarded by the
    /// open/ringer caps (`max_open_spots`/`max_ringer_spots`) *in addition
    /// to* the ordinary side/match caps, via `take_open_slot_update` rather
    /// than `match_ops::take_slot_update`. `player.joined_via` must already
    /// be `Some(JoinSourceRecord::Open)` — set by the caller, not asserted
    /// here (mirrors `join_match_tx`'s own contract).
    ///
    /// `Conflict` if any cap has been reached since the caller last read it.
    #[tracing::instrument(skip(self, player), fields(player_id = %player.player_id))]
    pub async fn join_open_match_tx(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        side_max_players: Option<u32>,
        total_max_players: Option<u32>,
        max_ringer_spots: Option<u32>,
        max_open_spots: Option<u32>,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<()> {
        let items = self.join_open_match_items(
            match_id,
            player,
            side_max_players,
            total_max_players,
            max_ringer_spots,
            max_open_spots,
            starts_at,
            now,
        )?;
        self.send_transact_items(items).await
    }

    /// Build the writes `join_open_match_tx` sends, and the writes
    /// `approve_join_request_tx` sends alongside deciding the request —
    /// shared the same way `match_ops::join_match_items` is shared with
    /// `Dao::move_in_from_waitlist_tx`.
    fn join_open_match_items(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        side_max_players: Option<u32>,
        total_max_players: Option<u32>,
        max_ringer_spots: Option<u32>,
        max_open_spots: Option<u32>,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<Vec<TransactWriteItem>> {
        let put_player = Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let update_meta = take_open_slot_update(
            self.table(),
            match_id,
            player.side_id.as_deref(),
            side_max_players,
            total_max_players,
            max_ringer_spots,
            max_open_spots,
        )?;

        let mut items = vec![
            TransactWriteItem::builder().put(put_player).build(),
            TransactWriteItem::builder().update(update_meta).build(),
        ];

        if let Some(uid) = &player.user_id {
            let feed_item = self.feed_item(
                uid,
                match_id,
                starts_at,
                now,
                &AudienceMember {
                    viewer_side_id: player.side_id.clone(),
                    viewer_is_going: true,
                    ..Default::default()
                },
            )?;
            let feed_put = Put::builder()
                .table_name(self.table())
                .set_item(Some(feed_item))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            items.push(TransactWriteItem::builder().put(feed_put).build());
        }

        Ok(items)
    }

    /// Fetch one user's join request for a match. `None` if they have none
    /// (never requested, or a plain self-serve/link/team join instead).
    #[tracing::instrument(skip(self))]
    pub async fn get_join_request(
        &self,
        match_id: &str,
        user_id: &str,
    ) -> DaoResult<Option<MatchJoinRequestRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::JoinRequest(user_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// List every join request on a match, pending and decided alike,
    /// oldest-requested first — the service splits "pending" from "recently
    /// decided" for display (see `JoinRequestStatusRecord`). Draining query,
    /// same as `list_waitlist`.
    #[tracing::instrument(skip(self))]
    pub async fn list_join_requests(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<MatchJoinRequestRecord>> {
        let mut entries = self
            .query_match_collection::<MatchJoinRequestRecord>(match_id, &Sk::join_request_prefix())
            .await?;
        entries.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        Ok(entries)
    }

    /// Request to join a match through its open-join policy — used when
    /// `OpenJoinPolicyRecord::requires_approval` is `true`; otherwise the
    /// caller should join directly via `join_open_match_tx`. A plain
    /// conditional put guarded on the caller not already having a request
    /// at this key, whatever its status — the same uniqueness-guard
    /// convention as `waitlist_join_tx`, and what stops a declined
    /// candidate from simply retrying.
    ///
    /// `Conflict` if the caller already has a request (pending or decided)
    /// for this match.
    #[tracing::instrument(skip(self, request), fields(user_id = %request.user_id))]
    pub async fn create_join_request_tx(
        &self,
        match_id: &str,
        request: &MatchJoinRequestRecord,
    ) -> DaoResult<()> {
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::JoinRequest(request.user_id.clone()),
            TYPE_MATCH_JOIN_REQUEST,
            request,
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
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(
                "you've already requested to join this match".into(),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Approve a pending join request: mark it `Approved` and seat `player`,
    /// atomically — the request transitioning and the roster gaining the
    /// player either both happen or neither does, so a failure partway never
    /// leaves an approved request with nobody on the roster (or a seated
    /// player with a request still showing "pending"). Shares
    /// `join_open_match_items` with the direct (non-approval) join path, so
    /// the two can never disagree about what counts as a free spot.
    ///
    /// `player.user_id` must equal the request's `user_id` — the caller's
    /// responsibility, not re-checked here.
    ///
    /// `Conflict` if the request is no longer pending (already decided, or
    /// never existed) or a cap has been reached since the caller's read.
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(self, player), fields(player_id = %player.player_id))]
    pub async fn approve_join_request_tx(
        &self,
        match_id: &str,
        user_id: &str,
        decided_by_user_id: &str,
        decided_at: &str,
        player: &MatchPlayerRecord,
        side_max_players: Option<u32>,
        total_max_players: Option<u32>,
        max_ringer_spots: Option<u32>,
        max_open_spots: Option<u32>,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<()> {
        let update_request = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::JoinRequest(user_id.into()).to_string()))
            .condition_expression("#status = :pending")
            .update_expression(
                "SET #status = :approved, decided_at = :at, decided_by_user_id = :by",
            )
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":pending", s("pending"))
            .expression_attribute_values(":approved", s("approved"))
            .expression_attribute_values(":at", s(decided_at))
            .expression_attribute_values(":by", s(decided_by_user_id))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut items = vec![TransactWriteItem::builder().update(update_request).build()];
        items.extend(self.join_open_match_items(
            match_id,
            player,
            side_max_players,
            total_max_players,
            max_ringer_spots,
            max_open_spots,
            starts_at,
            now,
        )?);

        self.send_transact_items(items).await
    }

    /// Decline a pending join request: a soft status update, same convention
    /// as `revoke_join_link`. `Conflict` if the request is no longer pending
    /// (already decided, or never existed).
    #[tracing::instrument(skip(self))]
    pub async fn decline_join_request(
        &self,
        match_id: &str,
        user_id: &str,
        decided_by_user_id: &str,
        decided_at: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::JoinRequest(user_id.into()).to_string()))
            .condition_expression("#status = :pending")
            .update_expression(
                "SET #status = :declined, decided_at = :at, decided_by_user_id = :by",
            )
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":pending", s("pending"))
            .expression_attribute_values(":declined", s("declined"))
            .expression_attribute_values(":at", s(decided_at))
            .expression_attribute_values(":by", s(decided_by_user_id))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Err(DaoError::Conflict(format!(
                "join request from {user_id} is no longer pending"
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

/// Serialize a record value into a DynamoDB `AttributeValue` (nested map).
fn to_attr<T: serde::Serialize>(value: &T) -> DaoResult<AttributeValue> {
    Ok(serde_dynamo::to_attribute_value(value)?)
}

/// The cap-guarded `ADD` for a write that takes an open/ringer spot:
/// `total_player_count` + `open_joined_count`, plus
/// `sides.<side_id>.player_count` + `sides.<side_id>.ringers_joined_count`
/// when `side_id` is given — conditioned on the match existing and, for
/// whichever cap is `Some`, there still being room under it: the ordinary
/// side/match caps (`side_max_players`/`total_max_players`, same meaning as
/// `match_ops::take_slot_update`'s) *and* the open-specific ones
/// (`max_ringer_spots`/`max_open_spots`). A separate builder rather than an
/// extension of `take_slot_update` — DynamoDB allows only one `Update`
/// transact item per key, so an open/ringer join's single `Update` has to
/// express every cap it must respect in one `ADD`/one `ConditionExpression`,
/// not two `Update`s layered on top of each other.
fn take_open_slot_update(
    table: &str,
    match_id: &str,
    side_id: Option<&str>,
    side_max_players: Option<u32>,
    total_max_players: Option<u32>,
    max_ringer_spots: Option<u32>,
    max_open_spots: Option<u32>,
) -> DaoResult<Update> {
    let mut names: HashMap<String, String> = HashMap::new();
    let mut values: HashMap<String, AttributeValue> = HashMap::new();
    names.insert("#pk".into(), ATTR_PK.into());
    values.insert(":one".into(), AttributeValue::N("1".into()));

    let mut add_clauses = vec![
        "total_player_count :one".to_string(),
        "open_joined_count :one".to_string(),
    ];
    let mut conditions: Vec<String> = vec!["attribute_exists(#pk)".into()];
    if let Some(max) = total_max_players {
        conditions.push(
            "(attribute_not_exists(total_player_count) OR total_player_count < :totalmax)".into(),
        );
        values.insert(":totalmax".into(), AttributeValue::N(max.to_string()));
    }
    if let Some(max) = max_open_spots {
        conditions.push(
            "(attribute_not_exists(open_joined_count) OR open_joined_count < :openmax)".into(),
        );
        values.insert(":openmax".into(), AttributeValue::N(max.to_string()));
    }
    if let Some(side_id) = side_id {
        names.insert("#sid".into(), side_id.to_string());
        add_clauses.push("sides.#sid.player_count :one".to_string());
        add_clauses.push("sides.#sid.ringers_joined_count :one".to_string());
        if let Some(max) = side_max_players {
            conditions.push(
                "(attribute_not_exists(sides.#sid.max_players) OR sides.#sid.player_count < :sidemax)"
                    .into(),
            );
            values.insert(":sidemax".into(), AttributeValue::N(max.to_string()));
        }
        if let Some(max) = max_ringer_spots {
            conditions.push(
                "(attribute_not_exists(sides.#sid.ringers_joined_count) OR sides.#sid.ringers_joined_count < :ringermax)"
                    .into(),
            );
            values.insert(":ringermax".into(), AttributeValue::N(max.to_string()));
        }
    }

    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
        .key(ATTR_SK, s(Sk::Meta.to_string()))
        .update_expression(format!("ADD {}", add_clauses.join(", ")))
        .condition_expression(conditions.join(" AND "))
        .set_expression_attribute_names(Some(names))
        .set_expression_attribute_values(Some(values))
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), PutItemError::ConditionalCheckFailedException(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_open_slot_update_combines_ordinary_and_open_caps() {
        let update = take_open_slot_update(
            "table",
            "m1",
            Some("side_a"),
            Some(10), // side_max_players
            Some(20), // total_max_players
            Some(2),  // max_ringer_spots
            Some(4),  // max_open_spots
        )
        .unwrap();
        let expr = update.update_expression();
        assert!(expr.contains("total_player_count"));
        assert!(expr.contains("open_joined_count"));
        assert!(expr.contains("sides.#sid.player_count"));
        assert!(expr.contains("sides.#sid.ringers_joined_count"));

        let cond = update.condition_expression().unwrap();
        assert!(cond.contains("total_player_count"));
        assert!(cond.contains("open_joined_count"));
        assert!(cond.contains("sides.#sid.max_players"));
        assert!(cond.contains("sides.#sid.ringers_joined_count"));
    }

    #[test]
    fn take_open_slot_update_uncapped_when_every_max_is_none() {
        let update = take_open_slot_update("table", "m1", None, None, None, None, None).unwrap();
        assert_eq!(
            update.condition_expression().unwrap(),
            "attribute_exists(#pk)"
        );
        assert_eq!(
            update.update_expression(),
            "ADD total_player_count :one, open_joined_count :one"
        );
    }
}
