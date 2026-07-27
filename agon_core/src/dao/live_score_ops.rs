//! Live-scoring event log: batched, transactional append with optimistic
//! concurrency (so an offline device can catch up safely), plus the derived
//! state cache.
//!
//! The event log (`LIVEEVT#<seq>`) is the source of truth; `#LIVESTATE` is a
//! rebuildable cache, never trusted as authoritative on its own. Appending is
//! the only mutation this module offers — corrections are recorded as new
//! events (e.g. a `void` payload referencing an earlier seq, interpreted by
//! the API layer), never edits to history.

use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{LiveEventPayloadRecord, LiveEventRecord, LiveStateRecord};

pub const TYPE_LIVE_EVENT: &str = "live_event";
pub const TYPE_LIVE_STATE: &str = "live_state";

/// DynamoDB caps `TransactWriteItems` at 100 items per call. One of those is
/// the seq-counter reservation, leaving this many for the events themselves.
/// Callers with a larger offline backlog split it into multiple batches.
pub const MAX_LIVE_EVENTS_PER_BATCH: usize = 99;

/// One event to append, before a seq has been assigned by the DAO.
#[derive(Debug, Clone)]
pub struct NewLiveEvent {
    pub payload: LiveEventPayloadRecord,
    pub client_event_id: String,
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
                client_event_id: event.client_event_id.clone(),
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

    /// Read every live event in the match's log, in seq order. Drains all
    /// query pages — event logs are small (low hundreds at most for a single
    /// match) so this is never a 1 MB-page concern.
    pub async fn list_live_events(&self, match_id: &str) -> DaoResult<Vec<LiveEventRecord>> {
        self.query_match_collection(match_id, Sk::LiveEvent(0).prefix())
            .await
    }

    /// Fetch the cached derived live state. `None` if never computed.
    pub async fn get_live_state(&self, match_id: &str) -> DaoResult<Option<LiveStateRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::LiveState.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Write (overwrite) the cached derived live state. Best-effort: never
    /// part of the append transaction, always safely recomputable from the
    /// log, so a failure here doesn't need to roll back the append.
    pub async fn put_live_state(&self, match_id: &str, state: &LiveStateRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::LiveState,
            TYPE_LIVE_STATE,
            state,
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
}
