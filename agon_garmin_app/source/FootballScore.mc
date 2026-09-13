import Toybox.Lang;

//! In-memory football score state for this session: goal tally per side
//! plus the current match period. Every change is echoed to a
//! `MockApiClient` (see its doc comment) — not persisted anywhere yet, and
//! not the server's `Score` (`GET /matches/:id/score`), which will be the
//! real source of truth once this talks to `agon_service` for real. This
//! is deliberately just "local state + a menu to change it" — the basics.
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

    var _apiClient as MockApiClient;

    function initialize(apiClient as MockApiClient) {
        homeGoals = 0;
        awayGoals = 0;
        period = PERIOD_NOT_STARTED;
        _apiClient = apiClient;
    }

    //! `scorer`/`assist` are `null` when skipped (the on-watch flow's
    //! "Unknown"/"No assist" options — see GoalFlow.mc) — a goal doesn't
    //! require picking a player, same as the backend's
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
        _apiClient.recordPeriod(periodLabel());
    }

    //! Short, on-watch label for the current period — also what gets
    //! logged to the mock API client, so the two never disagree.
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
