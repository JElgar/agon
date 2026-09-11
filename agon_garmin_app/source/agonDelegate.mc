import Toybox.Lang;
import Toybox.WatchUi;

class agonDelegate extends WatchUi.BehaviorDelegate {

    function initialize() {
        BehaviorDelegate.initialize();
    }

    function onMenu() as Boolean {
        openMenu();
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
        openMenu();
        return true;
    }

    function openMenu() as Void {
        // Built dynamically from the match's current period — see
        // agonMenuDelegate.buildMainMenu — not the (now unused)
        // resources/menus/menu.xml resource.
        WatchUi.pushView(buildMainMenu(), new agonMenuDelegate(), WatchUi.SLIDE_UP);
    }

}
