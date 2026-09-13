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
class LiveApiClient {

    var _matchId as String?;
    //! The seq this device last saw for `_matchId` — 0 means "never
    //! synced", the same convention `AppendLiveEventsInput.
    //! expected_last_seq` itself uses. Seeded from
    //! `GET /matches/:id/live/seq` in `setMatch`, then kept in step from
    //! each append response's own returned `last_seq`.
    var _lastSeq as Number;
    var _apiClient as MatchApiClient;

    function initialize() {
        _matchId = null;
        _lastSeq = 0;
        _apiClient = new MatchApiClient();
    }

    //! Start scoring `matchId` — called once, from MatchPickerView, right
    //! after `MatchContext` is populated for the same match.
    function setMatch(matchId as String) as Void {
        _matchId = matchId;
        _lastSeq = 0;
        var url = API_BASE_URL + "/matches/" + matchId + "/live/seq";
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        Communications.makeWebRequest(url, null, options, method(:onSeq));
        // Load whatever's already been scored (by this device on a
        // previous visit, or another device entirely) rather than
        // starting the screen from a misleading 0-0.
        refreshScore();
    }

    function onSeq(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var seq = dict.get("last_seq");
            if (seq != null) {
                _lastSeq = seq as Number;
            }
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
        Communications.makeWebRequest(url, body, options, method(:onAppendResponse));
    }

    function onAppendResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var seq = dict.get("last_seq");
            if (seq != null) {
                _lastSeq = seq as Number;
            }
            return;
        }
        if (responseCode == 409 && _matchId != null) {
            // Another writer moved the log on since we last synced (or
            // this device just hasn't seeded expected_last_seq correctly
            // yet). Re-fetch the real tip so the *next* action succeeds —
            // this specific event is simply dropped rather than retried,
            // matching the app's existing "basics only" scope (no offline
            // queue yet — see docs/garmin-live-scoring.md) — and refresh
            // the on-screen score too, since a conflict means whatever
            // just happened elsewhere is exactly the kind of update the
            // local tally alone would otherwise never learn about.
            var url = API_BASE_URL + "/matches/" + (_matchId as String) + "/live/seq";
            var options = {
                :method => Communications.HTTP_REQUEST_METHOD_GET,
                :headers => { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() },
                :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
            };
            Communications.makeWebRequest(url, null, options, method(:onSeq));
            refreshScore();
        }
        // Any other outcome (network error, 403 not-an-admin, ...): there's
        // no on-watch error UI for this yet, so it's silently dropped —
        // same "basics only" scope as above.
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
        Communications.makeWebRequest(url, null, options, method(:onScore));
    }

    function onScore(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode != 200 || data == null) {
            // 404 (no score recorded yet — a brand new match) or a
            // network error: nothing to apply, leave the current tally
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
        getApp().score.applyServerState(homeGoals, awayGoals, periodFromScore(score));
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
