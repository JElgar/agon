import Toybox.Graphics;
import Toybox.WatchUi;
import Toybox.Timer;

class agonView extends WatchUi.View {

    //! Tick cadence for this screen's own timer — once a second, same as
    //! `ActivityStatsView`'s, so the current-half clock below reads live
    //! rather than only updating whenever some other event happens to
    //! redraw the screen. A plain `WatchUi.View` doesn't redraw on its
    //! own cadence the way a watch face does — `onPollTick` has to ask
    //! for it explicitly (`WatchUi.requestUpdate()`) every tick.
    const POLL_INTERVAL_MS = 1000;

    //! Every this many ticks (so every 5s), poll the server's
    //! authoritative score — the only way this device's own local tally
    //! (only ever updated by its *own* recordGoal/setPeriod calls) learns
    //! about a goal or period marker another device recorded. Same 5s
    //! cadence this screen always polled at; only the underlying timer's
    //! own interval changed, not how often the network actually gets hit.
    //! See `LiveApiClient.refreshScore`'s doc comment.
    const SERVER_POLL_EVERY_TICKS = 5;

    var _pollTimer as Timer.Timer or Null;
    var _tickCount as Number;

    function initialize() {
        View.initialize();
        _pollTimer = null;
        _tickCount = 0;
    }

    // Drawn manually in onUpdate below — a score line + a period label
    // doesn't need a static layout resource (resources/layouts/layout.xml
    // is still there, just unused now).
    function onLayout(dc as Dc) as Void {
    }

    // Called when this View is brought to the foreground. Restore
    // the state of this View and prepare it to be shown. This includes
    // loading resources into memory.
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
            // Keeps _lastSeq current proactively (another device's append
            // or undo) rather than only reactively, on this device's own
            // next append conflicting — see LiveApiClient.refreshSeq's
            // doc comment.
            getApp().liveApiClient.refreshSeq();
        }
        // Ticks the current-half clock below even on ticks that don't hit
        // the network.
        WatchUi.requestUpdate();
    }

    // Update the view
    function onUpdate(dc as Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();
        drawNotRecordingRing(dc);

        var score = getApp().score;
        var centerX = dc.getWidth() / 2;
        var centerY = dc.getHeight() / 2;

        dc.drawText(
            centerX, centerY - 30, Graphics.FONT_NUMBER_MEDIUM,
            score.homeGoals.toString() + " - " + score.awayGoals.toString(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );

        dc.drawText(
            centerX, centerY + 25, Graphics.FONT_SMALL,
            score.periodLabel(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );

        // Current-half clock — only while a half is actually running (see
        // FootballScore.currentHalfElapsedSeconds), so this is blank
        // rather than a stale/misleading "0:00" before kick-off, at
        // half-time, and after full-time. Sourced from the server's own
        // period-start timestamp, not this device's local recording clock
        // — see LiveApiClient.currentHalfStartMoment's doc comment on why.
        var halfSeconds = score.currentHalfElapsedSeconds();
        if (halfSeconds != null) {
            dc.drawText(
                centerX, centerY + 55, Graphics.FONT_XTINY,
                formatDuration(halfSeconds),
                Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
            );
        }

        dc.drawText(
            centerX, dc.getHeight() - 20, Graphics.FONT_XTINY,
            "Menu to score",
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    // Called when this View is removed from the screen. Save the
    // state of this View here. This includes freeing resources from
    // memory.
    function onHide() as Void {
        if (_pollTimer != null) {
            _pollTimer.stop();
            _pollTimer = null;
        }
    }

}
