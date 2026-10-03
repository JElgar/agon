import Toybox.Lang;
import Toybox.WatchUi;

//! Handles the sport-select screen shown on launch (`Rez.Menus.SportMenu`
//! — see agonApp.getInitialView). Only football is wired up; other sports
//! would each get their own live-scoring view/delegate pair the same way
//! football has agonView/agonDelegate.
//!
//! `Rez.Menus.SportMenu` is declared as a `<menu2>` resource (see
//! resources/menus/sport_menu.xml) — Menu2, not the legacy Menu every
//! menu in this app originally used, for the same round-screen-clipping
//! reason documented on `MatchPickerView.mc`. A `<menu2>` resource's
//! `menu-item id` still resolves to a plain Symbol at runtime exactly
//! like the legacy `<menu>` did, so `item.getId() == :football` below
//! works the same way `onMenuItem`'s `item == :football` always did.
//!
//! `switchToView` (not `pushView`) — there's no "back to sport picker"
//! once you're picking a match to score, so this replaces the view stack
//! rather than layering on top of it.
class SportMenuDelegate extends WatchUi.Menu2InputDelegate {

    function initialize() {
        Menu2InputDelegate.initialize();
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        if (item.getId() == :football) {
            // MatchPickerView, not straight to agonView — the score
            // screen needs a real match (side/roster data — see
            // MatchContext) before there's anything to score.
            var pickerView = new MatchPickerView();
            WatchUi.switchToView(
                pickerView,
                new MatchPickerDelegate(pickerView),
                WatchUi.SLIDE_LEFT
            );
        }
    }
}
