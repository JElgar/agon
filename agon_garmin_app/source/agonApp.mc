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
        // Recording starts as soon as the app opens, independent of the
        // scoring menu entirely — see docs/garmin-live-scoring.md's
        // "Recording + scoring, concurrently" section on why these two
        // don't gate each other.
        activityRecorder.start();
    }

    // onStop() is called when your application is exiting
    function onStop(state as Dictionary?) as Void {
        activityRecorder.stopAndSave();
    }

    // Return the initial view of your application here
    function getInitialView() as [Views] or [Views, InputDelegates] {
        return [ new agonView(), new agonDelegate() ];
    }

}

function getApp() as agonApp {
    return Application.getApp() as agonApp;
}
