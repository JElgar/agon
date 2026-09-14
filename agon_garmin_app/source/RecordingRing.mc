import Toybox.Graphics;
import Toybox.Lang;
import Toybox.System;

//! Pen width (px) of the not-recording ring — thick enough to read at a
//! glance mid-match, which is the whole point of it.
const RECORDING_RING_WIDTH = 12;

//! A thick red ring around the edge of the screen whenever this watch's
//! own activity isn't recording — never started, or paused. Recording and
//! scoring are separate controls (see ActivityRecorder's doc comment), so
//! it's entirely possible to be scoring a match that's already under way
//! — opened after someone else's kick-off, say — without recording it;
//! this makes that impossible to miss. Called by each match page's
//! `onUpdate` (the score screen and the activity stats screen) straight
//! after `dc.clear()`, before any text.
//!
//! A circle on round screens (and on semi-round/semi-octagon ones, whose
//! flat edges just clip it), a rectangle on rectangular ones. Either
//! stroke is centred on its outline, hence the half-pen inset.
function drawNotRecordingRing(dc as Dc) as Void {
    if (getApp().activityRecorder.isRecording()) {
        return;
    }
    var width = dc.getWidth();
    var height = dc.getHeight();
    var inset = RECORDING_RING_WIDTH / 2;

    dc.setAntiAlias(true);
    dc.setPenWidth(RECORDING_RING_WIDTH);
    dc.setColor(Graphics.COLOR_RED, Graphics.COLOR_TRANSPARENT);
    if (System.getDeviceSettings().screenShape == System.SCREEN_SHAPE_RECTANGLE) {
        dc.drawRectangle(inset, inset, width - RECORDING_RING_WIDTH, height - RECORDING_RING_WIDTH);
    } else {
        dc.drawCircle(width / 2, height / 2, width / 2 - inset);
    }

    // Back to the pen and colours every page's own text drawing expects.
    dc.setPenWidth(1);
    dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
}
