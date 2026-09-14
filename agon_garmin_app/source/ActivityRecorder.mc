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
    //! `Activity.Info.timerTime` (ms) at the moment the current half
    //! started — see `markHalfStart`/`currentHalfTimerTimeMs`. `0` before
    //! any half has started, matching `timerTime`'s own baseline.
    var _halfStartTimerTimeMs as Number;

    function initialize() {
        _session = null;
        _halfStartTimerTimeMs = 0;
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
    //! no-op if nothing is recording yet.
    function markLap() as Void {
        if (_session != null && _session.isRecording()) {
            _session.addLap();
        }
    }

    //! Elapsed timer time (ms) since `markHalfStart` was last called —
    //! `null` if `Activity.Info.timerTime` itself is (no active session
    //! yet). Doesn't reset again at half-time; there's no explicit pause
    //! in this app, so the underlying timer — and this along with it —
    //! just keeps counting through the break until the next kick-off
    //! calls `markHalfStart` again.
    function currentHalfTimerTimeMs() as Number? {
        var info = Activity.getActivityInfo();
        if (info.timerTime == null) {
            return null;
        }
        return (info.timerTime as Number) - _halfStartTimerTimeMs;
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
