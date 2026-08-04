//! Live-scoring event log: batched, transactional append with optimistic
//! concurrency (so an offline device can catch up safely), plus direct
//! delete/amend for corrections.
//!
//! The event log (`LIVEEVT#<seq>`) is the sole source of truth — one item per
//! event, with no per-item size ceiling regardless of how long a match runs.
//! The derived scorecard it folds into lives in `MatchScoreRecord`
//! (`LIVESCORE#<sport>`) — see that record's doc comment. Corrections are
//! direct mutations of the log — `delete_live_event` removes an item
//! outright, `amend_live_event` overwrites its payload in place — not a
//! layered "this is void" marker to filter out on every read.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::delete_item::DeleteItemError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{LiveEventPayloadRecord, LiveEventRecord};

pub const TYPE_LIVE_EVENT: &str = "live_event";

/// DynamoDB caps `TransactWriteItems` at 100 items per call. One of those is
/// the seq-counter reservation, leaving this many for the events themselves.
/// Callers with a larger offline backlog split it into multiple batches.
pub const MAX_LIVE_EVENTS_PER_BATCH: usize = 99;

/// One event to append, before a seq has been assigned by the DAO.
#[derive(Debug, Clone)]
pub struct NewLiveEvent {
    pub payload: LiveEventPayloadRecord,
    pub recorded_by_user_id: String,
    pub occurred_at: String,
    pub recorded_at: String,
}

impl Dao {
    /// Atomically reserve the next `events.len()` seq numbers and write them,
    /// all-or-nothing. `expected_last_seq` must match the match's current tip
    /// (`MatchRecord.live_seq`) or the call fails with `Conflict` — the caller
    /// re-fetches the log/tip and reconciles (two devices scoring the same
    /// match, or a retried batch racing a previous attempt that actually
    /// landed). This single conditional update on the meta item is both the
    /// concurrency check and the seq reservation, so there's no window where a
    /// range is reserved but not yet committed to being written.
    ///
    /// Returns the new tip seq on success. `events` must be non-empty and at
    /// most [`MAX_LIVE_EVENTS_PER_BATCH`].
    pub async fn append_live_events(
        &self,
        match_id: &str,
        expected_last_seq: u32,
        events: &[NewLiveEvent],
    ) -> DaoResult<u32> {
        if events.is_empty() {
            return Err(DaoError::Malformed("no events to append".into()));
        }
        if events.len() > MAX_LIVE_EVENTS_PER_BATCH {
            return Err(DaoError::Malformed(format!(
                "batch of {} exceeds the {MAX_LIVE_EVENTS_PER_BATCH}-event limit",
                events.len()
            )));
        }

        let n = events.len() as u32;
        let new_tip = expected_last_seq + n;

        // Reserve [expected_last_seq+1, new_tip] by bumping the counter,
        // conditioned on it currently reading `expected_last_seq`.
        // `if_not_exists` treats a never-written counter as 0, matching a
        // fresh match's implicit tip of 0; the condition's first branch
        // covers that same never-written case explicitly (a plain
        // `live_seq = :expected` would never match when the attribute is
        // absent, even if :expected is 0).
        let reserve = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET live_seq = if_not_exists(live_seq, :zero) + :n")
            .condition_expression(
                "attribute_exists(#pk) AND \
                 ((attribute_not_exists(live_seq) AND :expected = :zero) OR live_seq = :expected)",
            )
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":zero", AttributeValue::N("0".into()))
            .expression_attribute_values(":n", AttributeValue::N(n.to_string()))
            .expression_attribute_values(
                ":expected",
                AttributeValue::N(expected_last_seq.to_string()),
            )
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(reserve).build());

        for (i, event) in events.iter().enumerate() {
            let seq = expected_last_seq + 1 + i as u32;
            let record = LiveEventRecord {
                seq,
                payload: event.payload.clone(),
                recorded_by_user_id: event.recorded_by_user_id.clone(),
                occurred_at: event.occurred_at.clone(),
                recorded_at: event.recorded_at.clone(),
            };
            let item = to_item(
                &Pk::Match(match_id.into()),
                &Sk::LiveEvent(seq),
                TYPE_LIVE_EVENT,
                &record,
            )?;
            let put = Put::builder()
                .table_name(self.table())
                .set_item(Some(item))
                .condition_expression("attribute_not_exists(#pk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }

        match tx.send().await {
            Ok(_) => Ok(new_tip),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("match {match_id} live log has moved on from seq {expected_last_seq}"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Delete a single live event outright — a correction for "this never
    /// happened" (a duplicate, a wrong-match entry), not an edit. `NotFound`
    /// if it's already gone. Doesn't touch `live_seq`: that counter only
    /// ever tracks the highest seq ever assigned, which stays a valid fact
    /// regardless of which earlier seqs still physically exist.
    pub async fn delete_live_event(&self, match_id: &str, seq: u32) -> DaoResult<()> {
        let result = self
            .client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::LiveEvent(seq).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_delete_conditional_failure(&e) => Err(DaoError::NotFound(format!(
                "live event {seq} on match {match_id}"
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Overwrite a single live event's payload in place — a correction for
    /// "this happened, but I recorded the wrong facts" (wrong bowler, wrong
    /// runs, wrong dismissal), keeping its position in the log. `NotFound` if
    /// the seq doesn't exist. Unconditional beyond that: two people amending
    /// the exact same ball at the exact same moment is not a case this app's
    /// usage pattern (one active scorer at a time) needs to guard against.
    pub async fn amend_live_event(
        &self,
        match_id: &str,
        seq: u32,
        payload: &LiveEventPayloadRecord,
    ) -> DaoResult<()> {
        let payload_attr = to_attr(payload)?;
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::LiveEvent(seq).to_string()))
            .update_expression("SET payload = :payload")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":payload", payload_attr)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Err(DaoError::NotFound(format!(
                "live event {seq} on match {match_id}"
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Read every live event in the match's log, in seq order. Drains all
    /// query pages, so this stays correct regardless of how many events a
    /// match accumulates (a full multi-innings, unlimited-overs match can run
    /// to thousands) — each is its own small item, so there's no per-item
    /// size concern the way one record holding the whole log would have.
    /// Internal use only (folding the whole log) — the public API paginates
    /// via `list_live_events_page` instead of ever returning this whole.
    pub async fn list_live_events(&self, match_id: &str) -> DaoResult<Vec<LiveEventRecord>> {
        self.query_match_collection(match_id, Sk::LiveEvent(0).prefix())
            .await
    }

    /// One page of the match's live event log, oldest first (`seq` order —
    /// the zero-padded key sorts numerically). What `GET
    /// /matches/:id/live/events` actually serves.
    pub async fn list_live_events_page(
        &self,
        match_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<LiveEventRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
                .expression_attribute_values(":sk", s(Sk::LiveEvent(0).prefix())),
            cursor,
            limit,
        )
        .await
    }
}

/// Serialize a record value into a DynamoDB `AttributeValue` (nested map).
fn to_attr<T: serde::Serialize>(value: &T) -> DaoResult<AttributeValue> {
    Ok(serde_dynamo::to_attribute_value(value)?)
}

fn is_delete_conditional_failure(err: &SdkError<DeleteItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), DeleteItemError::ConditionalCheckFailedException(_))
    )
}

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}
