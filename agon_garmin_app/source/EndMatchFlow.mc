import Toybox.Lang;
import Toybox.System;
import Toybox.WatchUi;

//! "End match" -> keep or throw away this watch's recorded activity, then
//! leave the app: the same Save/Discard choice a native Garmin activity
//! offers once it's finished. Only reached when there *is* an activity to
//! decide about — `agonMenuDelegate`'s `:end_match` exits straight away
//! when nothing was ever started.
//!
//! Ending the match here is about this watch's recording only. Nothing is
//! posted to the live score (Full-time is its own menu item), so another
//! device still scoring the same match is unaffected.
//!
//! Leaves via `System.exit()` rather than going back to the score screen,
//! the way a finished native activity hands back to the watch face.
//! `agonApp.onStop`'s save-on-exit safety net is then a no-op, the session
//! having already been saved or discarded here. Back from this menu
//! cancels: the activity is left exactly as it was, recording or paused.

function buildEndMatchMenu() as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => "End match" });
    // Save first, so it's the item already highlighted when this opens —
    // a hurried double-press keeps the activity rather than losing it.
    menu.addItem(new WatchUi.MenuItem("Save activity", null, :save, {}));
    menu.addItem(new WatchUi.MenuItem("Discard activity", null, :discard, {}));
    return menu;
}

class EndMatchMenuDelegate extends WatchUi.Menu2InputDelegate {

    function initialize() {
        Menu2InputDelegate.initialize();
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var id = item.getId();
        if (id == :save) {
            getApp().activityRecorder.stopAndSave();
            System.exit();
        } else if (id == :discard) {
            // Unrecoverable, so asked twice. Pushed on top of this menu,
            // so answering No lands back on the Save/Discard choice.
            WatchUi.pushView(
                new WatchUi.Confirmation("Discard activity?"),
                new DiscardActivityDelegate(),
                WatchUi.SLIDE_IMMEDIATE
            );
        }
    }
}

class DiscardActivityDelegate extends WatchUi.ConfirmationDelegate {

    function initialize() {
        ConfirmationDelegate.initialize();
    }

    function onResponse(response as WatchUi.Confirm) as Boolean {
        if (response == WatchUi.CONFIRM_YES) {
            getApp().activityRecorder.stopAndDiscard();
            System.exit();
        }
        return true;
    }
}
