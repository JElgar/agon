import Toybox.Activity;
import Toybox.ActivityRecording;
import Toybox.Lang;

//! Wraps a normal `ActivityRecording.Session` so the wearer's own activity
//! (GPS/HR) records exactly like any other Garmin workout, entirely
//! independent of the scoring menu elsewhere in this app — see
//! docs/garmin-live-scoring.md's "Recording + scoring, concurrently"
//! section.
//!
//! Sport is `Activity.SPORT_SOCCER` — `ActivityRecording.SPORT_GENERIC`
//! (what this used before a real build caught it) is deprecated in favor
//! of the `Activity.Sport` constants, and there's a real soccer/football
//! one available, so no need for "generic" at all.
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
            :sport => Activity.SPORT_SOCCER,
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
