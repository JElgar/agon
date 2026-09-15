import Toybox.Lang;
import Toybox.Time;

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
    //! The wall-clock start of the currently-running half, per the
    //! server's `period_times` (`LiveApiClient.currentHalfStartMoment`) —
    //! `null` whenever there's no half currently running. See
    //! `currentHalfElapsedSeconds`, which is what actually reads this.
    var currentHalfStartedAt as Time.Moment?;
    //! The server's raw wire period string (`score.get("period")`) as of
    //! the last update — unlike `period` above (which only recognizes the
    //! four periods this app's own menu can record, and leaves a `null`
    //! wire value alone rather than guess), this tracks *any* period the
    //! server reports, extra time included, purely so `applyServerState`
    //! can tell a genuine transition apart from a repeated poll of the
    //! same period. See `recordingActionForPeriodWire`, which is what
    //! actually reads this.
    var periodWire as String?;

    var _apiClient as LiveApiClient;

    function initialize(apiClient as LiveApiClient) {
        homeGoals = 0;
        awayGoals = 0;
        period = PERIOD_NOT_STARTED;
        hasServerState = false;
        currentHalfStartedAt = null;
        periodWire = null;
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
    //! Returns the recording action this update implies — `:start` for the
    //! kickoff of any half (normal or extra time), `:pause` for any
    //! half-time break (normal or extra time), `null` for anything else
    //! (no period marker, a terminal one like full-time, or a repeated
    //! poll of a period already seen). Callers apply it onto
    //! `ActivityRecorder` themselves (`LiveApiClient.onScore`) — same
    //! "every watch with this match open reacts the same way" idea the
    //! old kick-off-only version of this method already had, generalized
    //! to every period transition rather than just the first one.
    //!
    //! Never fires on the very first update (`!hasServerState`) — a match
    //! already under way (or already at half-time) when the watch opened
    //! it shows the red ring instead, and the wearer starts/resumes their
    //! activity by hand — nor when `newPeriodWire` repeats whatever was
    //! last seen (most polls: nothing changed). See
    //! `recordingActionForPeriodWire` for the wire-value -> action mapping
    //! itself, and `periodWire`'s own doc comment for why this compares
    //! against the raw wire string rather than `period`.
    //!
    //! `newCurrentHalfStartedAt` overwrites `currentHalfStartedAt`
    //! unconditionally (unlike `newPeriod`, which a `null` leaves alone) —
    //! it's already `null` exactly when there's no half running, straight
    //! from `LiveApiClient.currentHalfStartMoment`, so there's no separate
    //! "unknown, don't touch it" case to preserve here.
    function applyServerState(newHomeGoals as Number, newAwayGoals as Number, newPeriod as Number?, newPeriodWire as String?, newCurrentHalfStartedAt as Time.Moment?) as Symbol? {
        var previousPeriodWire = periodWire;
        homeGoals = newHomeGoals;
        awayGoals = newAwayGoals;
        if (newPeriod != null) {
            period = newPeriod;
        }
        currentHalfStartedAt = newCurrentHalfStartedAt;

        var periodChanged = hasServerState
            && newPeriodWire != null
            && !(newPeriodWire.equals(previousPeriodWire));
        if (newPeriodWire != null) {
            periodWire = newPeriodWire;
        }
        hasServerState = true;
        if (!periodChanged) {
            return null;
        }
        return recordingActionForPeriodWire(newPeriodWire);
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
    //! Doesn't return a recording action the way `applyServerState` does
    //! — undo only ever reflects *this* device's own most recent action
    //! (see `LiveApiClient`'s own doc comment on why), so this device
    //! already applied whatever pause/start effect that action implied
    //! directly, synchronously, when the wearer originally tapped it
    //! (`agonMenuDelegate`); undoing it doesn't reverse that here. Still
    //! updates `periodWire` (from the now-current `period`, via
    //! `_apiClient.periodWireValue` — `null` for `PERIOD_NOT_STARTED`,
    //! same as everywhere else that function is used) so a *later*
    //! `applyServerState` transition is compared against the right
    //! baseline rather than one this undo just made stale.
    function applyUndoState(newHomeGoals as Number, newAwayGoals as Number, newPeriod as Number?, newCurrentHalfStartedAt as Time.Moment?) as Void {
        homeGoals = newHomeGoals;
        awayGoals = newAwayGoals;
        period = (newPeriod != null) ? (newPeriod as Number) : PERIOD_NOT_STARTED;
        currentHalfStartedAt = newCurrentHalfStartedAt;
        periodWire = (newPeriod != null) ? _apiClient.periodWireValue(newPeriod as Number) : null;
    }

    //! Seconds elapsed in the current half, per `currentHalfStartedAt` —
    //! `null` when there's no half currently running (not started,
    //! half-time, full-time) or the server hasn't reported one yet. Can
    //! come back slightly negative right at kick-off (this device's own
    //! clock a touch behind the server's, or the two disagreeing by a
    //! second) — not clamped here; `formatDuration` (the only caller)
    //! guards against that instead.
    function currentHalfElapsedSeconds() as Number? {
        if (currentHalfStartedAt == null) {
            return null;
        }
        var elapsed = Time.now().subtract(currentHalfStartedAt as Time.Moment);
        return elapsed.value();
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
