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
//! Laid out as a boxed 2x2 grid with thin divider lines under a small
//! score/period header — the same "gridded fields" look a stock Garmin
//! running/multisport activity's own data screens use, rather than this
//! view's original single column of centered lines. A quarter of the
//! content area per field means each one can afford a much bigger value
//! than five lines stacked on one screen ever could.
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

    //! Fraction of the screen's shorter side the boxed content area
    //! occupies on a round/semi-round watch, so every corner of the 2x2
    //! grid below stays clear of the bezel — the same inscribed-square
    //! idea `drawNotRecordingRing` already applies to the ring drawn on
    //! top of this. Unused on a rectangular watch — see `contentBounds`.
    const ROUND_CONTENT_FRACTION = 0.74;
    //! The margin kept clear of a rectangular screen's own edges instead.
    const RECT_CONTENT_MARGIN = 14;
    //! Fraction of the content area's height given to the score/period
    //! header above the stat grid.
    const HEADER_FRACTION = 0.24;

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
        var bounds = contentBounds(dc);
        var x = bounds[0];
        var y = bounds[1];
        var w = bounds[2];
        var h = bounds[3];
        var headerHeight = (h * HEADER_FRACTION).toNumber();
        var gridY = y + headerHeight;
        var gridHeight = h - headerHeight;

        drawHeader(dc, x, y, w, headerHeight);

        dc.setColor(Graphics.COLOR_DK_GRAY, Graphics.COLOR_TRANSPARENT);
        dc.drawLine(x, gridY, x + w, gridY);
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_TRANSPARENT);

        drawStatGrid(dc, x, gridY, w, gridHeight, info);
    }

    //! Top-left + size ([x, y, width, height]) of the square/rect the
    //! header and stat grid draw inside — inset clear of the bezel on a
    //! round watch (`ROUND_CONTENT_FRACTION`), or just a fixed margin on a
    //! rectangular one. Mirrors the same `screenShape` check
    //! `drawNotRecordingRing` makes right before this runs.
    function contentBounds(dc as Dc) as Array<Number> {
        var width = dc.getWidth();
        var height = dc.getHeight();
        if (System.getDeviceSettings().screenShape == System.SCREEN_SHAPE_RECTANGLE) {
            var m = RECT_CONTENT_MARGIN;
            return [m, m, width - 2 * m, height - 2 * m];
        }
        var shorterSide = width;
        if (height < shorterSide) {
            shorterSide = height;
        }
        var side = (shorterSide * ROUND_CONTENT_FRACTION).toNumber();
        return [(width - side) / 2, (height - side) / 2, side, side];
    }

    //! Score first, then period, both small — `agonView` is the primary
    //! place to read either, this is just "while I'm here".
    function drawHeader(dc as Dc, x as Number, y as Number, w as Number, h as Number) as Void {
        var cx = x + w / 2;
        var scoreY = y + (h * 30) / 100;
        var periodY = y + (h * 75) / 100;

        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_TRANSPARENT);
        dc.drawText(
            cx, scoreY, Graphics.FONT_XTINY, scoreLabel(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        dc.setColor(Graphics.COLOR_LT_GRAY, Graphics.COLOR_TRANSPARENT);
        dc.drawText(
            cx, periodY, Graphics.FONT_XTINY, getApp().score.periodLabel(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    //! The four boxed fields: current-half time, total time, distance and
    //! heart rate — divided by grid lines the same way a stock Garmin
    //! data screen's own fields are. Order is reading order: time (this
    //! screen's original primary stat) top, effort/distance bottom.
    function drawStatGrid(dc as Dc, x as Number, y as Number, w as Number, h as Number, info as Activity.Info) as Void {
        var halfW = w / 2;
        var halfH = h / 2;
        var rightW = w - halfW;
        var bottomH = h - halfH;

        dc.setColor(Graphics.COLOR_DK_GRAY, Graphics.COLOR_TRANSPARENT);
        dc.drawLine(x, y + halfH, x + w, y + halfH);
        dc.drawLine(x + halfW, y, x + halfW, y + h);

        drawTimeBox(dc, x, y, halfW, halfH, "HALF", durationLabel(getApp().activityRecorder.currentHalfTimerTimeMs()));
        drawTimeBox(dc, x + halfW, y, rightW, halfH, "TOTAL", durationLabel(info.timerTime));
        drawTextBox(dc, x, y + halfH, halfW, bottomH, "DIST", distanceLabel(info.elapsedDistance));
        drawTextBox(dc, x + halfW, y + halfH, rightW, bottomH, "HR", heartRateLabel(info.currentHeartRate));
    }

    //! A boxed field whose value is pure digits/colon (mm:ss) — safe to
    //! render in a number font (`Graphics.FONT_NUMBER_MILD`), which reads
    //! much bigger and bolder than a regular text font at the same box
    //! size, the same "hero number" look a native activity's time/
    //! distance fields use.
    function drawTimeBox(dc as Dc, x as Number, y as Number, w as Number, h as Number, label as String, value as String) as Void {
        drawBoxLabel(dc, x, y, w, h, label);
        var cx = x + w / 2;
        var valueY = y + (h * 65) / 100;
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_TRANSPARENT);
        dc.drawText(
            cx, valueY, Graphics.FONT_NUMBER_MILD, value,
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    //! A boxed field whose value has real letters in it (a unit suffix —
    //! "km"/"mi"/"bpm"). Garmin's number fonts only have glyphs for
    //! digits/colon/decimal point, so these two need a regular text font
    //! instead — same reason this view's very first version used
    //! `FONT_SMALL`/`FONT_XTINY` rather than a number font for these.
    function drawTextBox(dc as Dc, x as Number, y as Number, w as Number, h as Number, label as String, value as String) as Void {
        drawBoxLabel(dc, x, y, w, h, label);
        var cx = x + w / 2;
        var valueY = y + (h * 65) / 100;
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_TRANSPARENT);
        dc.drawText(
            cx, valueY, Graphics.FONT_MEDIUM, value,
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    //! The small caps label near the top of a boxed field — shared by
    //! `drawTimeBox`/`drawTextBox` so both draw it identically.
    function drawBoxLabel(dc as Dc, x as Number, y as Number, w as Number, h as Number, label as String) as Void {
        var cx = x + w / 2;
        var labelY = y + (h * 24) / 100;
        dc.setColor(Graphics.COLOR_LT_GRAY, Graphics.COLOR_TRANSPARENT);
        dc.drawText(
            cx, labelY, Graphics.FONT_XTINY, label,
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
//! to it, and menu/select open the same main menu. Also swallows Back the
//! same way `agonDelegate` does — both pages sit at the same depth in the
//! view stack (nothing underneath, reached via switchToView), so an
//! unoverridden Back would exit the app here too.
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

    function onBack() as Boolean {
        return true;
    }
}
