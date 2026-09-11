import Toybox.Application;
import Toybox.Lang;
import Toybox.WatchUi;

class agonApp extends Application.AppBase {

    var activityRecorder as ActivityRecorder;
    var score as FootballScore;

    function initialize() {
        AppBase.initialize();
        activityRecorder = new ActivityRecorder();
        score = new FootballScore(new MockApiClient());
    }

    // onStart() is called on application start up
    function onStart(state as Dictionary?) as Void {
        // Recording is *not* started here — it starts when the wearer
        // records the first "start of half" event (kick-off / second-half
        // kick-off), not the moment the app happens to open. See
        // agonMenuDelegate's period handling.
    }

    // onStop() is called when your application is exiting
    function onStop(state as Dictionary?) as Void {
        // Safety net if the app is closed mid-match without an explicit
        // "End match" — never leaves a recording running unsaved.
        activityRecorder.stopAndSave();
    }

    // Return the initial view of your application here: a sport picker
    // (only football is wired up today — see SportMenuDelegate), not the
    // scoring screen directly. Titled explicitly so a single-item menu
    // still reads as "pick one of these", not a bare, unlabeled screen.
    function getInitialView() as [Views] or [Views, InputDelegates] {
        var sportMenu = new Rez.Menus.SportMenu();
        sportMenu.setTitle("Select sport");
        return [ sportMenu, new SportMenuDelegate() ];
    }

}

function getApp() as agonApp {
    return Application.getApp() as agonApp;
}
