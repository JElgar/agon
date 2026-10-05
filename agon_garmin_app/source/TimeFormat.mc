import Toybox.Lang;
import Toybox.System;

//! Current wall-clock time of day, e.g. "14:32" or "2:32 PM" — respects
//! the device's own 12h/24h clock setting (`System.DeviceSettings.
//! is24Hour`), same as the watch face, rather than hardcoding one format.
function formatTimeOfDay() as String {
    var clockTime = System.getClockTime();
    var hour = clockTime.hour;
    if (System.getDeviceSettings().is24Hour) {
        return hour.format("%02d") + ":" + clockTime.min.format("%02d");
    }
    var displayHour = hour % 12;
    if (displayHour == 0) {
        displayHour = 12;
    }
    var suffix = (hour >= 12) ? "PM" : "AM";
    return displayHour.toString() + ":" + clockTime.min.format("%02d") + " " + suffix;
}

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
