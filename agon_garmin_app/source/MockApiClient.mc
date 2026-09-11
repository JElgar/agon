import Toybox.Lang;
import Toybox.System;

//! Stand-in for the real Agon API client (see
//! docs/garmin-live-scoring.md and the backend's
//! POST /matches/:id/live/events) — no network calls yet, just logs what
//! would be sent. Deliberately the same call shape a real client would
//! need (one method per event kind, called as the event happens), so
//! swapping this out later for one that actually calls
//! `Communications.makeWebRequest` is a drop-in replacement for this one
//! class — `FootballScore` and the UI don't need to change at all.
class MockApiClient {

    function initialize() {
    }

    function recordGoal(side as Symbol, ownGoal as Boolean) as Void {
        System.println("[mock api] Goal — side=" + side.toString() + " ownGoal=" + ownGoal.toString());
    }

    function recordPeriod(periodLabel as String) as Void {
        System.println("[mock api] Period — " + periodLabel);
    }
}
