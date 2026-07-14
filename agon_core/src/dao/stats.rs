//! Per-user, per-sport stats: list, and reconcile from a match's current state.

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, from_item, item_sk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{StatContributionRecord, UserSportStatsRecord};

/// Type tag for the per-match stat-contribution item.
pub const TYPE_STAT_CONTRIBUTION: &str = "stat_contribution";

impl Dao {
    /// List all of a user's per-sport stats (one item per sport). Unpaginated —
    /// a user only has a handful of sports.
    pub async fn list_user_stats(&self, user_id: &str) -> DaoResult<Vec<UserSportStatsRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::Stats(String::new()).prefix()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut stats = Vec::new();
        for item in out.items.unwrap_or_default() {
            // Every item under this range is a stats item, but guard anyway.
            if let Sk::Stats(_) = item_sk(&item)? {
                stats.push(from_item(item)?);
            }
        }
        Ok(stats)
    }

    /// User ids that currently have a stored stat contribution for this match.
    /// The reconciler unions these with the match's current participants so a
    /// player removed from the roster still gets their contribution backed out.
    pub async fn list_stat_contribution_user_ids(&self, match_id: &str) -> DaoResult<Vec<String>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::StatContribution(String::new()).prefix()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut ids = Vec::new();
        for item in out.items.unwrap_or_default() {
            if let Sk::StatContribution(uid) = item_sk(&item)? {
                ids.push(uid);
            }
        }
        Ok(ids)
    }

    /// Reconcile one participant's stats with a match's **current** state.
    ///
    /// `sport`/`played`/`won` describe what the match *should* contribute right
    /// now (`played` = completed && the user played; `won` = their side is the
    /// confirmed winner). This is diffed against the contribution we last stored
    /// for `(match, user)` and only the delta is applied to the user's
    /// `STATS#<sport>` counters — so the same call is:
    ///
    /// - **idempotent**: unchanged state → zero delta → no write (safe under
    ///   at-least-once delivery and the match-meta events fired by every
    ///   like/comment);
    /// - **self-correcting**: a re-score moves `wins`, a late roster add counts
    ///   the new player, and cancelling a completed match (`played=false`) backs
    ///   the counts out — including moving counts between sports if the match's
    ///   sport changed.
    ///
    /// Concurrency: the contribution write is conditional on the value we read,
    /// so two racing reconciles can't both apply a delta — the loser's
    /// transaction fails (`Conflict`) and redelivery re-reads and converges.
    pub async fn reconcile_match_contribution(
        &self,
        match_id: &str,
        user_id: &str,
        sport: &str,
        played: bool,
        won: bool,
    ) -> DaoResult<()> {
        let contrib_pk = Pk::Match(match_id.into()).to_string();
        let contrib_sk = Sk::StatContribution(user_id.into()).to_string();

        // What's currently stored for this (match, user), if anything.
        let stored: Option<StatContributionRecord> = {
            let out = self
                .client
                .get_item()
                .table_name(self.table())
                .key(ATTR_PK, s(&contrib_pk))
                .key(ATTR_SK, s(&contrib_sk))
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            out.item.map(from_item).transpose()?
        };

        let desired_played: u64 = played.into();
        let desired_won: u64 = won.into();

        // A missing contribution is effectively zero in the desired sport, so a
        // no-op event (e.g. a like on a scheduled match) matches and writes
        // nothing.
        let (old_sport, old_played, old_won) = match &stored {
            Some(c) => (c.match_type.clone(), c.played, c.won),
            None => (sport.to_string(), 0, 0),
        };

        if old_sport == sport && old_played == desired_played && old_won == desired_won {
            return Ok(());
        }

        let mut tx: Vec<TransactWriteItem> = Vec::new();

        // 1. Update the contribution item to the desired value (or delete it when
        //    the match no longer contributes), guarded on the value we read.
        if desired_played == 0 && desired_won == 0 {
            let mut b = Delete::builder()
                .table_name(self.table())
                .key(ATTR_PK, s(&contrib_pk))
                .key(ATTR_SK, s(&contrib_sk));
            b = match &stored {
                None => b
                    .condition_expression("attribute_not_exists(#pk)")
                    .expression_attribute_names("#pk", ATTR_PK),
                Some(c) => guard_delete(b, c),
            };
            tx.push(
                TransactWriteItem::builder()
                    .delete(b.build().map_err(|e| DaoError::Dynamo(e.to_string()))?)
                    .build(),
            );
        } else {
            let record = StatContributionRecord {
                match_type: sport.to_string(),
                played: desired_played,
                won: desired_won,
            };
            let item = to_item(
                &Pk::Match(match_id.into()),
                &Sk::StatContribution(user_id.into()),
                TYPE_STAT_CONTRIBUTION,
                &record,
            )?;
            let mut b = Put::builder().table_name(self.table()).set_item(Some(item));
            b = match &stored {
                None => b
                    .condition_expression("attribute_not_exists(#pk)")
                    .expression_attribute_names("#pk", ATTR_PK),
                Some(c) => guard_put(b, c),
            };
            tx.push(
                TransactWriteItem::builder()
                    .put(b.build().map_err(|e| DaoError::Dynamo(e.to_string()))?)
                    .build(),
            );
        }

        // 2. Apply the counter delta(s). If the sport changed, back the old
        //    contribution out of the old sport and add the new one; otherwise
        //    apply the net delta on the single sport.
        if old_sport != sport {
            if let Some(u) =
                self.stats_delta(user_id, &old_sport, -(old_played as i64), -(old_won as i64))?
            {
                tx.push(TransactWriteItem::builder().update(u).build());
            }
            if let Some(u) =
                self.stats_delta(user_id, sport, desired_played as i64, desired_won as i64)?
            {
                tx.push(TransactWriteItem::builder().update(u).build());
            }
        } else if let Some(u) = self.stats_delta(
            user_id,
            sport,
            desired_played as i64 - old_played as i64,
            desired_won as i64 - old_won as i64,
        )? {
            tx.push(TransactWriteItem::builder().update(u).build());
        }

        match self
            .client
            .transact_write_items()
            .set_transact_items(Some(tx))
            .send()
            .await
        {
            Ok(_) => Ok(()),
            // Lost an optimistic-lock race: another reconcile moved the stored
            // contribution between our read and write. Surface it so the message
            // is retried and re-reads the fresh state.
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("stat contribution for match {match_id} changed concurrently"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// An `ADD matches_played/wins` update on `STATS#<sport>`, or `None` when both
    /// deltas are zero. Stamps `match_type` so a freshly-created item carries its
    /// sport.
    fn stats_delta(
        &self,
        user_id: &str,
        sport: &str,
        played_delta: i64,
        won_delta: i64,
    ) -> DaoResult<Option<Update>> {
        if played_delta == 0 && won_delta == 0 {
            return Ok(None);
        }
        let u = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Stats(sport.into()).to_string()))
            .update_expression("ADD matches_played :p, wins :w SET match_type = :mt")
            .expression_attribute_values(":p", AttributeValue::N(played_delta.to_string()))
            .expression_attribute_values(":w", AttributeValue::N(won_delta.to_string()))
            .expression_attribute_values(":mt", s(sport))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(Some(u))
    }
}

/// Constrain a contribution `Put` to the value we read (optimistic lock).
fn guard_put(
    b: aws_sdk_dynamodb::types::builders::PutBuilder,
    c: &StatContributionRecord,
) -> aws_sdk_dynamodb::types::builders::PutBuilder {
    b.condition_expression("played = :op AND won = :ow AND match_type = :omt")
        .expression_attribute_values(":op", AttributeValue::N(c.played.to_string()))
        .expression_attribute_values(":ow", AttributeValue::N(c.won.to_string()))
        .expression_attribute_values(":omt", s(&c.match_type))
}

/// Constrain a contribution `Delete` to the value we read (optimistic lock).
fn guard_delete(
    b: aws_sdk_dynamodb::types::builders::DeleteBuilder,
    c: &StatContributionRecord,
) -> aws_sdk_dynamodb::types::builders::DeleteBuilder {
    b.condition_expression("played = :op AND won = :ow AND match_type = :omt")
        .expression_attribute_values(":op", AttributeValue::N(c.played.to_string()))
        .expression_attribute_values(":ow", AttributeValue::N(c.won.to_string()))
        .expression_attribute_values(":omt", s(&c.match_type))
}
