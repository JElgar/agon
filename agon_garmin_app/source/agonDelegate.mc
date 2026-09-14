import Toybox.Lang;
import Toybox.WatchUi;

class agonDelegate extends WatchUi.BehaviorDelegate {

    function initialize() {
        BehaviorDelegate.initialize();
    }

    function onMenu() as Boolean {
        openMainMenu();
        return true;
    }

    // Also open the menu on plain select — onMenu() alone depends on a
    // gesture (often a long-press) that isn't obvious/available on every
    // device or in the simulator, and this screen doesn't otherwise use
    // select for anything else. Several devices in this app's product
    // list (venu/vivoactive/venusq — touchscreen, no physical buttons)
    // have no button-based "menu" gesture at all, so this is the one way
    // in that's guaranteed to exist everywhere.
    function onSelect() as Boolean {
        openMainMenu();
        return true;
    }

    //! Down/up (or a swipe, on touchscreens) pages to the activity stats
    //! screen, the same gesture that cycles a native Garmin activity's
    //! data screens. With only two pages, next and previous both go to
    //! the other one. `switchToView` keeps both pages at the same depth
    //! in the view stack, so Back behaves identically on either.
    function onNextPage() as Boolean {
        WatchUi.switchToView(new ActivityStatsView(), new ActivityStatsDelegate(), WatchUi.SLIDE_UP);
        return true;
    }

    function onPreviousPage() as Boolean {
        WatchUi.switchToView(new ActivityStatsView(), new ActivityStatsDelegate(), WatchUi.SLIDE_DOWN);
        return true;
    }

}
