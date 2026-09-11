import Toybox.Lang;
import Toybox.WatchUi;

class agonMenuDelegate extends WatchUi.MenuInputDelegate {

    function initialize() {
        MenuInputDelegate.initialize();
    }

    function onMenuItem(item as Symbol) as Void {
        var app = getApp();

        if (item == :goal) {
            // Side -> scorer -> assist, handled entirely in GoalFlow.mc —
            // it calls back into app.score.recordGoal itself once done.
            WatchUi.pushView(buildSideMenu(), new GoalSideMenuDelegate(), WatchUi.SLIDE_UP);
            return;
        } else if (item == :period_kick_off) {
            // The first "start of half" event this match sees is what
            // actually starts activity recording — not app launch, see
            // agonApp.onStart. ActivityRecorder.start() is a no-op if
            // already recording, so this is safe even if kick-off gets
            // logged more than once.
            app.activityRecorder.start();
            app.score.setPeriod(FootballScore.PERIOD_KICK_OFF);
        } else if (item == :period_half_time) {
            app.score.setPeriod(FootballScore.PERIOD_HALF_TIME);
        } else if (item == :period_second_half) {
            // Also "starts a half" — same idempotent start() as kick-off.
            app.activityRecorder.start();
            app.score.setPeriod(FootballScore.PERIOD_SECOND_HALF);
        } else if (item == :period_full_time) {
            app.score.setPeriod(FootballScore.PERIOD_FULL_TIME);
        } else if (item == :end_match) {
            // Explicit early stop — onStop() also calls this when the app
            // closes, so leaving the app is never required just to save
            // the recording.
            app.activityRecorder.stopAndSave();
        }

        // The score screen reads app.score fresh every onUpdate rather
        // than being told what changed — just ask it to redraw.
        WatchUi.requestUpdate();
    }

}
