using Toybox.Communications;
using Toybox.Lang;

//! Thin wrapper around `Communications.makeWebRequest` for the three
//! `agon_service` calls this app needs. Every call takes a `Method`
//! callback invoked as `callback.invoke(success as Boolean, responseCode
//! as Number, data as Dictionary?)` — uniform across all three so callers
//! (`AgonLiveScoringApp`, `ScoringView`) don't need per-endpoint response
//! handling.
//!
//! `Communications.makeWebRequest` auto-serializes a `Dictionary` `params`
//! argument to JSON when the request carries a `Content-Type:
//! application/json` header — that's relied on throughout rather than
//! hand-building JSON strings.
//!
//! One request in flight at a time per call kind (the callback field is
//! cleared as soon as it fires) — fine for this app's usage, which never
//! fires two pairing attempts or two flushes concurrently.
class AgonApiClient {

    var _baseUrl as String;
    var _onPairComplete as Lang.Method?;
    var _onSeqComplete as Lang.Method?;
    var _onAppendComplete as Lang.Method?;

    function initialize(baseUrl as String) {
        _baseUrl = baseUrl;
    }

    //! `POST /devices/pair` — exchange a pairing code (from
    //! `POST /devices/pairing-codes`, entered by the user via this app's
    //! settings) for a long-lived device access token. No `Authorization`
    //! header — the device has no token yet; that's the entire point of
    //! this endpoint. On success, `data` is `{"access_token": "..."}`.
    function pairDevice(code as String, callback as Lang.Method) as Void {
        _onPairComplete = callback;
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_POST,
            :headers => { "Content-Type" => "application/json" },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON,
        };
        Communications.makeWebRequest(
            _baseUrl + "/devices/pair",
            { "code" => code },
            options,
            method(:onPairResponse)
        );
    }

    function onPairResponse(responseCode as Number, data as Dictionary?) as Void {
        var cb = _onPairComplete;
        _onPairComplete = null;
        if (cb == null) {
            return;
        }
        cb.invoke(responseCode == 200, responseCode, data);
    }

    //! `GET /matches/:id/live/seq` — the match's current live-event log
    //! tip, i.e. the `expected_last_seq` a fresh device must seed itself
    //! with before its first append (starting from a bare `0` would 409 on
    //! any match that already has events from another scorer/device — see
    //! `EventQueue.lastAckedSeq`'s doc comment). On success, `data` is
    //! `{"last_seq": <Number>}`.
    function fetchLiveSeq(matchId as String, accessToken as String, callback as Lang.Method) as Void {
        _onSeqComplete = callback;
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => { "Authorization" => "Bearer " + accessToken },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON,
        };
        Communications.makeWebRequest(
            _baseUrl + "/matches/" + matchId + "/live/seq",
            {},
            options,
            method(:onSeqResponse)
        );
    }

    function onSeqResponse(responseCode as Number, data as Dictionary?) as Void {
        var cb = _onSeqComplete;
        _onSeqComplete = null;
        if (cb == null) {
            return;
        }
        cb.invoke(responseCode == 200, responseCode, data);
    }

    //! `POST /matches/:id/live/events` — append a batch
    //! (`EventQueue.nextBatch`) atomically. `expectedLastSeq` must be the
    //! tip this device last saw (`EventQueue.lastAckedSeq`); a `409`
    //! response means another device moved the log on since, and the
    //! caller should re-`fetchLiveSeq` and retry rather than assume the
    //! batch landed. On success, `data` is a `LiveScoreSnapshot`:
    //! `{"last_seq": <Number>, "score": {...}}`.
    function appendLiveEvents(
        matchId as String,
        accessToken as String,
        expectedLastSeq as Number,
        events as Array,
        callback as Lang.Method
    ) as Void {
        _onAppendComplete = callback;
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_POST,
            :headers => {
                "Content-Type" => "application/json",
                "Authorization" => "Bearer " + accessToken,
            },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON,
        };
        Communications.makeWebRequest(
            _baseUrl + "/matches/" + matchId + "/live/events",
            {
                "expected_last_seq" => expectedLastSeq,
                "events" => events,
            },
            options,
            method(:onAppendResponse)
        );
    }

    function onAppendResponse(responseCode as Number, data as Dictionary?) as Void {
        var cb = _onAppendComplete;
        _onAppendComplete = null;
        if (cb == null) {
            return;
        }
        cb.invoke(responseCode == 200, responseCode, data);
    }
}
