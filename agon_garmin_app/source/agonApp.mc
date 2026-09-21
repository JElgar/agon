import Toybox.Application;
import Toybox.Lang;
import Toybox.WatchUi;

class agonApp extends Application.AppBase {

    var activityRecorder as ActivityRecorder;
    var score as FootballScore;
    //! The real side/roster data for whatever match MatchPickerView picks
    //! — empty (default side names, no players) until then. Read by
    //! GoalFlow/LiveApiClient once scoring actually starts.
    var matchContext as MatchContext;
    //! Posts FootballScore's events to the real API — created up front
    //! (like `score`/`matchContext`) so `score`'s constructor always has
    //! something to call, but inert (`recordGoal`/`recordPeriod` no-op)
    //! until MatchPickerView calls `setMatch` once a match is actually
    //! chosen.
    var liveApiClient as LiveApiClient;

    function initialize() {
        AppBase.initialize();
        activityRecorder = new ActivityRecorder();
        matchContext = new MatchContext();
        liveApiClient = new LiveApiClient();
        score = new FootballScore(liveApiClient);
    }

    // onStart() is called on application start up
    function onStart(state as Dictionary?) as Void {
        // Recording is *not* started here — kick-off starts it on every
        // watch with the match open, and otherwise the wearer starts it
        // from the main menu. See ActivityRecorder.startForKickOff.
    }

    // onStop() is called when your application is exiting
    function onStop(state as Dictionary?) as Void {
        // Safety net if the app is closed mid-match without going through
        // End match — never leaves a recording (running or paused)
        // unsaved. A no-op after End match, which has already saved or
        // discarded it.
        activityRecorder.stopAndSave();
        activityRecorder.disableGps();
    }

    // Return the initial view of your application here. No stored device
    // access token yet (first launch, or pairing never finished) means
    // this watch isn't attached to an account — show PairingView instead
    // of the sport picker, same as any other "log in first" gate. Once
    // paired, every future launch skips straight to the sport picker
    // (only football is wired up today — see SportMenuDelegate). Titled
    // explicitly so a single-item menu still reads as "pick one of
    // these", not a bare, unlabeled screen.
    function getInitialView() as [Views] or [Views, InputDelegates] {
        if (DeviceAuth.getAccessToken() == null) {
            var pairingView = new PairingView();
            return [ pairingView, new PairingDelegate(pairingView) ];
        }
        var sportMenu = new Rez.Menus.SportMenu();
        sportMenu.setTitle("Select sport");
        return [ sportMenu, new SportMenuDelegate() ];
    }

    //! Called by PairingView once `POST /devices/pair` returns a token —
    //! persists it and moves on to today's sport picker, the same place a
    //! fresh launch goes once a token is already stored.
    function onPaired(accessToken as String) as Void {
        DeviceAuth.setAccessToken(accessToken);
        var sportMenu = new Rez.Menus.SportMenu();
        sportMenu.setTitle("Select sport");
        WatchUi.switchToView(sportMenu, new SportMenuDelegate(), WatchUi.SLIDE_LEFT);
    }

}

function getApp() as agonApp {
    return Application.getApp() as agonApp;
}
