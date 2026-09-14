import Toybox.Lang;
import Toybox.WatchUi;

//! Built dynamically (not from a menu.xml resource — see
//! resources/menus/menu.xml, now unused) because which actions make sense
//! depends on the match's current period: no goals before kick-off or
//! during half-time, no starting the second half before the first one has
//! ended, etc. Each period state maps to exactly one set of legal items,
//! so illegal transitions (start the second half from PERIOD_NOT_STARTED,
//! say) are simply never offered rather than needing to be rejected.
//!
//! Uses `WatchUi.Menu2`, not the legacy `WatchUi.Menu` this used
//! originally — see `MatchPickerView.mc`'s doc comment for the
//! screenshot-confirmed round-screen clipping bug that motivated the
//! switch across the whole app. `Menu2` labels here are all short/fixed
//! so clipping was never actually observed on this particular menu, but
//! it's converted for consistency (and so any future item added here
//! doesn't quietly reintroduce the bug).
function buildMainMenu() as WatchUi.Menu2 {
    var period = getApp().score.period;
    var menu = new WatchUi.Menu2({ :title => "Menu" });

    if (period == FootballScore.PERIOD_NOT_STARTED) {
        menu.addItem(new WatchUi.MenuItem("Kick-off", null, :period_kick_off, {}));
    } else if (period == FootballScore.PERIOD_KICK_OFF) {
        menu.addItem(new WatchUi.MenuItem("Goal", null, :goal, {}));
        menu.addItem(new WatchUi.MenuItem("Half-time", null, :period_half_time, {}));
    } else if (period == FootballScore.PERIOD_HALF_TIME) {
        menu.addItem(new WatchUi.MenuItem("2nd half kick-off", null, :period_second_half, {}));
    } else if (period == FootballScore.PERIOD_SECOND_HALF) {
        menu.addItem(new WatchUi.MenuItem("Goal", null, :goal, {}));
        menu.addItem(new WatchUi.MenuItem("Full-time", null, :period_full_time, {}));
    }
    // PERIOD_FULL_TIME: nothing sport-specific left to record.

    // Always available, regardless of period — worth checking even
    // before kick-off (GPS lock, say), and there's no reason to hide it
    // once recording's started either.
    menu.addItem(new WatchUi.MenuItem("Activity stats", null, :activity_stats, {}));

    // Always available, regardless of period — a safety valve to stop
    // and save the recording whatever state the match is in.
    menu.addItem(new WatchUi.MenuItem("End match (save activity)", null, :end_match, {}));
    return menu;
}

class agonMenuDelegate extends WatchUi.Menu2InputDelegate {

    function initialize() {
        Menu2InputDelegate.initialize();
    }

    function onSelect(menuItem as WatchUi.MenuItem) as Void {
        var app = getApp();
        var item = menuItem.getId();

        if (item == :goal) {
            // Side -> scorer -> assist, handled entirely in GoalFlow.mc —
            // it calls back into app.score.recordGoal itself once done.
            // switchToView replaces this pushed menu (and agonView below
            // it) outright, so — unlike every branch below — this one
            // needs no explicit popView.
            WatchUi.switchToView(buildSideMenu(), new GoalSideMenuDelegate(), WatchUi.SLIDE_UP);
            return;
        } else if (item == :activity_stats) {
            // Pushed on top of this still-open menu (not popped first) —
            // Back from the stats screen (ActivityStatsDelegate's default
            // BehaviorDelegate handling, no override needed) lands back
            // on the menu, and Back again on agonView, same nesting
            // agonDelegate.openMenu already layers this menu on top of.
            WatchUi.pushView(new ActivityStatsView(), new ActivityStatsDelegate(), WatchUi.SLIDE_LEFT);
            return;
        } else if (item == :period_kick_off) {
            // The first "start of half" event this match sees is what
            // actually starts activity recording — not app launch, see
            // agonApp.onStart. ActivityRecorder.start() is a no-op if
            // already recording, so this is safe even if kick-off gets
            // logged more than once.
            app.activityRecorder.start();
            app.activityRecorder.markHalfStart();
            app.score.setPeriod(FootballScore.PERIOD_KICK_OFF);
        } else if (item == :period_half_time) {
            // Closes the first-half lap and opens a new one for the
            // break — see ActivityRecorder.markLap's doc comment.
            app.activityRecorder.markLap();
            app.score.setPeriod(FootballScore.PERIOD_HALF_TIME);
        } else if (item == :period_second_half) {
            // Also "starts a half" — same idempotent start() as kick-off.
            app.activityRecorder.start();
            // Closes the half-time-break lap and opens the second-half
            // one, same as markHalfStart resets the live clock's own
            // baseline right below.
            app.activityRecorder.markLap();
            app.activityRecorder.markHalfStart();
            app.score.setPeriod(FootballScore.PERIOD_SECOND_HALF);
        } else if (item == :period_full_time) {
            // Closes the second-half lap explicitly, rather than leaving
            // it to whatever stop()/save() do implicitly whenever the
            // recording actually ends (immediately, if :end_match is
            // picked next, or later).
            app.activityRecorder.markLap();
            app.score.setPeriod(FootballScore.PERIOD_FULL_TIME);
        } else if (item == :end_match) {
            // Explicit early stop — onStop() also calls this when the app
            // closes, so leaving the app is never required just to save
            // the recording.
            app.activityRecorder.stopAndSave();
        }

        // Unlike the legacy WatchUi.Menu (which popped itself once
        // onMenuItem returned), Menu2's onSelect doesn't dismiss anything
        // on its own. This menu was pushed on top of agonView (see
        // agonDelegate.openMenu), so every branch that falls through to
        // here has to pop back to it explicitly.
        WatchUi.popView(WatchUi.SLIDE_DOWN);

        // The score screen reads app.score fresh every onUpdate rather
        // than being told what changed — just ask it to redraw.
        WatchUi.requestUpdate();
    }

}
