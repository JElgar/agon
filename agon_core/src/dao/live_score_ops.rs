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
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, item_sk, s, to_item};
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
    #[tracing::instrument(skip(self))]
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
            // `live_tip_seq` rides along with `live_seq` here: an append's
            // newest event is always the new physical tip too (an append
            // never creates a gap), so it's free to keep in sync — both SET
            // clauses compute the same new value off the pre-update
            // `live_seq`. This is what lets `delete_live_event` trust
            // `live_tip_seq` as the log's real tip even across repeated
            // undos (see that method's doc comment for why `live_seq` alone
            // can't be used for that once a delete has ever happened).
            .update_expression(
                "SET live_seq = if_not_exists(live_seq, :zero) + :n, \
                     live_tip_seq = if_not_exists(live_seq, :zero) + :n",
            )
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
    /// happened" (a duplicate, a wrong-match entry), not an edit — but only
    /// when it's still the log's current tip. In one `TransactWriteItems`,
    /// atomically deletes the item *and* bumps `live_seq` by one, with the
    /// bump's own condition (`live_tip_seq = :seq`) doubling as the "only the
    /// tip can be undone" guard — enforced against the live counter at
    /// commit time, not a value the caller read a moment earlier the way a
    /// plain read-then-check-then-delete would.
    ///
    /// The guard is on `live_tip_seq`, deliberately *not* `live_seq`: an
    /// earlier version of this method compared against `live_seq` directly,
    /// which is only ever equal to the physical tip's own seq before any
    /// delete has happened. The very first undo bumps `live_seq` one past
    /// the deleted event — correct for the reservation-counter role below,
    /// but from then on `live_seq` no longer equals *any* real event's seq,
    /// so a second consecutive undo (targeting the new, lower physical tip)
    /// could never satisfy `live_seq = :seq` again — every undo past the
    /// first one 400'd as "not the tip" even when it genuinely was.
    /// `live_tip_seq` is a second counter tracking the physical tip alone,
    /// kept accurate across repeated undos by resolving what the *next*
    /// physical tip will be (`find_previous_live_event_seq`) before this
    /// transaction, then writing that as part of the same guarded update.
    ///
    /// `live_seq` is still bumped alongside it, for the same reason as
    /// before: `seq` itself is never reused for a new event either way (an
    /// append always reserves a fresh number above whatever `live_seq`
    /// currently reads — see `append_live_events`), but bumping it here
    /// additionally means any device whose last-known tip predates this
    /// delete gets its next append rejected with `Conflict` instead of
    /// silently building on top of an event that's no longer there. Same
    /// "the log moved on since you last looked" guarantee
    /// `append_live_events` gives against a racing append, now also given
    /// against a racing delete.
    ///
    /// Returns the new tip on success — the bumped `live_seq` counter, i.e.
    /// the client's next `expected_last_seq`, not `live_tip_seq` (which is
    /// internal-only; see that field's doc comment on `MatchRecord`).
    ///
    /// That return value is *not* simply `seq + 1`, even though it was
    /// exactly that in an earlier version of this method (back when the
    /// tip-bump's condition was on `live_seq` itself — see this method's
    /// doc comment above — `seq + 1` only equals the post-transaction
    /// `live_seq` when `seq` happened to equal the pre-transaction
    /// `live_seq`, true for a match's very first undo but false for every
    /// undo after that, once `seq` is a real event's number well below the
    /// climbing counter). So this reads the counter back for real —
    /// strongly consistent, since it's read immediately after this same
    /// method's own write and must reflect it.
    #[tracing::instrument(skip(self))]
    pub async fn delete_live_event(&self, match_id: &str, seq: u32) -> DaoResult<u32> {
        // Resolve what `live_tip_seq` must become if this delete succeeds,
        // before starting the transaction: the next-highest surviving
        // event's seq, or none if deleting `seq` empties the log. A plain
        // `seq - 1` isn't safe here — once an earlier undo has been
        // followed by an append, the physical log can have a permanent gap
        // below `seq` (see `append_live_events`'s doc comment), so the
        // *real* next event has to be looked up rather than assumed.
        let previous_seq = self.find_previous_live_event_seq(match_id, seq).await?;

        let delete_event = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::LiveEvent(seq).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // The `attribute_not_exists(live_tip_seq) AND live_seq = :seq`
        // branch is the migration fallback: a match whose log predates this
        // field (never appended to since it shipped) has no `live_tip_seq`
        // yet, but for such a match `live_seq` still equals the physical
        // tip exactly (nothing has ever deleted from it under the new
        // logic), so falling back to the old check is correct for exactly
        // that first delete. Every append (old matches included) stamps
        // `live_tip_seq` from then on, so this branch only ever fires once
        // per match at most.
        let mut bump_tip = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .condition_expression(
                "(attribute_not_exists(live_tip_seq) AND live_seq = :seq) OR live_tip_seq = :seq",
            )
            .expression_attribute_values(":seq", AttributeValue::N(seq.to_string()));
        bump_tip = match previous_seq {
            Some(prev) => bump_tip
                .update_expression("SET live_seq = live_seq + :one, live_tip_seq = :prev")
                .expression_attribute_values(":one", AttributeValue::N("1".into()))
                .expression_attribute_values(":prev", AttributeValue::N(prev.to_string())),
            None => bump_tip
                .update_expression("SET live_seq = live_seq + :one REMOVE live_tip_seq")
                .expression_attribute_values(":one", AttributeValue::N("1".into())),
        };
        let bump_tip = bump_tip
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // Order matters: `delete_live_event_failure` below reads position
        // 0/1 off the cancellation reasons to tell which condition failed.
        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete_event).build())
            .transact_items(TransactWriteItem::builder().update(bump_tip).build())
            .send()
            .await;

        match result {
            Ok(_) => self.live_seq_after_delete(match_id).await,
            Err(e) => match delete_live_event_failure(&e) {
                Some(DeleteLiveEventFailure::EventMissing) => Err(DaoError::NotFound(format!(
                    "live event {seq} on match {match_id}"
                ))),
                Some(DeleteLiveEventFailure::NotTip) => Err(DaoError::Conflict(format!(
                    "match {match_id} live log has moved on: seq {seq} is no longer the tip"
                ))),
                None => Err(DaoError::Dynamo(e.to_string())),
            },
        }
    }

    /// The authoritative `live_seq` to hand back from `delete_live_event`
    /// once its transaction has committed — a strongly consistent point
    /// read of the meta item's counter, not a value derived from `seq` (see
    /// `delete_live_event`'s doc comment on why that shortcut doesn't
    /// generalize past the first undo). `consistent_read` matters here
    /// specifically because this runs immediately after this same call's
    /// own write to the same item: an eventually-consistent read could
    /// still see the pre-delete value and hand the client a stale counter.
    async fn live_seq_after_delete(&self, match_id: &str) -> DaoResult<u32> {
        #[derive(serde::Deserialize)]
        struct LiveSeqOnly {
            #[serde(default)]
            live_seq: u32,
        }

        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .projection_expression("live_seq")
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let item = out.item.ok_or_else(|| {
            DaoError::Dynamo(format!(
                "match {match_id} meta item missing right after a live-event delete committed"
            ))
        })?;
        let record: LiveSeqOnly = from_item(item)?;
        Ok(record.live_seq)
    }

    /// The seq of the highest live event still standing below `before_seq`,
    /// or `None` if there isn't one (the log would go empty, or `before_seq`
    /// is already the earliest possible). Used by `delete_live_event` to
    /// resolve the next `live_tip_seq` *before* committing the delete —
    /// a single descending, `Limit(1)` range query bounded to the
    /// `LIVEEVT#` keyspace (`Sk::live_event_prefix()` as the lower bound, so
    /// it can never stray into the match's other item kinds sharing the same
    /// partition), not a full drain of the log.
    async fn find_previous_live_event_seq(
        &self,
        match_id: &str,
        before_seq: u32,
    ) -> DaoResult<Option<u32>> {
        let Some(upper) = before_seq.checked_sub(1) else {
            return Ok(None);
        };

        let resp = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND SK BETWEEN :low AND :high")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .expression_attribute_values(":low", s(Sk::live_event_prefix()))
            .expression_attribute_values(":high", s(Sk::LiveEvent(upper).to_string()))
            .scan_index_forward(false)
            .limit(1)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let Some(item) = resp.items.unwrap_or_default().into_iter().next() else {
            return Ok(None);
        };
        match item_sk(&item)? {
            Sk::LiveEvent(found_seq) => Ok(Some(found_seq)),
            other => Err(DaoError::Malformed(format!(
                "live-event range query on match {match_id} returned a non-live-event key {other}"
            ))),
        }
    }

    /// Overwrite a single live event's payload in place — a correction for
    /// "this happened, but I recorded the wrong facts" (wrong bowler, wrong
    /// runs, wrong dismissal), keeping its position in the log. `NotFound` if
    /// the seq doesn't exist. Unconditional beyond that: two people amending
    /// the exact same ball at the exact same moment is not a case this app's
    /// usage pattern (one active scorer at a time) needs to guard against.
    #[tracing::instrument(skip(self))]
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
    #[tracing::instrument(skip(self))]
    pub async fn list_live_events(&self, match_id: &str) -> DaoResult<Vec<LiveEventRecord>> {
        self.query_match_collection(match_id, &Sk::live_event_prefix())
            .await
    }

    /// One page of the match's live event log, oldest first (`seq` order —
    /// the zero-padded key sorts numerically). What `GET
    /// /matches/:id/live/events` actually serves.
    #[tracing::instrument(skip(self))]
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
                .expression_attribute_values(":sk", s(Sk::live_event_prefix())),
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

/// Which of `delete_live_event`'s two guarded writes caused a
/// `TransactWriteItems` cancellation, if either did.
enum DeleteLiveEventFailure {
    /// The delete's own condition failed — nothing exists at that seq
    /// (never did, or a concurrent delete already removed it).
    EventMissing,
    /// The tip-bump's condition failed — `seq` wasn't `live_tip_seq` at
    /// commit time (a mid-log seq, or the tip moved on since the caller last
    /// saw it).
    NotTip,
}

/// Checked by position in the cancellation-reasons list (0 = the event
/// delete, 1 = the tip bump — the same order `delete_live_event` submits
/// them in) rather than by inspecting each reason's `item` payload, the same
/// way `super::is_transaction_conditional_failure` treats the list
/// elsewhere in this crate — just distinguishing *which* item failed instead
/// of collapsing straight to a bool. `NotTip` takes priority when both
/// conditions failed at once: whenever `seq` isn't the current tip, that's
/// the more fundamental fact to report, even if the item at `seq` also
/// happens to be physically gone (e.g. a retried undo replaying a seq that
/// a previous call already deleted — the counter has moved on *and* nothing
/// lives there any more; "not the tip" is still the right rejection, not a
/// bare "not found" that would suggest `seq` was never valid at all).
/// `EventMissing` alone (tip condition satisfied, delete condition failed)
/// means `seq` genuinely is the current counter value but nothing was ever
/// written there.
fn delete_live_event_failure(
    err: &SdkError<TransactWriteItemsError>,
) -> Option<DeleteLiveEventFailure> {
    let SdkError::ServiceError(se) = err else {
        return None;
    };
    let TransactWriteItemsError::TransactionCanceledException(e) = se.err() else {
        return None;
    };
    let reasons = e.cancellation_reasons();
    let failed = |i: usize| {
        reasons
            .get(i)
            .is_some_and(|r| r.code() == Some("ConditionalCheckFailed"))
    };
    if failed(1) {
        Some(DeleteLiveEventFailure::NotTip)
    } else if failed(0) {
        Some(DeleteLiveEventFailure::EventMissing)
    } else {
        None
    }
}
