using Toybox.Application;
using Toybox.Application.Storage;
using Toybox.Application.Properties;
using Toybox.Lang;
using Toybox.WatchUi;

//! Application entry point. Owns the long-lived collaborators (the offline
//! event queue, the API client, the activity recorder) as singletons
//! reachable via `AgonLiveScoringApp.get()`, and reacts to the user
//! pairing the device through Garmin Connect Mobile's per-app settings
//! screen (see resources/settings/settings.xml).
class AgonLiveScoringApp extends Application.AppBase {

    const STORAGE_KEY_ACCESS_TOKEN = "accessToken";
    // Idempotency guard for onSettingsChanged — see that method's comment.
    const STORAGE_KEY_LAST_CLAIMED_PAIRING_CODE = "lastClaimedPairingCode";

    var eventQueue as EventQueue;
    var apiClient as AgonApiClient;
    var activityRecorder as ActivityRecorder;

    function initialize() {
        AppBase.initialize();
        eventQueue = new EventQueue();
        apiClient = new AgonApiClient(Properties.getValue("apiBaseUrl"));
        activityRecorder = new ActivityRecorder();
    }

    function onStart(state as Dictionary?) as Void {
        // Recording starts as soon as the app opens, independent of
        // pairing/scoring-config state entirely — see
        // docs/garmin-live-scoring.md's "Recording + scoring,
        // concurrently" section on why these two don't gate each other.
        activityRecorder.start();
    }

    function onStop(state as Dictionary?) as Void {
        activityRecorder.stopAndSave();
    }

    function getInitialView() as Array {
        var view = new ScoringView();
        return [view, new ScoringDelegate(view)];
    }

    //! Fires whenever the user changes a setting via Garmin Connect
    //! Mobile. The only field that triggers real behavior is
    //! `pairingCode`: entering a fresh code kicks off
    //! `AgonApiClient.pairDevice` automatically, so pairing needs no
    //! separate "confirm" step on the watch itself — `ScoringView` just
    //! watches `accessToken()` to notice it landed.
    function onSettingsChanged() as Void {
        // The base URL can change independently of pairing (e.g. pointed
        // at a different environment) — rebuild the client either way.
        apiClient = new AgonApiClient(Properties.getValue("apiBaseUrl"));

        var code = Properties.getValue("pairingCode");
        if (code == null || code.equals("")) {
            return;
        }
        // Guard against re-claiming an already-claimed code: this handler
        // can fire again for reasons unrelated to pairing (matchId edited
        // afterwards, say) while Garmin Connect Mobile still reports the
        // same code it last synced — a pairing code is single-use, so a
        // second claim attempt would just fail loudly for no reason.
        var lastClaimed = Storage.getValue(STORAGE_KEY_LAST_CLAIMED_PAIRING_CODE);
        if (lastClaimed != null && code.equals(lastClaimed)) {
            return;
        }
        if (accessToken() != null) {
            return;
        }

        apiClient.pairDevice(code, method(:onPairDeviceComplete));
    }

    function onPairDeviceComplete(success as Boolean, responseCode as Number, data as Dictionary?) as Void {
        if (success && data != null && data.hasKey("access_token")) {
            Storage.setValue(STORAGE_KEY_ACCESS_TOKEN, data.get("access_token"));
            Storage.setValue(STORAGE_KEY_LAST_CLAIMED_PAIRING_CODE, Properties.getValue("pairingCode"));
        }
        // Whether it succeeded or not, tell the visible view to redraw —
        // ScoringView reads accessToken()/isConfigured() fresh every
        // onUpdate rather than being told the outcome directly.
        WatchUi.requestUpdate();
    }

    function accessToken() as String? {
        return Storage.getValue(STORAGE_KEY_ACCESS_TOKEN);
    }

    function matchId() as String? {
        return nonEmpty(Properties.getValue("matchId"));
    }

    function homeSideId() as String? {
        return nonEmpty(Properties.getValue("homeSideId"));
    }

    function awaySideId() as String? {
        return nonEmpty(Properties.getValue("awaySideId"));
    }

    //! True once pairing has succeeded *and* a match + both side ids are
    //! set — everything `ScoringView`'s scoring UI needs. See the
    //! README's "Configuration (App Settings)" section for where these
    //! come from (there's no on-watch match/roster browser yet).
    function isConfigured() as Boolean {
        return accessToken() != null
            && matchId() != null
            && homeSideId() != null
            && awaySideId() != null;
    }

    function nonEmpty(v as String?) as String? {
        if (v == null || v.equals("")) {
            return null;
        }
        return v;
    }

    //! Typed accessor for the running app instance — `Application.getApp()`
    //! returns the base `AppBase` type, so callers elsewhere use this
    //! instead of casting at every call site.
    static function get() as AgonLiveScoringApp {
        return Application.getApp() as AgonLiveScoringApp;
    }
}
