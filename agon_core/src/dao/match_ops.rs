//! Match operations: create (meta + sides + players in one transaction),
//! get aggregate, update meta, detailed score, and player roster writes.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ItemBuilder, from_item, item_sk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{
    ConfirmedScoreRecord, MatchDetailedScoreRecord, MatchPlayerRecord, MatchRecord,
    MatchSideRecord, PendingScoreRecord,
};

pub const TYPE_MATCH: &str = "match";
pub const TYPE_MATCH_SIDE: &str = "match_side";
pub const TYPE_MATCH_PLAYER: &str = "match_player";
pub const TYPE_MATCH_DETAIL: &str = "match_detail";

/// A match plus its sides and players, assembled from one collection query.
/// Excludes the detailed score, likes and comments (fetched separately).
#[derive(Debug)]
pub struct MatchAggregate {
    pub match_: MatchRecord,
    pub sides: Vec<MatchSideRecord>,
    pub players: Vec<MatchPlayerRecord>,
}

impl Dao {
    /// Create a match with its sides and players in a single transaction.
    /// `Conflict` if the match id already exists.
    ///
    /// Note: DynamoDB caps a transaction at 100 items, so a match with a very
    /// large roster would need chunking — not handled here (fine for real
    /// team sizes). Feed fan-out happens asynchronously off the stream, not here.
    pub async fn create_match(
        &self,
        match_: &MatchRecord,
        sides: &[MatchSideRecord],
        players: &[MatchPlayerRecord],
    ) -> DaoResult<()> {
        let meta_item = to_item(&Pk::Match(match_.id.clone()), &Sk::Meta, TYPE_MATCH, match_)?;

        let put_meta = Put::builder()
            .table_name(self.table())
            .set_item(Some(meta_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_meta).build());

        for side in sides {
            let item = to_item(
                &Pk::Match(match_.id.clone()),
                &Sk::Side(side.side_id.clone()),
                TYPE_MATCH_SIDE,
                side,
            )?;
            let put = Put::builder()
                .table_name(self.table())
                .set_item(Some(item))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }

        for player in players {
            let put = Put::builder()
                .table_name(self.table())
                .set_item(Some(self.match_player_item(&match_.id, player)?))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }

        match tx.send().await {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("match {} already exists", match_.id),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch the match aggregate (meta + sides + players) in one partition query.
    /// `None` if the meta item is absent. Likes/comments/detailed-score are
    /// deliberately not loaded here — fetch via their own paginated ops.
    pub async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>> {
        // Read the whole match partition and pick out the meta/side/player items.
        //
        // Note: we deliberately do NOT use a `SK < "SCORESUB#"` range bound to
        // skip the large LIKE#/COMMENT# ranges — the SK prefixes don't partition
        // cleanly by that boundary. Lexical order is:
        //   #META < COMMENT# < DETAIL# < LIKE# < PLAYER# < SCORESUB# < SIDE#
        // so `SIDE#` sorts *after* `SCORESUB#`; any upper bound that excludes
        // score submissions also excludes the sides we need (that bug returned an
        // aggregate with no sides, which broke score validation). Filtering by SK
        // type in the loop below is correct regardless of ordering.
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut meta: Option<MatchRecord> = None;
        let mut sides: Vec<MatchSideRecord> = Vec::new();
        let mut players: Vec<MatchPlayerRecord> = Vec::new();

        for item in out.items.unwrap_or_default() {
            match item_sk(&item)? {
                Sk::Meta => meta = Some(from_item(item)?),
                Sk::Side(_) => sides.push(from_item(item)?),
                Sk::Player(_) => players.push(from_item(item)?),
                // DETAIL#, LIKE#, COMMENT#, SCORESUB# etc. — not part of this aggregate.
                _ => {}
            }
        }

        Ok(meta.map(|match_| MatchAggregate {
            match_,
            sides,
            players,
        }))
    }

    /// Update a match's mutable meta fields. Any `Some` field is written; `name`,
    /// `description`, `status`, `starts_at`, `location` (Some(None) clears it),
    /// and the resolved `confirmed_score`/`pending_score` blobs. `NotFound` if
    /// the match is absent.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_match_meta(
        &self,
        match_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        starts_at: Option<&str>,
        confirmed_score: Option<ConfirmedScoreRecord>,
        pending_score: Option<Option<PendingScoreRecord>>,
    ) -> DaoResult<()> {
        let mut set: Vec<String> = Vec::new();
        let mut remove: Vec<String> = Vec::new();
        let mut names: std::collections::HashMap<String, String> = Default::default();
        let mut values: std::collections::HashMap<String, AttributeValue> = Default::default();

        let set_str =
            |field: &str,
             alias: &str,
             val: &str,
             set: &mut Vec<String>,
             names: &mut std::collections::HashMap<String, String>,
             values: &mut std::collections::HashMap<String, AttributeValue>| {
                set.push(format!("#{alias} = :{alias}"));
                names.insert(format!("#{alias}"), field.to_string());
                values.insert(format!(":{alias}"), s(val));
            };

        if let Some(v) = name {
            set_str("name", "name", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = description {
            set_str("description", "desc", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = status {
            set_str("status", "status", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = starts_at {
            set_str("starts_at", "starts", v, &mut set, &mut names, &mut values);
        }
        if let Some(cs) = confirmed_score {
            set.push("#cs = :cs".into());
            names.insert("#cs".into(), "confirmed_score".into());
            values.insert(":cs".into(), to_attr(&cs)?);
        }
        match pending_score {
            Some(Some(ps)) => {
                set.push("#ps = :ps".into());
                names.insert("#ps".into(), "pending_score".into());
                values.insert(":ps".into(), to_attr(&ps)?);
            }
            Some(None) => {
                remove.push("#ps".into());
                names.insert("#ps".into(), "pending_score".into());
            }
            None => {}
        }

        if set.is_empty() && remove.is_empty() {
            return Ok(());
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
        names.insert("#pk".into(), ATTR_PK.into());

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
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
                Err(DaoError::NotFound(format!("match {match_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Add or update a single match player (roster reconciliation / late adds).
    pub async fn put_match_player(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
    ) -> DaoResult<()> {
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Fetch a match's detailed score. `None` if none recorded.
    pub async fn get_match_detailed_score(
        &self,
        match_id: &str,
        sport: &str,
    ) -> DaoResult<Option<MatchDetailedScoreRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Detail(sport.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Write (overwrite) a match's detailed score.
    pub async fn put_match_detailed_score(
        &self,
        match_id: &str,
        detail: &MatchDetailedScoreRecord,
    ) -> DaoResult<()> {
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::Detail(detail.sport.clone()),
            TYPE_MATCH_DETAIL,
            detail,
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

    /// Build a match-player item, projecting players with a linked user into GSI1
    /// (`UMATCHES#<userId>`) — not used for the feed (that's fan-out), but handy
    /// for "matches involving me" style reverse lookups if needed later. Players
    /// with no user id (external) are not projected.
    fn match_player_item(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
    ) -> DaoResult<std::collections::HashMap<String, AttributeValue>> {
        let base = to_item(
            &Pk::Match(match_id.into()),
            &Sk::Player(player.player_id.clone()),
            TYPE_MATCH_PLAYER,
            player,
        )?;
        let item = match &player.user_id {
            Some(uid) => ItemBuilder::new(base)
                .gsi1(
                    format!("UMATCHES#{uid}"),
                    Pk::Match(match_id.into()).to_string(),
                )
                .build(),
            None => base,
        };
        Ok(item)
    }
}

/// Serialize any record value into a DynamoDB AttributeValue (nested map/list).
fn to_attr<T: serde::Serialize>(value: &T) -> DaoResult<AttributeValue> {
    Ok(serde_dynamo::to_attribute_value(value)?)
}

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}
