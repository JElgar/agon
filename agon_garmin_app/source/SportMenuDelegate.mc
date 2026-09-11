import Toybox.Lang;
import Toybox.WatchUi;

//! Handles the sport-select screen shown on launch (`Rez.Menus.SportMenu`
//! — see agonApp.getInitialView). Only football is wired up; other sports
//! would each get their own live-scoring view/delegate pair the same way
//! football has agonView/agonDelegate.
//!
//! `switchToView` (not `pushView`) — there's no "back to sport picker"
//! once you're scoring a match, so this replaces the view stack rather
//! than layering on top of it.
class SportMenuDelegate extends WatchUi.MenuInputDelegate {

    function initialize() {
        MenuInputDelegate.initialize();
    }

    function onMenuItem(item as Symbol) as Void {
        if (item == :football) {
            WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_LEFT);
        }
    }
}
