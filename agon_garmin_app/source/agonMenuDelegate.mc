import Toybox.Lang;
import Toybox.System;
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
    var recorder = getApp().activityRecorder;
    var menu = new WatchUi.Menu2({ :title => "Menu" });

    // Not recording (never started, or paused) is exactly when the red
    // ring is showing — so its fix goes first, as the item already
    // highlighted when the menu opens. While recording it drops below the
    // scoring items instead, keeping Goal one press away.
    if (!recorder.isRecording()) {
        menu.addItem(activityMenuItem(recorder));
    }

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

    if (recorder.isRecording()) {
        menu.addItem(activityMenuItem(recorder));
    }

    // Always offered — MatchContext's player roster is only ever loaded
    // once, when the match is first picked (MatchMenuDelegate.
    // onMatchDetails), so a sub added to the match afterwards otherwise
    // never reaches GoalFlow's scorer/assist menus for the rest of the
    // session without asking for it again. See LiveApiClient.
    // refreshRoster's own doc comment on why this isn't just polled
    // alongside the score.
    menu.addItem(new WatchUi.MenuItem("Refresh players", null, :refresh_roster, {}));

    // Only offered once LiveApiClient actually has a real seq to send —
    // see its own canUndo()/doc comment. Absent rather than disabled
    // while that's still being derived (e.g. right after picking this
    // match) or once nothing's been recorded yet, same as agon_ui's own
    // UndoLastEventButton hiding itself outright rather than graying out.
    if (getApp().liveApiClient.canUndo()) {
        menu.addItem(new WatchUi.MenuItem("Undo last", null, :undo_last, {}));
    }

    // Always available, regardless of period — whatever state the match
    // is in, the wearer can finish their activity (see EndMatchFlow.mc).
    menu.addItem(new WatchUi.MenuItem("End match", null, :end_match, {}));
    return menu;
}

//! Start / Pause / Resume — one item whose label and action follow the
//! recording's own state, like a native Garmin activity's start/stop
//! button. Handled as `:activity_toggle` in `agonMenuDelegate.onSelect`.
function activityMenuItem(recorder as ActivityRecorder) as WatchUi.MenuItem {
    var label = "Start activity";
    if (recorder.isRecording()) {
        label = "Pause activity";
    } else if (recorder.hasSession()) {
        label = "Resume activity";
    }
    return new WatchUi.MenuItem(label, null, :activity_toggle, {});
}

//! Opens the main menu on top of whichever match page is showing — the
//! score screen (`agonDelegate`) or the activity stats screen
//! (`ActivityStatsDelegate`). Both open it the same way, and the menu
//! pops back to whichever one it came from.
function openMainMenu() as Void {
    WatchUi.pushView(buildMainMenu(), new agonMenuDelegate(), WatchUi.SLIDE_UP);
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
            // pushView layers the side menu on top of this one (and
            // agonView below it) rather than replacing them, so Back
            // steps out one screen at a time instead of exiting straight
            // past the whole flow — see GoalFlow.mc's doc comment. Since
            // nothing is being popped here, this needs no explicit
            // popView, unlike every branch below.
            WatchUi.pushView(buildSideMenu(), new GoalSideMenuDelegate(), WatchUi.SLIDE_UP);
            return;
        } else if (item == :activity_toggle) {
            if (app.activityRecorder.isRecording()) {
                app.activityRecorder.pause();
            } else {
                // matchContext is already populated by now (MatchPickerView,
                // before this menu is reachable at all).
                app.activityRecorder.start(app.matchContext.matchName());
            }
        } else if (item == :period_kick_off) {
            // Starts this wearer's activity (and resets the half clock) —
            // the same thing every other watch with this match open does
            // once its next score poll sees this kick-off.
            app.activityRecorder.startForKickOff(app.matchContext.matchName());
            app.score.setPeriod(FootballScore.PERIOD_KICK_OFF);
        } else if (item == :period_half_time) {
            // markLap before pause, deliberately — it's a no-op once the
            // session isn't recording (see its own doc comment), so
            // pausing first would silently skip closing the first-half
            // lap. Then pauses this wearer's activity immediately, rather
            // than waiting for this device's own next score poll to
            // notice — the same thing every other watch with this match
            // open does once its next poll sees this half-time marker
            // (LiveApiClient.onScore).
            app.activityRecorder.markLap();
            app.activityRecorder.pause();
            app.score.setPeriod(FootballScore.PERIOD_HALF_TIME);
        } else if (item == :period_second_half) {
            // Resumes (or starts) this wearer's activity immediately, same
            // idea as kick-off/half-time above — every watch with this
            // match open follows via its own next poll. Before markLap,
            // deliberately — half-time now pauses (see above), and markLap
            // is a no-op while paused, so this has to resume recording
            // first or the half-time-break lap would never close.
            // startForKickOff already calls markHalfStart itself
            // (resetting the live clock's own baseline), so nothing else
            // needed here besides markLap.
            app.activityRecorder.startForKickOff(app.matchContext.matchName());
            app.activityRecorder.markLap();
            app.score.setPeriod(FootballScore.PERIOD_SECOND_HALF);
        } else if (item == :period_full_time) {
            // Closes the second-half lap explicitly, rather than leaving
            // it to whatever stop()/save() do implicitly whenever the
            // recording actually ends (immediately, if :end_match is
            // picked next, or later).
            app.activityRecorder.markLap();
            app.score.setPeriod(FootballScore.PERIOD_FULL_TIME);
        } else if (item == :end_match) {
            if (app.activityRecorder.hasSession()) {
                // Save/Discard choice, pushed on top of this menu (same
                // as :goal above) so Back from it steps back to this menu
                // rather than skipping past it.
                WatchUi.pushView(buildEndMatchMenu(), new EndMatchMenuDelegate(), WatchUi.SLIDE_UP);
            } else {
                // No activity was ever started — nothing to save or
                // discard, so just leave.
                System.exit();
            }
            return;
        } else if (item == :undo_last) {
            // Fire-and-forget, same as recordGoal/recordPeriod — see
            // LiveApiClient.undoLast's own doc comment on why this has
            // no confirmation step and no retry on failure.
            app.liveApiClient.undoLast();
        } else if (item == :refresh_roster) {
            // Fire-and-forget too — a buzz on success (LiveApiClient.
            // onRoster) is the only feedback; there's no on-watch error
            // state to show if it fails, same "basics only" scope as
            // undo above.
            app.liveApiClient.refreshRoster();
        }

        // Unlike the legacy WatchUi.Menu (which popped itself once
        // onMenuItem returned), Menu2's onSelect doesn't dismiss anything
        // on its own. This menu was pushed on top of a match page (see
        // openMainMenu), so every branch that falls through to here has
        // to pop back to it explicitly.
        WatchUi.popView(WatchUi.SLIDE_DOWN);

        // The match pages read app state fresh every onUpdate rather than
        // being told what changed — just ask for a redraw.
        WatchUi.requestUpdate();
    }

}
