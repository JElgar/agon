import Toybox.Lang;
import Toybox.WatchUi;

class agonMenuDelegate extends WatchUi.MenuInputDelegate {

    function initialize() {
        MenuInputDelegate.initialize();
    }

    function onMenuItem(item as Symbol) as Void {
        var app = getApp();

        if (item == :goal_home) {
            app.score.recordHomeGoal();
        } else if (item == :goal_away) {
            app.score.recordAwayGoal();
        } else if (item == :period_kick_off) {
            app.score.setPeriod(FootballScore.PERIOD_KICK_OFF);
        } else if (item == :period_half_time) {
            app.score.setPeriod(FootballScore.PERIOD_HALF_TIME);
        } else if (item == :period_second_half) {
            app.score.setPeriod(FootballScore.PERIOD_SECOND_HALF);
        } else if (item == :period_full_time) {
            app.score.setPeriod(FootballScore.PERIOD_FULL_TIME);
        } else if (item == :end_match) {
            // Explicit early stop (e.g. full-time) — onStop() also calls
            // this when the app closes, so leaving the app is never
            // required just to save the recording.
            app.activityRecorder.stopAndSave();
        }

        // The score screen reads app.score fresh every onUpdate rather
        // than being told what changed — just ask it to redraw.
        WatchUi.requestUpdate();
    }

}
