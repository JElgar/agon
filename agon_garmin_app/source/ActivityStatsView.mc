import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.Activity;
import Toybox.System;
import Toybox.Timer;

//! Shows the underlying Garmin activity recording's own live stats
//! (duration, distance, heart rate, calories) alongside the football
//! score — reached from the main menu (see `agonMenuDelegate.mc`),
//! pushed on top of `agonView` the same way that menu itself is.
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

    var _pollTimer as Timer.Timer?;

    function initialize() {
        View.initialize();
        _pollTimer = null;
    }

    function onLayout(dc as Dc) as Void {
    }

    function onShow() as Void {
        _pollTimer = new Timer.Timer();
        _pollTimer.start(method(:onPollTick), POLL_INTERVAL_MS, true);
    }

    function onPollTick() as Void {
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

        if (!getApp().activityRecorder.isRecording()) {
            // Kick-off (or second-half kick-off) is what actually starts
            // the underlying recording (see ActivityRecorder.start's own
            // doc comment) — before that there's nothing real to show.
            dc.drawText(
                dc.getWidth() / 2, dc.getHeight() / 2, Graphics.FONT_XTINY,
                "Not recording yet",
                Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
            );
            return;
        }

        var info = Activity.getActivityInfo();
        var centerX = dc.getWidth() / 2;
        var centerY = dc.getHeight() / 2;

        dc.drawText(
            centerX, centerY - 45, Graphics.FONT_NUMBER_MEDIUM,
            durationLabel(info.timerTime),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.drawText(
            centerX, centerY, Graphics.FONT_SMALL,
            distanceLabel(info.elapsedDistance),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.drawText(
            centerX, centerY + 30, Graphics.FONT_SMALL,
            heartRateLabel(info.currentHeartRate),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.drawText(
            centerX, centerY + 55, Graphics.FONT_XTINY,
            caloriesLabel(info.calories),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
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

    function caloriesLabel(calories as Number?) as String {
        if (calories == null) {
            return "-- kcal";
        }
        return (calories as Number).toString() + " kcal";
    }
}

//! Back returns to the still-open main menu this view was pushed on top
//! of (see `agonMenuDelegate.mc`'s `:activity_stats` handling) — the
//! default `BehaviorDelegate.onBack` every other pushed-on-top screen in
//! this app relies on already does exactly that, no override needed.
class ActivityStatsDelegate extends WatchUi.BehaviorDelegate {

    function initialize() {
        BehaviorDelegate.initialize();
    }
}
