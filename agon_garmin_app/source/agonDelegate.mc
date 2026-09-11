import Toybox.Lang;
import Toybox.WatchUi;

class agonDelegate extends WatchUi.BehaviorDelegate {

    function initialize() {
        BehaviorDelegate.initialize();
    }

    function onMenu() as Boolean {
        WatchUi.pushView(new Rez.Menus.MainMenu(), new agonMenuDelegate(), WatchUi.SLIDE_UP);
        return true;
    }

}