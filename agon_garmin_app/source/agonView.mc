import Toybox.Graphics;
import Toybox.WatchUi;

class agonView extends WatchUi.View {

    function initialize() {
        View.initialize();
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
    }

}
