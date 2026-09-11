using Toybox.Application.Storage;
using Toybox.Lang;

//! Offline-safe local queue of not-yet-confirmed live-scoring events, plus
//! the `expected_last_seq` bookkeeping `POST /matches/:id/live/events`
//! needs (see that endpoint's doc comment in
//! `agon_service/src/live_score/mod.rs` — this class exists specifically
//! to be "a device with an offline backlog that resubmits with whatever
//! tip it last saw").
//!
//! Backed by `Application.Storage` (not `Properties` — this is app-owned
//! state, not user-editable settings), so a queued goal survives the watch
//! rebooting mid-match. Every event kept here is already in its final
//! wire shape (see `FootballEvents`) — this class only ever moves
//! dictionaries around, never builds or interprets them.
class EventQueue {

    const STORAGE_KEY_EVENTS = "eq_events";
    const STORAGE_KEY_LAST_ACKED_SEQ = "eq_last_acked_seq";

    //! Up to `dao::live_score_ops::MAX_LIVE_EVENTS_PER_BATCH` (99) per
    //! request on the server side; kept well under that so one flush never
    //! needs to split into multiple calls even after an extended offline
    //! spell.
    const MAX_BATCH_SIZE = 50;

    function initialize() {
    }

    //! Append one already-wrapped event (see
    //! `FootballEvents.wrapNewEvent`) to the queue.
    function enqueue(event as Dictionary) as Void {
        var events = allEvents();
        events.add(event);
        Storage.setValue(STORAGE_KEY_EVENTS, events);
    }

    function pendingCount() as Number {
        return allEvents().size();
    }

    //! Up to `MAX_BATCH_SIZE` queued events, oldest first, ready to send as
    //! `AppendLiveEventsInput.events`. Doesn't remove them — call
    //! `confirmSent` once the server has actually accepted the batch.
    function nextBatch() as Array {
        var events = allEvents();
        if (events.size() <= MAX_BATCH_SIZE) {
            return events;
        }
        return events.slice(0, MAX_BATCH_SIZE);
    }

    //! Drop the first `count` queued events (the ones a batch just got
    //! accepted) and record the new tip. Only ever called after a `200`
    //! from the server — never speculatively — since dropping an event
    //! this class hasn't confirmed sent would lose it for good.
    function confirmSent(count as Number, newLastSeq as Number) as Void {
        var events = allEvents();
        var remaining = [];
        var i = count;
        while (i < events.size()) {
            remaining.add(events[i]);
            i += 1;
        }
        Storage.setValue(STORAGE_KEY_EVENTS, remaining);
        setLastAckedSeq(newLastSeq);
    }

    //! `null` until `AgonApiClient.fetchLiveSeq` has seeded it — see that
    //! call's doc comment on why the queue can't just assume `0`.
    function lastAckedSeq() as Number? {
        return Storage.getValue(STORAGE_KEY_LAST_ACKED_SEQ);
    }

    function setLastAckedSeq(seq as Number) as Void {
        Storage.setValue(STORAGE_KEY_LAST_ACKED_SEQ, seq);
    }

    function allEvents() as Array {
        var events = Storage.getValue(STORAGE_KEY_EVENTS);
        if (events == null) {
            return [];
        }
        return events;
    }
}
