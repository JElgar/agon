import Toybox.Activity;
import Toybox.ActivityRecording;
import Toybox.Attention;
import Toybox.Lang;
import Toybox.Position;
import Toybox.System;

//! Wraps a normal `ActivityRecording.Session` so the wearer's own activity
//! (GPS/HR) records exactly like any other Garmin workout, entirely
//! independent of the scoring menu elsewhere in this app — see
//! docs/garmin-live-scoring.md's "Recording + scoring, concurrently"
//! section.
//!
//! Recording is its own control, not a side effect of scoring: the main
//! menu's Start/Pause/Resume activity item drives `start`/`pause`, and the
//! End match flow (EndMatchFlow.mc) finishes it with `stopAndSave` or
//! `stopAndDiscard`. The one automatic start is kick-off
//! (`startForKickOff`), on every watch that has the match open when it
//! happens; a watch that opens a match already under way shows the red
//! ring (RecordingRing.mc) until its wearer starts recording by hand.
//!
//! The GPS is a separate switch again (`enableGps`). A `Session` records
//! whichever sensors are already on and never turns the GPS on itself,
//! and the manifest's `Positioning` permission doesn't either — Garmin's
//! own RecordSample enables location events alongside its session for
//! exactly this reason. Without that call a match still "records", but
//! with no proper GPS track: the saved distance comes out far short of
//! what the wearer actually covered.
//!
//! Sport is `Activity.SPORT_SOCCER` — `ActivityRecording.SPORT_GENERIC`
//! (what this used before a real build caught it) is deprecated in favor
//! of the `Activity.Sport` constants, and there's a real soccer/football
//! one available, so no need for "generic" at all.
class ActivityRecorder {

    var _session as ActivityRecording.Session?;
    //! `Activity.Info.timerTime` (ms) at the moment the current half
    //! started — see `markHalfStart`/`currentHalfTimerTimeMs`. `0` before
    //! any half has started, matching `timerTime`'s own baseline.
    var _halfStartTimerTimeMs as Number;
    //! Whether `enableGps` has turned location events on — so repeat calls
    //! (every match opened, every `start`) don't re-request them.
    var _gpsEnabled as Boolean;

    function initialize() {
        _session = null;
        _halfStartTimerTimeMs = 0;
        _gpsEnabled = false;
    }

    //! Turn the GPS on, continuously, until `disableGps`. Called the moment
    //! a match is opened (`MatchMenuDelegate.onMatchDetails`), not only
    //! when recording starts, so the watch usually has a fix by kick-off
    //! rather than spending the first minute of the match acquiring one;
    //! `start` calls it too, as a backstop. Only the first call does
    //! anything.
    //!
    //! Asks for the most accurate satellite configuration the watch
    //! supports — see `bestGpsConfiguration`.
    function enableGps() as Void {
        if (_gpsEnabled) {
            return;
        }
        var configuration = bestGpsConfiguration();
        if (configuration != null) {
            Position.enableLocationEvents({
                :acquisitionType => Position.LOCATION_CONTINUOUS,
                :configuration => configuration,
            }, method(:onPosition));
        } else {
            Position.enableLocationEvents(Position.LOCATION_CONTINUOUS, method(:onPosition));
        }
        _gpsEnabled = true;
    }

    //! Multi-band first, then all-systems L1, then plain GPS — the fallback
    //! order of Garmin's own `Position.enableLocationEvents` example.
    //! Football is mostly short bursts and sharp turns, exactly the
    //! movement a less accurate fix smooths away (and under-counts
    //! distance on), so it's worth the extra battery for one match.
    //! `null` means none are supported, and `enableGps` falls back to
    //! plain continuous location events. `Position has` guards the query
    //! itself rather than trusting `minApiLevel` alone: a watch missing it
    //! would otherwise crash the moment a match is opened.
    function bestGpsConfiguration() as Position.Configuration? {
        if (!(Position has :hasConfigurationSupport)) {
            return null;
        }
        if (Position.hasConfigurationSupport(Position.CONFIGURATION_GPS_GLONASS_GALILEO_BEIDOU_L1_L5)) {
            return Position.CONFIGURATION_GPS_GLONASS_GALILEO_BEIDOU_L1_L5;
        }
        if (Position.hasConfigurationSupport(Position.CONFIGURATION_GPS_GLONASS_GALILEO_BEIDOU_L1)) {
            return Position.CONFIGURATION_GPS_GLONASS_GALILEO_BEIDOU_L1;
        }
        if (Position.hasConfigurationSupport(Position.CONFIGURATION_GPS)) {
            return Position.CONFIGURATION_GPS;
        }
        return null;
    }

    //! Nothing to do with each fix here: the `Session` records it on its
    //! own, and `ActivityStatsView` reads distance back from
    //! `Activity.getActivityInfo()`. Passed as the listener anyway, empty,
    //! the same as Garmin's RecordSample does.
    function onPosition(info as Position.Info) as Void {
    }

    //! Turn the GPS back off (`agonApp.onStop`, after the activity has been
    //! saved). A no-op if it was never turned on.
    function disableGps() as Void {
        if (!_gpsEnabled) {
            return;
        }
        Position.enableLocationEvents(Position.LOCATION_DISABLE, null);
        _gpsEnabled = false;
    }

    //! `true` only while the timer is actually running — `false` before
    //! the first `start` and while paused. What the red not-recording ring
    //! (RecordingRing.mc) keys off.
    function isRecording() as Boolean {
        return _session != null && _session.isRecording();
    }

    //! `true` once an activity has been started this match, whether it's
    //! recording or paused right now — `false` again once `stopAndSave`/
    //! `stopAndDiscard` has finished it. The broader check `isRecording`
    //! isn't: "is there anything to pause, resume, save or discard".
    function hasSession() as Boolean {
        return _session != null;
    }

    //! Start the activity, or resume it if paused. Safe to call more than
    //! once — a second call while already recording is a no-op, and
    //! `name` is only used the first time: it names the underlying
    //! `Session` at creation (`ActivityRecording.createSession` has no
    //! rename-after-the-fact call), so a resume passing a different string
    //! wouldn't rename anything anyway. Callers pass
    //! `MatchContext.matchName()` so the recorded activity's title in
    //! Garmin Connect is the actual fixture ("Home vs Away") rather than a
    //! bare sport name — see that method's doc comment on why the score
    //! can't be included here too.
    function start(name as String) as Void {
        // Normally already on since the match was opened — see enableGps.
        enableGps();
        if (_session != null) {
            if (!_session.isRecording()) {
                _session.start();
            }
            return;
        }
        _session = ActivityRecording.createSession({
            :name => name,
            :sport => Activity.SPORT_SOCCER,
        });
        _session.start();
    }

    //! Kick-off starts the activity on every watch that has the match open
    //! at that moment — whether this wearer pressed Kick-off
    //! (`agonMenuDelegate`) or another device did and this one saw it on
    //! its next score poll (`LiveApiClient.onScore`). Resumes a paused
    //! activity and leaves a running one alone, like `start`, and resets
    //! the half clock either way, since this *is* the first half starting.
    //! Buzzes when it actually starts (or resumes) recording, so a wearer
    //! whose watch started on its own knows it has.
    //!
    //! Second-half kick-off deliberately doesn't do this: someone who
    //! paused at half-time may have meant to stay paused (subbed off, say).
    function startForKickOff(name as String) as Void {
        var wasRecording = isRecording();
        start(name);
        markHalfStart();
        if (!wasRecording && (Attention has :vibrate) && System.getDeviceSettings().vibrateOn) {
            Attention.vibrate([new Attention.VibeProfile(75, 500)]);
        }
    }

    //! Pause a running activity — `Session.stop` halts the timer but keeps
    //! the session (and everything recorded so far) open, so `start`
    //! resumes the same activity rather than beginning a new one. A no-op
    //! if nothing is recording.
    function pause() as Void {
        if (_session != null && _session.isRecording()) {
            _session.stop();
        }
    }

    //! Call once at the start of each half (kick-off, second-half
    //! kick-off) — separate from `start`, which is a no-op the second
    //! time (the underlying session is already running by then) and so
    //! can't itself mark where a new half's clock should start counting
    //! from. Used by `ActivityStatsView`'s "current half" time, which
    //! would otherwise just be the whole match's `timerTime`.
    //!
    //! This is a *separate* concept from `markLap` below, not a
    //! replacement for it: `Activity.Info` has no live "current lap
    //! time" field at all (checked the full field list — nothing like
    //! it exists), so a real `Session` lap boundary alone can't drive
    //! this on-screen clock; it only ever shows up later, as a split, in
    //! the saved activity. This keeps its own baseline instead.
    function markHalfStart() as Void {
        var info = Activity.getActivityInfo();
        if (info.timerTime != null) {
            _halfStartTimerTimeMs = info.timerTime as Number;
        }
    }

    //! Mark a real lap boundary in the recording (`Session.addLap`) —
    //! called at half-time, second-half kick-off, and full-time (not
    //! kick-off itself: the session's own start is already the first
    //! lap's start, nothing to close yet) so the *saved* activity splits
    //! into "first half"/"half-time break"/"second half" laps, each with
    //! its own real pace/HR/distance splits, once viewed in Garmin
    //! Connect. Purely a FIT-file concern — see `markHalfStart`'s doc
    //! comment on why this can't also drive the live on-screen clock. A
    //! no-op if nothing is recording (not started yet, or paused).
    function markLap() as Void {
        if (_session != null && _session.isRecording()) {
            _session.addLap();
        }
    }

    //! Elapsed timer time (ms) since `markHalfStart` was last called —
    //! `null` if `Activity.Info.timerTime` itself is (no active session
    //! yet). Doesn't reset at half-time: it keeps counting through the
    //! break until the next kick-off calls `markHalfStart` again, unless
    //! the wearer pauses the activity (`timerTime` itself stops while
    //! paused, so this does too).
    function currentHalfTimerTimeMs() as Number? {
        var info = Activity.getActivityInfo();
        if (info.timerTime == null) {
            return null;
        }
        return (info.timerTime as Number) - _halfStartTimerTimeMs;
    }

    //! Stop and save the recorded activity (End match's "Save activity",
    //! or the app closing). Does nothing if there's no session — calling
    //! this before `start` is a no-op, not an error.
    function stopAndSave() as Void {
        if (_session == null) {
            return;
        }
        if (_session.isRecording()) {
            _session.stop();
        }
        _session.save();
        _session = null;
    }

    //! Stop and throw the recorded activity away (End match's "Discard
    //! activity", once confirmed — see EndMatchFlow.mc). A no-op without
    //! a session, same as `stopAndSave`.
    function stopAndDiscard() as Void {
        if (_session == null) {
            return;
        }
        if (_session.isRecording()) {
            _session.stop();
        }
        _session.discard();
        _session = null;
    }
}
