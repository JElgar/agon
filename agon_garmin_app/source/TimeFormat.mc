import Toybox.Lang;

//! `mm:ss` (or `h:mm:ss` past an hour) for a duration given in whole
//! seconds — shared by anything drawing a running clock (the score
//! screen's current-half clock, the activity stats screen's current-half
//! and total-time clocks). `null` (nothing to show yet — no GPS fix, no
//! period marker from the server yet, ...) renders as "--:--" rather than
//! a misleading 0:00.
function formatDuration(totalSeconds as Number?) as String {
    if (totalSeconds == null) {
        return "--:--";
    }
    // A small clock-skew/rounding negative (the moment just parsed being a
    // second "after" Time.now() on the same tick) reads better as 0:00
    // than as a garbled negative duration.
    var seconds = totalSeconds as Number;
    if (seconds < 0) {
        seconds = 0;
    }
    var hours = seconds / 3600;
    var minutes = (seconds / 60) % 60;
    var secs = seconds % 60;
    if (hours > 0) {
        return hours.toString() + ":" + minutes.format("%02d") + ":" + secs.format("%02d");
    }
    return minutes.toString() + ":" + secs.format("%02d");
}
