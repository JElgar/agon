import Toybox.ActivityRecording;
import Toybox.Lang;

//! Wraps a normal `ActivityRecording.Session` so the wearer's own activity
//! (GPS/HR) records exactly like any other Garmin workout, entirely
//! independent of the scoring menu elsewhere in this app — see
//! docs/garmin-live-scoring.md's "Recording + scoring, concurrently"
//! section.
//!
//! Uses `SPORT_GENERIC` with a descriptive session name rather than a
//! soccer-specific sport constant — swap in a more specific one if your
//! SDK version exposes it (check `Toybox.ActivityRecording`'s API docs).
//! Generic still gets a normal recorded activity in Garmin Connect either
//! way; only the auto-categorization would improve.
class ActivityRecorder {

    var _session as ActivityRecording.Session?;

    function initialize() {
        _session = null;
    }

    function isRecording() as Boolean {
        return _session != null && _session.isRecording();
    }

    //! Start (or resume, if already started) recording. Safe to call more
    //! than once — a second call while already recording is a no-op.
    function start() as Void {
        if (_session != null) {
            if (!_session.isRecording()) {
                _session.start();
            }
            return;
        }
        _session = ActivityRecording.createSession({
            :name => "Football",
            :sport => ActivityRecording.SPORT_GENERIC,
        });
        _session.start();
    }

    //! Stop and save the recorded activity (e.g. full-time, or the app
    //! closing). Does nothing if there's no session — calling this before
    //! `start` is a no-op, not an error.
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
}
