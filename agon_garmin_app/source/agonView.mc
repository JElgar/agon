import Toybox.Graphics;
import Toybox.WatchUi;
import Toybox.Timer;

class agonView extends WatchUi.View {

    //! How often to poll the server's authoritative score while this
    //! screen is visible — the only way this device's own local tally
    //! (only ever updated by its *own* recordGoal/setPeriod calls) learns
    //! about a goal or period marker another device recorded. See
    //! `LiveApiClient.refreshScore`'s doc comment.
    const POLL_INTERVAL_MS = 5000;

    var _pollTimer as Timer.Timer or Null;

    function initialize() {
        View.initialize();
        _pollTimer = null;
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
        _pollTimer = new Timer.Timer();
        _pollTimer.start(method(:onPollTick), POLL_INTERVAL_MS, true);
    }

    function onPollTick() as Void {
        getApp().liveApiClient.refreshScore();
        // Keeps _lastSeq current proactively (another device's append or
        // undo) rather than only reactively, on this device's own next
        // append conflicting — see LiveApiClient.refreshSeq's doc comment.
        getApp().liveApiClient.refreshSeq();
    }

    // Update the view
    function onUpdate(dc as Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();

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
