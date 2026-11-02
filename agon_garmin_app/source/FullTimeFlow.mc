import Toybox.Lang;
import Toybox.WatchUi;

//! The decision menu offered the moment full-time is marked: finish the
//! match, or (only when the match's format actually allows it —
//! `MatchContext.hasExtraTime`) keep scoring into extra time. Pushed
//! straight on top of the main menu by `agonMenuDelegate`'s
//! `:period_full_time` branch, right after it records the `full_time`
//! period marker itself — same idea as `GoalFlow`'s side -> scorer -> assist
//! chain (a flow menu built programmatically, since which items make sense
//! depends on match state no menu.xml resource could know).
//!
//! "Keep scoring" is a placeholder today: the watch app has no extra-time
//! period buttons yet (`agonMenuDelegate.buildMainMenu` only ever offers
//! the four normal-time periods — see its own doc comment), so there's
//! nothing real for it to do beyond backing out. It's offered anyway
//! (rather than left out until extra-time support exists) so the decision
//! itself — finish, or not — is visible at the moment it matters; a wearer
//! who picks it lands back on the score screen with the main menu's
//! "Finish match" item (still offered whenever period is
//! `PERIOD_FULL_TIME`) as the way back into this same choice.

function buildFullTimeMenu() as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => "Full-time" });
    menu.addItem(new WatchUi.MenuItem("Finish match", null, :finish, {}));
    if (getApp().matchContext.hasExtraTime) {
        menu.addItem(new WatchUi.MenuItem("Keep scoring", null, :keep_scoring, {}));
    }
    return menu;
}

class FullTimeMenuDelegate extends WatchUi.Menu2InputDelegate {

    function initialize() {
        Menu2InputDelegate.initialize();
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        if (item.getId() == :finish) {
            // Fire-and-forget, same as every other LiveApiClient action —
            // see finishMatch's own doc comment on feedback/retry.
            getApp().liveApiClient.finishMatch();
        }
        // :keep_scoring falls through to the same place :finish's result
        // does — nothing more to do until real extra-time support exists
        // (see this file's doc comment).

        // Collapses this flow and the main menu beneath it in one step,
        // same mechanism GoalFlow's final screen uses — there's nothing
        // left to step back into either way.
        WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_DOWN);
    }
}
