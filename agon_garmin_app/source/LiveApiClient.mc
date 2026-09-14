import Toybox.Lang;
import Toybox.Communications;
import Toybox.Time;
import Toybox.Time.Gregorian;
import Toybox.System;
import Toybox.WatchUi;

//! Posts real live-scoring events for the currently-picked match (see
//! MatchContext/MatchPickerView) — replaces MockApiClient now that
//! pairing actually produces a device token to authenticate with.
//! `FootballScore`'s own call shape into this is unchanged from
//! MockApiClient's, per that class's own original doc comment — only
//! what happens inside changed.
//!
//! Every `Communications.makeWebRequest` call in this class goes through
//! `enqueueRequest`/`pumpQueue` rather than being fired directly: Garmin's
//! own guidance is to keep at most one request outstanding at a time (see
//! https://forums.garmin.com/developer/connect-iq/f/discussion/268658/ —
//! the system technically tolerates a couple more, but a second request
//! fired before the first's callback returns can silently clobber it).
//! This class has three independent triggers that can otherwise overlap
//! this way — `agonView`'s 5s poll timer (`refreshScore`), a wearer's own
//! goal/period action (`append`), and conflict recovery firing both a
//! `/live/seq` re-fetch and `refreshScore` back-to-back — and the
//! resulting dropped/overwritten callback was exactly the bug where a
//! device recovers from one `expected_last_seq` conflict (the score
//! looks right again, since whichever of the two requests survived was
//! usually the score one) but every append after that keeps conflicting
//! forever, because the *other* request — the one that would have
//! corrected `_lastSeq` — never got to run its callback at all.
//!
//! A second, related bug survived serializing the requests: on a
//! conflict, the dropped event was never retried, and firing a score
//! refresh right when the conflict was detected (rather than after
//! resolving it) meant that refresh could land *before* anything fixed
//! `_lastSeq`, showing a score that still didn't include whatever this
//! device just tried to record. On a real device this looked like the
//! score flicking up (the optimistic local tally) then immediately back
//! down (that premature refresh) every time a goal was recorded while
//! out of sync — see `_pendingEvent`/`onSeqForRetry`, which now retries
//! the event itself once the seq is corrected, and defers the score
//! refresh until that retry (or giving up on it) actually resolves.
//!
//! `undoLast` deletes the log's current tip (`DELETE /matches/:id/
//! live/events/:seq`) — the only correction the backend supports (see
//! `delete_live_event`'s doc comment on the server: anything but the
//! real tip 400s). The seq to send is tracked separately from
//! `_lastSeq`: an append never creates a gap, so `_lastSeq` doubles as
//! the physical tip right after one, but an *undo* bumps the counter
//! past the deleted event (see `Dao::delete_live_event`'s own doc
//! comment) — from that point on `_lastSeq` and the real tip disagree
//! until the next append re-aligns them. `agon_ui`'s own
//! `useUndoTargetSeq` hook documents this exact trap in detail and
//! solves it the same way this class does: track the physical tip
//! (`_undoTargetSeq`) completely separately, seeded (`refreshUndoTargetSeq`)
//! by draining `GET /matches/:id/live/events` and taking the highest
//! `seq` actually there, never by trusting `/live/seq`'s raw counter.
//! Whenever this device *does* directly observe its own successful
//! append or undo, that response's own `last_seq` is still safe to
//! trust for whichever of the two fields it corresponds to.
class LiveApiClient {

    var _matchId as String?;
    //! The seq this device last saw for `_matchId` — 0 means "never
    //! synced", the same convention `AppendLiveEventsInput.
    //! expected_last_seq` itself uses. Seeded from
    //! `GET /matches/:id/live/seq` in `setMatch`, then kept in step from
    //! each append response's own returned `last_seq`.
    var _lastSeq as Number;
    var _apiClient as MatchApiClient;

    //! The most recent goal/period event this device tried to append but
    //! got rejected with a 409 conflict — see `onAppendResponse`/
    //! `onSeqForRetry`. Retried exactly once, after `_lastSeq` is fixed;
    //! `null` once there's nothing pending a retry (never sent, already
    //! succeeded, or the retry itself has already been fired). Recording
    //! a second goal/period while this is still pending (very tight
    //! timing — the wearer would need to get through the whole side ->
    //! scorer -> assist flow again before the first retry lands) would
    //! overwrite this and lose the first event's retry; not handled,
    //! same "basics only" scope as the rest of this class.
    var _pendingEvent as Dictionary?;
    //! True while `sendAppend` is (re-)sending `_pendingEvent` as a
    //! retry from `onSeqForRetry`, rather than a fresh call from
    //! `recordGoal`/`recordPeriod` — see `onAppendResponse`'s use of it.
    var _isRetryInFlight as Boolean;

    //! The log's real physical tip — what `undoLast` sends `DELETE
    //! .../live/events/:seq` at. `null` whenever it isn't currently
    //! known (never derived yet, or invalidated after this device's own
    //! undo/a stale-tip rejection) — `canUndo()` is false in that case,
    //! rather than risk sending a guessed seq. See the class doc comment
    //! on why this is never seeded from `_lastSeq`/`/live/seq` directly.
    var _undoTargetSeq as Number?;
    //! Running highest `seq` seen while `refreshUndoTargetSeq` drains
    //! the event log page by page — meaningless once that finishes
    //! (`_undoTargetSeq` holds the real answer from then on).
    var _undoDrainMaxSeq as Number;

    //! Requests not yet started: `{"url"=>.., "params"=>.., "options"=>..,
    //! "callback"=>..}` dictionaries, oldest first. See the class doc
    //! comment above.
    var _pendingRequests as Array;
    var _requestInFlight as Boolean;
    //! The real callback for whichever request is currently in flight —
    //! every actual `Communications.makeWebRequest` call in this class
    //! uses `onQueuedResponse` as ITS callback, which then invokes this
    //! one and starts the next queued request.
    var _currentCallback as Method?;

    function initialize() {
        _matchId = null;
        _lastSeq = 0;
        _apiClient = new MatchApiClient();
        _pendingEvent = null;
        _isRetryInFlight = false;
        _undoTargetSeq = null;
        _undoDrainMaxSeq = 0;
        _pendingRequests = [];
        _requestInFlight = false;
        _currentCallback = null;
    }

    //! Queue a web request instead of calling `Communications.
    //! makeWebRequest` directly — see the class doc comment.
    function enqueueRequest(url as String, params as Dictionary?, options as Dictionary, callback as Method) as Void {
        _pendingRequests = _pendingRequests.add({
            "url" => url,
            "params" => params,
            "options" => options,
            "callback" => callback
        });
        pumpQueue();
    }

    //! Starts the next queued request, if nothing is already in flight.
    //! Called after every enqueue and after every request completes
    //! (`onQueuedResponse`), so a request added while another is running
    //! doesn't get lost — it just waits for that call.
    function pumpQueue() as Void {
        if (_requestInFlight || _pendingRequests.size() == 0) {
            return;
        }
        var next = _pendingRequests[0] as Dictionary;
        _pendingRequests = _pendingRequests.slice(1, null);
        _requestInFlight = true;
        _currentCallback = next.get("callback") as Method;
        Communications.makeWebRequest(
            next.get("url") as String,
            next.get("params") as Dictionary or Null,
            next.get("options") as Dictionary,
            method(:onQueuedResponse)
        );
    }

    function onQueuedResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        var callback = _currentCallback;
        _requestInFlight = false;
        _currentCallback = null;
        if (callback != null) {
            callback.invoke(responseCode, data);
        }
        // In case the callback above didn't itself enqueue anything (so
        // nothing already re-triggered this) but the queue isn't empty —
        // e.g. something was enqueued while this request was in flight.
        pumpQueue();
    }

    //! Start scoring `matchId` — called once, from MatchPickerView, right
    //! after `MatchContext` is populated for the same match.
    function setMatch(matchId as String) as Void {
        _matchId = matchId;
        _lastSeq = 0;
        _undoTargetSeq = null;
        refreshSeq();
        // Load whatever's already been scored (by this device on a
        // previous visit, or another device entirely) rather than
        // starting the screen from a misleading 0-0. Queued right behind
        // the seq fetch above rather than fired alongside it.
        refreshScore();
    }

    //! Re-fetch `GET /matches/:id/live/seq` and update `_lastSeq` from it
    //! (`onSeq`) — called from `setMatch`, and from `agonView`'s poll
    //! timer alongside `refreshScore` so `_lastSeq` stays current
    //! proactively rather than only reactively (on the next append's own
    //! conflict). Without this, *any* other client's append or undo
    //! (either bumps `live_seq` — see `Dao::delete_live_event`'s doc
    //! comment on the backend) between two of this device's own appends
    //! would predictably conflict once and need a retry to recover, even
    //! though the poll timer was already running the whole time.
    function refreshSeq() as Void {
        if (_matchId == null) {
            return;
        }
        var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/live/seq";
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        enqueueRequest(url, null, options, method(:onSeq));
    }

    function onSeq(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var seq = dict.get("last_seq");
            if (seq != null) {
                _lastSeq = seq as Number;
            }
            // /live/seq's counter is never safe to reuse as the undo
            // target directly (see the class doc comment) — every time
            // this device (re-)learns it, from setMatch or a conflict
            // retry alike, re-derive the real physical tip from scratch
            // rather than assume the two still agree.
            _undoTargetSeq = null;
            refreshUndoTargetSeq();
        }
        // Any failure just leaves _lastSeq at 0 — the very first append
        // then either succeeds (the match genuinely has no prior events)
        // or comes back Conflict, which onAppendResponse already treats
        // as "re-fetch and retry" rather than a fatal error.
    }

    //! `scorer`/`assist` are player ids (`MatchContext`'s roster, resolved
    //! from `WatchUi.Menu` selections in GoalFlow) — `null` when skipped,
    //! same convention as the mocked version.
    function recordGoal(side as Symbol, scorer as String?, assist as String?) as Void {
        var event = {
            "sport" => "Football",
            "kind" => "Goal",
            "side_id" => getApp().matchContext.sideIdFor(side),
            "own_goal" => false,
            "penalty" => false
        };
        if (scorer != null) {
            event.put("scorer_player_id", scorer);
        }
        if (assist != null) {
            event.put("assist_player_id", assist);
        }
        append(event);
    }

    //! `period` is one of `FootballScore.PERIOD_*` — mapped here to the
    //! wire value `agon_service::detailed_score::football::FootballPeriod`
    //! actually expects (`#[oai(rename_all = "snake_case")]`). Only the
    //! four periods this app's menu ever offers are handled;
    //! `PERIOD_NOT_STARTED` never reaches here (it's the initial state,
    //! never something `setPeriod` transitions *to* from a menu action).
    function recordPeriod(period as Number) as Void {
        var wireValue = periodWireValue(period);
        if (wireValue == null) {
            return;
        }
        var event = {
            "sport" => "Football",
            "kind" => "Period",
            "period" => wireValue
        };
        append(event);
    }

    function periodWireValue(period as Number) as String? {
        if (period == FootballScore.PERIOD_KICK_OFF) {
            return "kick_off";
        }
        if (period == FootballScore.PERIOD_HALF_TIME) {
            return "half_time";
        }
        if (period == FootballScore.PERIOD_SECOND_HALF) {
            return "second_half_kick_off";
        }
        if (period == FootballScore.PERIOD_FULL_TIME) {
            return "full_time";
        }
        return null;
    }

    function append(event as Dictionary) as Void {
        if (_matchId == null) {
            // Shouldn't happen — the score screen isn't reachable before
            // MatchPickerView calls setMatch — but this method has no way
            // to report failure back to FootballScore either way, so
            // there's nothing better to do than skip the request.
            return;
        }
        // Recorded so onAppendResponse can retry this exact event once
        // (see onSeqForRetry) if it comes back as a conflict — a fresh
        // call from recordGoal/recordPeriod always overwrites whatever
        // was pending (see the field's own doc comment on that).
        _pendingEvent = event;
        sendAppend(event);
    }

    //! The actual POST — split out from `append` so `onSeqForRetry` can
    //! resend the same event after a conflict without re-deciding
    //! whether it should be tracked for retry (it already is).
    function sendAppend(event as Dictionary) as Void {
        var body = {
            "expected_last_seq" => _lastSeq,
            "events" => [
                { "occurred_at" => nowIso(), "event" => event }
            ]
        };
        var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/live/events";
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_POST,
            :headers => {
                "Authorization" => "Bearer " + DeviceAuth.getAccessToken(),
                "Content-Type" => Communications.REQUEST_CONTENT_TYPE_JSON
            },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        enqueueRequest(url, body, options, method(:onAppendResponse));
    }

    function onAppendResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        // Temporary — see MatchPickerView's matching comment. Traces the
        // conflict-retry path end to end: which attempt this is, what
        // came back, and what _lastSeq/_pendingEvent looked like when it
        // did.
        System.println(
            "[live-api] onAppendResponse responseCode=" + responseCode +
            " wasRetry=" + _isRetryInFlight +
            " lastSeq=" + _lastSeq +
            " pendingEvent=" + (_pendingEvent != null)
        );
        // Cleared up front — every branch below either finishes the
        // conflict-recovery saga this flag tracks, or was never part of
        // one to begin with.
        var wasRetry = _isRetryInFlight;
        _isRetryInFlight = false;

        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var seq = dict.get("last_seq");
            if (seq != null) {
                _lastSeq = seq as Number;
            }
            _pendingEvent = null;
            // A fresh (non-retry) success needs no extra refresh — the
            // optimistic local tally FootballScore already applied is
            // correct. A retry's success is different: onSeqForRetry's
            // refreshScore (see below) already ran and would have shown
            // a stale score missing this now-committed event, so surface
            // the real one immediately instead of waiting up to 5s for
            // the next poll tick.
            if (wasRetry) {
                refreshScore();
            }
            return;
        }
        if (responseCode == 409 && _matchId != null) {
            // Another writer moved the log on since we last synced (or
            // this device just hasn't seeded expected_last_seq correctly
            // yet). Re-fetch the real tip — onSeqForRetry retries
            // _pendingEvent once that's back (can't retry immediately,
            // the corrected seq isn't known yet), or refreshes the score
            // itself if there's nothing left to retry (see its own doc
            // comment). Deliberately NOT refreshing the score here too,
            // even though a conflict means something else happened
            // server-side — firing it in parallel with the retry let it
            // resolve first and apply a score that doesn't include the
            // event this device is about to successfully commit,
            // visible on a real device as the score flicking up (the
            // optimistic local tally) then immediately back down (that
            // stale refresh) even though the goal *did* end up recorded.
            var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/live/seq";
            var options = {
                :method => Communications.HTTP_REQUEST_METHOD_GET,
                :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
                :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
            };
            enqueueRequest(url, null, options, method(:onSeqForRetry));
            return;
        }
        // Any other outcome (network error, 403 not-an-admin, ...): there's
        // no on-watch error UI for this yet, so it's silently dropped —
        // same "basics only" scope as above.
        _pendingEvent = null;
        if (wasRetry) {
            refreshScore();
        }
    }

    //! Same as `onSeq`, plus either retries `_pendingEvent` now that
    //! `_lastSeq` is correct, or — once there's nothing left to retry —
    //! refreshes the score, which `onAppendResponse` deliberately doesn't
    //! do itself (see its own doc comment on why). Clears `_pendingEvent`
    //! before retrying, so if the retry itself comes back as a conflict,
    //! `onAppendResponse` re-syncs `_lastSeq` again but lands back here
    //! with nothing left to retry — bounding this to exactly one retry
    //! per event rather than potentially looping forever against a match
    //! under heavy concurrent writes.
    function onSeqForRetry(responseCode as Number, data as Dictionary or String or Null) as Void {
        onSeq(responseCode, data);
        System.println(
            "[live-api] onSeqForRetry responseCode=" + responseCode +
            " correctedLastSeq=" + _lastSeq +
            " pendingEvent=" + (_pendingEvent != null)
        );
        if (_pendingEvent != null) {
            var event = _pendingEvent as Dictionary;
            _pendingEvent = null;
            _isRetryInFlight = true;
            sendAppend(event);
        } else {
            refreshScore();
        }
    }

    //! Re-derive `_undoTargetSeq` by draining `GET /matches/:id/
    //! live/events` from the start and taking the highest `seq` actually
    //! there (0 if the match has no events at all) — see the class doc
    //! comment on why this, not `/live/seq`'s own counter, is the only
    //! safe source for it. Cheap in practice: a football match's whole
    //! log (goals + period markers) is a handful of small items, almost
    //! always one page.
    function refreshUndoTargetSeq() as Void {
        if (_matchId == null) {
            return;
        }
        _undoDrainMaxSeq = 0;
        fetchLiveEventsPage(null);
    }

    function fetchLiveEventsPage(cursor as String?) as Void {
        var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/live/events";
        var params = { "limit" => "20" };
        if (cursor != null) {
            params.put("cursor", cursor);
        }
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        enqueueRequest(url, params, options, method(:onLiveEventsPage));
    }

    function onLiveEventsPage(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode != 200 || data == null) {
            // Leave _undoTargetSeq at null (unknown) — canUndo() stays
            // false rather than risk a guessed seq. Picked back up
            // whenever something next calls refreshUndoTargetSeq.
            return;
        }
        var dict = data as Dictionary;
        var items = dict.get("items");
        if (items != null) {
            var itemsArray = items as Array;
            var i = 0;
            while (i < itemsArray.size()) {
                var item = itemsArray[i] as Dictionary;
                var seq = item.get("seq");
                if (seq != null && (seq as Number) > _undoDrainMaxSeq) {
                    _undoDrainMaxSeq = seq as Number;
                }
                i += 1;
            }
        }
        var nextCursor = dict.get("next_cursor");
        if (nextCursor != null) {
            fetchLiveEventsPage(nextCursor as String);
            return;
        }
        _undoTargetSeq = _undoDrainMaxSeq;
    }

    //! Whether `undoLast` has a real seq to send — `false` while it's
    //! still being derived (e.g. right after `setMatch`) or once nothing
    //! has ever been recorded yet, same as `agon_ui`'s own
    //! `UndoLastEventButton` treating a `0`/`undefined` seq as "nothing
    //! to undo".
    function canUndo() as Boolean {
        return _undoTargetSeq != null && (_undoTargetSeq as Number) > 0;
    }

    //! Undo the log's current tip (`DELETE /matches/:id/live/events/
    //! :seq`) — the only correction the backend supports; anything but
    //! the real tip 400s (see `delete_live_event`'s doc comment on the
    //! server). Fire-and-forget, same convention as `recordGoal`/
    //! `recordPeriod` — `agonMenuDelegate` calls this and just asks for
    //! a redraw, no confirmation step (unlike `agon_ui`'s own confirm
    //! dialog — a deliberate simplification here, not an oversight).
    //!
    //! Unlike an append conflict, a failed undo is never safely
    //! retryable with a "corrected" seq: retrying an *append* after a
    //! conflict resends the exact same intended write, but retrying a
    //! *delete* against a since-moved tip would delete a different,
    //! unintended event. So `onUndoResponse` only ever re-syncs state on
    //! failure — it never retries the delete itself.
    function undoLast() as Void {
        if (_matchId == null || !canUndo()) {
            return;
        }
        var url = API_BASE_URL + "/matches/" + (_matchId as String) +
            "/live/events/" + (_undoTargetSeq as Number).toString();
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_DELETE,
            :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        enqueueRequest(url, null, options, method(:onUndoResponse));
    }

    function onUndoResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var seq = dict.get("last_seq");
            if (seq != null) {
                _lastSeq = seq as Number;
            }

            var homeGoals = getApp().score.homeGoals;
            var awayGoals = getApp().score.awayGoals;
            var scoreObj = dict.get("score");
            var period = undoPeriodFromScoreObj(scoreObj);
            var currentHalfStartedAt = undoCurrentHalfStartMoment(scoreObj, period);
            if (scoreObj != null) {
                var score = scoreObj as Dictionary;
                var tally = score.get("score");
                if (tally != null) {
                    var tallyDict = tally as Dictionary;
                    var home = tallyDict.get(getApp().matchContext.side0Id);
                    var away = tallyDict.get(getApp().matchContext.side1Id);
                    if (home != null) {
                        homeGoals = home as Number;
                    }
                    if (away != null) {
                        awayGoals = away as Number;
                    }
                }
            }
            getApp().score.applyUndoState(homeGoals, awayGoals, period, currentHalfStartedAt);
            WatchUi.requestUpdate();

            // The deleted event's own seq is gone, and _lastSeq above
            // now points past it (see Dao::delete_live_event's doc
            // comment on the backend) — nothing safely tells us the new
            // physical tip without a fresh drain.
            _undoTargetSeq = null;
            refreshUndoTargetSeq();
            return;
        }
        // 400 ("only the most recently recorded event can be undone" —
        // this device's tracked target wasn't actually the tip anymore,
        // another writer moved it) or any other failure: this specific
        // undo attempt is simply dropped — see this method's own doc
        // comment on why a delete can't safely be retried the way an
        // append conflict can. Re-sync everything for next time.
        refreshSeq();
        refreshUndoTargetSeq();
        refreshScore();
    }

    //! `scoreObj` is a `LiveScoreSnapshot`'s `score` field (`dict.
    //! get("score")`) — `null` only if a 200 response is somehow missing
    //! it entirely (shouldn't happen; `dict.get` just never guarantees
    //! non-null either way).
    function undoPeriodFromScoreObj(scoreObj as Object?) as Number? {
        if (scoreObj == null) {
            return null;
        }
        return periodFromScore(scoreObj as Dictionary);
    }

    //! Same shape as `undoPeriodFromScoreObj` — `scoreObj` typed `Object?`
    //! rather than `Dictionary?` for the same reason (a raw JSON value,
    //! not yet cast), guarding `currentHalfStartMoment` the same way.
    function undoCurrentHalfStartMoment(scoreObj as Object?, period as Number?) as Time.Moment? {
        if (scoreObj == null) {
            return null;
        }
        return currentHalfStartMoment(scoreObj as Dictionary, period);
    }

    //! Poll the server's authoritative score (`GET /matches/:id/score`)
    //! and apply it onto `FootballScore` (`applyServerState`) — the local
    //! tally otherwise only ever reflects *this* device's own
    //! recordGoal/setPeriod calls, so another device recording something
    //! would otherwise never reach the screen. Called from `agonView`'s
    //! poll timer while the score screen is visible, plus once from
    //! `setMatch` and once on every append conflict (see above).
    function refreshScore() as Void {
        if (_matchId == null) {
            return;
        }
        var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/score";
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        enqueueRequest(url, null, options, method(:onScore));
    }

    function onScore(responseCode as Number, data as Dictionary or String or Null) as Void {
        System.println("[live-api] onScore responseCode=" + responseCode);
        if (responseCode == 404) {
            // No live events yet — the match hasn't kicked off. Nothing to
            // apply onto the tally, but it does tell this device the
            // match's state (see FootballScore.markNoServerScore).
            getApp().score.markNoServerScore();
            return;
        }
        if (responseCode != 200 || data == null) {
            // A network error: nothing to apply, leave the current tally
            // as-is rather than resetting it to 0-0.
            return;
        }
        var score = data as Dictionary;
        var homeGoals = getApp().score.homeGoals;
        var awayGoals = getApp().score.awayGoals;
        var tally = score.get("score");
        if (tally != null) {
            var tallyDict = tally as Dictionary;
            var home = tallyDict.get(getApp().matchContext.side0Id);
            var away = tallyDict.get(getApp().matchContext.side1Id);
            if (home != null) {
                homeGoals = home as Number;
            }
            if (away != null) {
                awayGoals = away as Number;
            }
        }
        var period = periodFromScore(score);
        var currentHalfStartedAt = currentHalfStartMoment(score, period);
        if (getApp().score.applyServerState(homeGoals, awayGoals, period, currentHalfStartedAt)) {
            // Another device kicked off while this watch had the match
            // open — start this wearer's activity too.
            getApp().activityRecorder.startForKickOff(getApp().matchContext.matchName());
        }
        WatchUi.requestUpdate();
    }

    //! `null` if the score has no period marker yet, or one this app
    //! doesn't recognize (extra time, penalties — not offered on this
    //! app's menu, see `periodWireValue`'s doc comment).
    function periodFromScore(score as Dictionary) as Number? {
        var period = score.get("period");
        if (period == null) {
            return null;
        }
        return periodFromWireValue(period as String);
    }

    function periodFromWireValue(value as String) as Number? {
        if (value.equals("kick_off")) {
            return FootballScore.PERIOD_KICK_OFF;
        }
        if (value.equals("half_time")) {
            return FootballScore.PERIOD_HALF_TIME;
        }
        if (value.equals("second_half_kick_off")) {
            return FootballScore.PERIOD_SECOND_HALF;
        }
        if (value.equals("full_time")) {
            return FootballScore.PERIOD_FULL_TIME;
        }
        return null;
    }
}

//! Current UTC time as an RFC-3339 / ISO-8601 string (e.g.
//! `2026-06-01T10:30:15Z`) — what `NewLiveEventInput.occurred_at` and
//! `AppendLiveEventsInput`'s events need. `Time.now()` is already UTC;
//! `Gregorian.utcInfo` (not `info`, which is local time) reads it back
//! out as separate numeric fields to format.
function nowIso() as String {
    var info = Gregorian.utcInfo(Time.now(), Time.FORMAT_SHORT);
    return Lang.format(
        "$1$-$2$-$3$T$4$:$5$:$6$Z",
        [
            info.year.format("%04d"),
            info.month.format("%02d"),
            info.day.format("%02d"),
            info.hour.format("%02d"),
            info.min.format("%02d"),
            info.sec.format("%02d")
        ]
    );
}

//! The inverse of `nowIso` — parses an RFC-3339 / ISO-8601 UTC timestamp
//! (e.g. `agon_service`'s `FootballScore.period_times` entries, same wire
//! shape `nowIso` produces) back into a `Time.Moment`. Only reads the
//! first 19 characters (date + `HH:MM:SS`); any fractional-seconds suffix
//! chrono's serializer adds (`.123`, `.123456789`, ...) is ignored, since
//! nothing here needs sub-second precision. `null` for anything shorter
//! than that or structurally unparseable (a malformed field shouldn't
//! happen against a real API response, but this parses arbitrary JSON, so
//! it's not assumed — same caution as `MatchContext.playerEntry`).
//!
//! `Gregorian.moment`'s own options dictionary is documented as expecting
//! its `:year`/.../`:second` fields in UTC, matching `nowIso`'s own
//! `utcInfo`-sourced fields above — no timezone conversion needed either
//! way.
function parseIso(iso as String) as Time.Moment? {
    if (iso.length() < 19) {
        return null;
    }
    var year = iso.substring(0, 4).toNumber();
    var month = iso.substring(5, 7).toNumber();
    var day = iso.substring(8, 10).toNumber();
    var hour = iso.substring(11, 13).toNumber();
    var minute = iso.substring(14, 16).toNumber();
    var second = iso.substring(17, 19).toNumber();
    if (year == null || month == null || day == null || hour == null || minute == null || second == null) {
        return null;
    }
    return Gregorian.moment({
        :year => year,
        :month => month,
        :day => day,
        :hour => hour,
        :minute => minute,
        :second => second,
    });
}

//! "kick_off"/"second_half_kick_off" — the `period_times` key of whichever
//! half `period` says is currently running — `null` for any other period
//! (not started, half-time, full-time, or unrecognized), same as
//! `currentHalfStartMoment`'s own doc comment below. Split out (rather
//! than a local `var key = null; ...` inside that function) so `key`
//! there infers its type from this function's own declared `String?`
//! return type — a bare `null` literal doesn't give a local variable
//! enough to infer from (see `MatchContext.memberName`'s doc comment for
//! the same pattern, hit the same way).
function currentHalfPeriodKey(period as Number?) as String? {
    if (period == FootballScore.PERIOD_KICK_OFF) {
        return "kick_off";
    }
    if (period == FootballScore.PERIOD_SECOND_HALF) {
        return "second_half_kick_off";
    }
    return null;
}

//! The wall-clock start of whichever half is *currently running*, per the
//! server's own `period_times` — `period_times["kick_off"]` during the
//! first half, `period_times["second_half_kick_off"]` during the second,
//! `null` any other time (not started, half-time, full-time, or an
//! unrecognized period like extra time/penalties — same periods
//! `periodWireValue` offers, nothing more). Deliberately reads this off
//! the server's own timestamp rather than this device's local
//! `ActivityRecorder` clock — that's this device's own recording, which
//! only tracks the halves *this watch* tapped through (see
//! docs/garmin-live-scoring.md's "still to fix" #13) — so this stays
//! correct even on a watch that opened a match someone else is scoring,
//! or isn't recording an activity at all. `score` is a `Score`'s own
//! wire shape — either `GET /matches/:id/score`'s body directly, or a
//! `LiveScoreSnapshot`'s nested `score` field (undo's response), both the
//! same shape.
function currentHalfStartMoment(score as Dictionary, period as Number?) as Time.Moment? {
    var key = currentHalfPeriodKey(period);
    if (key == null) {
        return null;
    }
    var periodTimes = score.get("period_times");
    if (periodTimes == null) {
        return null;
    }
    var iso = (periodTimes as Dictionary).get(key);
    if (iso == null) {
        return null;
    }
    return parseIso(iso as String);
}
