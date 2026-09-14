import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.Activity;
import Toybox.System;
import Toybox.Timer;

//! Shows the underlying Garmin activity recording's own live stats
//! (current-half time, total time, distance, heart rate) alongside the
//! football score — the second match page, paged to with up/down from
//! the score screen (see `agonDelegate.onNextPage`), the way a native
//! Garmin activity cycles its data screens.
//!
//! There's no prebuilt "activity data" widget to reuse here: `Toybox.
//! WatchUi.SimpleDataField`/`DataField` — Garmin's own classes for
//! exactly this — are locked to the separate `datafield` app type
//! declared in manifest.xml, unusable from a `watchApp`-type project
//! like this one (confirmed from the Connect IQ docs; a `DataField`
//! isn't even a `WatchUi.View` subclass pushable into a normal view
//! stack). What every data field actually reads from — `Activity.
//! getActivityInfo()` — is the same call this view makes directly, then
//! draws by hand, the same `Dc.drawText` approach already used for the
//! score screen.
class ActivityStatsView extends WatchUi.View {

    //! Once per second, matching the cadence a real Data Field's own
    //! `compute()` is called at — these numbers only need to look live,
    //! not be redrawn any faster than a human can read them.
    const POLL_INTERVAL_MS = 1000;

    //! Every this many ticks (so every 5s, `agonView.POLL_INTERVAL_MS`'s
    //! cadence) also poll the server's score and seq. This is a full match
    //! page now, not a brief detour off the menu, so it can stay up for a
    //! whole half — and without polling, the score shown here and the
    //! period-driven menu items would go stale while it does.
    const SERVER_POLL_EVERY_TICKS = 5;

    var _pollTimer as Timer.Timer?;
    var _tickCount as Number;

    function initialize() {
        View.initialize();
        _pollTimer = null;
        _tickCount = 0;
    }

    function onLayout(dc as Dc) as Void {
    }

    function onShow() as Void {
        getApp().liveApiClient.refreshScore();
        _tickCount = 0;
        _pollTimer = new Timer.Timer();
        _pollTimer.start(method(:onPollTick), POLL_INTERVAL_MS, true);
    }

    function onPollTick() as Void {
        _tickCount += 1;
        if (_tickCount % SERVER_POLL_EVERY_TICKS == 0) {
            getApp().liveApiClient.refreshScore();
            getApp().liveApiClient.refreshSeq();
        }
        WatchUi.requestUpdate();
    }

    function onHide() as Void {
        if (_pollTimer != null) {
            _pollTimer.stop();
            _pollTimer = null;
        }
    }

    function onUpdate(dc as Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();
        drawNotRecordingRing(dc);

        if (!getApp().activityRecorder.hasSession()) {
            // Nothing's been recorded to show until the activity starts —
            // at kick-off, or from the menu (see ActivityRecorder).
            // A paused activity falls through instead: its stats are real,
            // just frozen, and the red ring already says it's paused.
            dc.drawText(
                dc.getWidth() / 2, dc.getHeight() / 2, Graphics.FONT_XTINY,
                "Activity not started",
                Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
            );
            return;
        }

        var info = Activity.getActivityInfo();
        var centerX = dc.getWidth() / 2;
        var centerY = dc.getHeight() / 2;

        // Score first, small — agonView is the primary place to read it,
        // this is just "while I'm here".
        dc.drawText(
            centerX, centerY - 70, Graphics.FONT_XTINY,
            scoreLabel(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        // Current-half time is the primary stat on this screen — a
        // running "how long has this half been going" clock, not the
        // whole match's timerTime (see ActivityRecorder.
        // currentHalfTimerTimeMs's own doc comment).
        dc.drawText(
            centerX, centerY - 30, Graphics.FONT_NUMBER_MEDIUM,
            durationLabel(getApp().activityRecorder.currentHalfTimerTimeMs()),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        // Total match time, small — secondary to the current-half clock
        // above, not the other way around.
        dc.drawText(
            centerX, centerY + 5, Graphics.FONT_XTINY,
            "Total " + durationLabel(info.timerTime),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.drawText(
            centerX, centerY + 35, Graphics.FONT_SMALL,
            distanceLabel(info.elapsedDistance),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.drawText(
            centerX, centerY + 62, Graphics.FONT_XTINY,
            heartRateLabel(info.currentHeartRate),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    function scoreLabel() as String {
        var score = getApp().score;
        return score.homeGoals.toString() + " - " + score.awayGoals.toString();
    }

    //! `null` (no GPS fix / timer not actually ticking yet) shows as
    //! "--:--" rather than a misleading 0:00.
    function durationLabel(timerTimeMs as Number?) as String {
        if (timerTimeMs == null) {
            return "--:--";
        }
        var totalSeconds = (timerTimeMs as Number) / 1000;
        var hours = totalSeconds / 3600;
        var minutes = (totalSeconds / 60) % 60;
        var seconds = totalSeconds % 60;
        if (hours > 0) {
            return hours.toString() + ":" + minutes.format("%02d") + ":" + seconds.format("%02d");
        }
        return minutes.toString() + ":" + seconds.format("%02d");
    }

    //! Meters -> the device's own configured unit (see
    //! `System.DeviceSettings.distanceUnits`) — never hardcoded, since
    //! this app's product list spans plenty of watches sold in
    //! statute-unit markets.
    function distanceLabel(elapsedDistanceMeters as Float?) as String {
        if (elapsedDistanceMeters == null) {
            return "-- km";
        }
        var meters = elapsedDistanceMeters as Float;
        var isStatute = System.getDeviceSettings().distanceUnits == System.UNIT_STATUTE;
        if (isStatute) {
            var miles = meters / 1609.34;
            return miles.format("%.2f") + " mi";
        }
        var km = meters / 1000.0;
        return km.format("%.2f") + " km";
    }

    function heartRateLabel(currentHeartRate as Number?) as String {
        if (currentHeartRate == null) {
            return "-- bpm";
        }
        return (currentHeartRate as Number).toString() + " bpm";
    }
}

//! Same controls as the score screen's `agonDelegate`: up/down pages back
//! to it, and menu/select open the same main menu. Back isn't overridden
//! — both pages sit at the same depth in the view stack, so it does the
//! same thing from here as from the score screen.
class ActivityStatsDelegate extends WatchUi.BehaviorDelegate {

    function initialize() {
        BehaviorDelegate.initialize();
    }

    function onMenu() as Boolean {
        openMainMenu();
        return true;
    }

    function onSelect() as Boolean {
        openMainMenu();
        return true;
    }

    function onNextPage() as Boolean {
        WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_UP);
        return true;
    }

    function onPreviousPage() as Boolean {
        WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_DOWN);
        return true;
    }
}
