import Toybox.Lang;

//! In-memory football score state for this session: goal tally per side
//! plus the current match period. Every change is posted to `LiveApiClient`
//! (real `POST /matches/:id/live/events` calls now, once a match has been
//! picked — see MatchPickerView) — this tally itself isn't persisted or
//! read back from anywhere; the server's `Score` (`GET /matches/:id/score`)
//! is the real source of truth. This is deliberately just "local state + a
//! menu to change it" — the basics.
class FootballScore {

    enum {
        PERIOD_NOT_STARTED,
        PERIOD_KICK_OFF,
        PERIOD_HALF_TIME,
        PERIOD_SECOND_HALF,
        PERIOD_FULL_TIME,
    }

    var homeGoals as Number;
    var awayGoals as Number;
    var period as Number;
    //! `false` until this device has learnt the server's score state for
    //! the match at least once — a real score, or a 404 meaning nothing's
    //! been recorded yet (`markNoServerScore`). `applyServerState`'s return
    //! value needs it to tell "already under way when this watch opened
    //! the match" apart from "kicked off while it was open".
    var hasServerState as Boolean;

    var _apiClient as LiveApiClient;

    function initialize(apiClient as LiveApiClient) {
        homeGoals = 0;
        awayGoals = 0;
        period = PERIOD_NOT_STARTED;
        hasServerState = false;
        _apiClient = apiClient;
    }

    //! `scorer`/`assist` are player ids, `null` when skipped (the on-watch
    //! flow's "Unknown"/"No assist" options — see GoalFlow.mc) — a goal
    //! doesn't require picking a player, same as the backend's
    //! `FootballGoalEvent.scorer_player_id`/`assist_player_id` being
    //! optional.
    function recordGoal(side as Symbol, scorer as String?, assist as String?) as Void {
        if (side == :home) {
            homeGoals += 1;
        } else {
            awayGoals += 1;
        }
        _apiClient.recordGoal(side, scorer, assist);
    }

    function setPeriod(newPeriod as Number) as Void {
        period = newPeriod;
        // The period Number itself, not periodLabel()'s display text —
        // LiveApiClient maps it to the wire value the server expects.
        _apiClient.recordPeriod(newPeriod);
    }

    //! Overwrite this tally with the server's authoritative one — called
    //! by `LiveApiClient.refreshScore()` when polling
    //! `GET /matches/:id/score`, the only way this device learns about a
    //! goal or period marker some *other* device recorded (recordGoal/
    //! setPeriod above only ever reflect this device's own actions).
    //! `newPeriod` is `null` when the server hasn't seen a period marker
    //! at all yet, or sent a wire value this app doesn't recognize
    //! (extra time, penalties — not offered on this app's menu) — either
    //! way, leaving `period` as this device's own last-known value is
    //! safer than guessing.
    //!
    //! Returns `true` when this update is the match kicking off while this
    //! watch had it open: the period moved out of not-started (to anything
    //! but full-time) *after* the server's state had already been seen
    //! once. The first update never counts — a match already under way
    //! when the watch opened it shows the red ring instead, and the wearer
    //! starts their activity by hand. See `ActivityRecorder.startForKickOff`.
    function applyServerState(newHomeGoals as Number, newAwayGoals as Number, newPeriod as Number?) as Boolean {
        var previousPeriod = period;
        homeGoals = newHomeGoals;
        awayGoals = newAwayGoals;
        if (newPeriod != null) {
            period = newPeriod;
        }
        var kickedOffWhileOpen = hasServerState
            && previousPeriod == PERIOD_NOT_STARTED
            && period != PERIOD_NOT_STARTED
            && period != PERIOD_FULL_TIME;
        hasServerState = true;
        return kickedOffWhileOpen;
    }

    //! The server has no score for this match yet (`GET /matches/:id/score`
    //! 404s until the first live event) — which still counts as learning
    //! its state: not started. Without this, a brand-new match's first
    //! real score (its kick-off) would be mistaken for the state on
    //! opening, and wouldn't auto-start anyone's activity.
    function markNoServerScore() as Void {
        hasServerState = true;
    }

    //! Same shape as `applyServerState`, but for `LiveApiClient.undoLast`
    //! specifically — unlike that one, `newPeriod == null` here IS
    //! trusted, as `PERIOD_NOT_STARTED`. Safe only because undo's own
    //! caller guarantees this: it never undoes anything but the last
    //! event *this device itself* just appended (see `LiveApiClient`'s
    //! own doc comment on why), so a `null` period back from the server
    //! unambiguously means "the period marker that was just undone was
    //! the log's only one" — not the extra-time/penalties ambiguity
    //! `applyServerState` has to hedge against when polling.
    function applyUndoState(newHomeGoals as Number, newAwayGoals as Number, newPeriod as Number?) as Void {
        homeGoals = newHomeGoals;
        awayGoals = newAwayGoals;
        period = (newPeriod != null) ? (newPeriod as Number) : PERIOD_NOT_STARTED;
    }

    //! Short, on-watch label for the current period.
    function periodLabel() as String {
        if (period == PERIOD_NOT_STARTED) {
            return "Not started";
        } else if (period == PERIOD_KICK_OFF) {
            return "1st half";
        } else if (period == PERIOD_HALF_TIME) {
            return "Half-time";
        } else if (period == PERIOD_SECOND_HALF) {
            return "2nd half";
        } else if (period == PERIOD_FULL_TIME) {
            return "Full-time";
        }
        return "";
    }
}
