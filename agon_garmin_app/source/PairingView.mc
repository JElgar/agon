import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.Timer;
import Toybox.System;

//! Shown instead of the sport picker whenever the watch has no stored
//! device access token yet (see `agonApp.getInitialView`/`onPaired`) —
//! displays a pairing code (as a QR code, falling back to plain text if
//! the QR image fails to load) and polls `POST /devices/pair` until
//! someone confirms it from agon_ui's `/pair` page. See
//! docs/garmin-live-scoring.md for the full flow and
//! `agon_core::dao::device_pairing` for the server side.
class PairingView extends WatchUi.View {

    //! How often to poll `POST /devices/pair` while waiting for someone
    //! to confirm the code — frequent enough to feel instant once
    //! confirmed, without hammering the API.
    const POLL_INTERVAL_MS = 3000;

    //! How long to keep displaying (and polling for) one code before
    //! giving up and generating a fresh one. Comfortably under the
    //! server's own confirmed-code TTL (10 minutes, and that only starts
    //! counting at confirm time — later than this), so a code already on
    //! screen never goes stale out from under someone mid-scan.
    const CODE_LIFETIME_MS = 8 * 60 * 1000;

    var _apiClient as PairingApiClient;
    var _code as String;
    var _qrImage as WatchUi.BitmapResource or Graphics.BitmapReference or Null;
    var _qrWidth as Number;
    var _qrHeight as Number;
    var _statusText as String;
    var _pollTimer as Timer.Timer or Null;
    var _codeStartedAt as Number;

    function initialize() {
        View.initialize();
        _apiClient = new PairingApiClient();
        _code = "";
        _qrImage = null;
        _qrWidth = 0;
        _qrHeight = 0;
        _statusText = "Waiting for phone...";
        _pollTimer = null;
        _codeStartedAt = 0;
    }

    function onLayout(dc as Dc) as Void {
    }

    function onShow() as Void {
        startNewCode();
        _pollTimer = new Timer.Timer();
        _pollTimer.start(method(:onPollTick), POLL_INTERVAL_MS, true);
    }

    function onHide() as Void {
        if (_pollTimer != null) {
            _pollTimer.stop();
            _pollTimer = null;
        }
    }

    function onUpdate(dc as Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();

        var centerX = dc.getWidth() / 2;

        if (_qrImage != null) {
            dc.drawBitmap(centerX - _qrWidth / 2, 8, _qrImage);
            dc.drawText(
                centerX, 8 + _qrHeight + 6, Graphics.FONT_SMALL,
                _code,
                Graphics.TEXT_JUSTIFY_CENTER
            );
        } else {
            // QR still loading, or failed to load — the bare code is
            // always enough to pair on its own (agon_ui's /pair page
            // takes typed-in codes too), so this is a fully functional
            // fallback, not just an error state.
            // Two separate drawText calls, not one string with an
            // embedded newline — Dc.drawText doesn't wrap/interpret line
            // breaks, it just draws whatever string it's given on one
            // line.
            dc.drawText(
                centerX, dc.getHeight() / 2 - 60, Graphics.FONT_SMALL,
                "Pair at",
                Graphics.TEXT_JUSTIFY_CENTER
            );
            dc.drawText(
                centerX, dc.getHeight() / 2 - 40, Graphics.FONT_SMALL,
                "get-agon.com/pair",
                Graphics.TEXT_JUSTIFY_CENTER
            );
            dc.drawText(
                centerX, dc.getHeight() / 2 + 10, Graphics.FONT_NUMBER_MEDIUM,
                _code,
                Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
            );
        }

        dc.drawText(
            centerX, dc.getHeight() - 20, Graphics.FONT_XTINY,
            _statusText,
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
    }

    //! Generate a fresh code, request its QR image, and reset the give-up
    //! clock. Exposed (not just called from onShow) so PairingDelegate can
    //! let the wearer force this manually, e.g. if the QR image failed to
    //! load.
    function regenerateCode() as Void {
        _code = PairingCode.regenerate();
        loadCode();
    }

    //! Same as `regenerateCode`, but reuses a not-yet-expired stored code
    //! rather than always minting a new one — only called from onShow, so
    //! relaunching the app mid-pair doesn't invalidate a code someone's
    //! about to type in.
    function startNewCode() as Void {
        _code = PairingCode.getOrCreate();
        loadCode();
    }

    function loadCode() as Void {
        _qrImage = null;
        _statusText = "Waiting for phone...";
        _codeStartedAt = System.getTimer();
        _apiClient.fetchQrCode(_code, method(:onQrImage));
        WatchUi.requestUpdate();
    }

    function onQrImage(responseCode as Number, image as WatchUi.BitmapResource or Graphics.BitmapReference or Null) as Void {
        if (responseCode == 200 && image != null) {
            _qrImage = image;
            // getWidth/getHeight are a Graphics.BitmapReference thing
            // (API 4.0+) — this app's minApiLevel is 4.2.0, so that's
            // what makeImageRequest actually hands back in practice, even
            // though the callback's declared type also covers the older
            // BitmapResource for source compatibility.
            var bitmap = image as Graphics.BitmapReference;
            _qrWidth = bitmap.getWidth();
            _qrHeight = bitmap.getHeight();
        }
        // Any other outcome just leaves _qrImage null — onUpdate's
        // fallback (plain-text code) covers it, no separate error state
        // needed.
        WatchUi.requestUpdate();
    }

    function onPollTick() as Void {
        if (System.getTimer() - _codeStartedAt > CODE_LIFETIME_MS) {
            regenerateCode();
            return;
        }
        _apiClient.pair(_code, method(:onPairResponse));
    }

    function onPairResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            // The server only ever sends a JSON object for 200 (see
            // PairDeviceOutput) — cast rather than branch on `data`'s
            // declared union type, which only exists because
            // Communications' callback signature covers every possible
            // response shape, not because 200 can actually be a String
            // here.
            var dict = data as Dictionary;
            var accessToken = dict.get("access_token");
            if (accessToken != null) {
                getApp().onPaired(accessToken as String);
            }
            return;
        }
        if (responseCode == 202) {
            // Nobody's confirmed the code yet — expected steady state.
            _statusText = "Waiting for phone...";
        } else if (responseCode == 400) {
            // Confirmed-then-claimed already (by an earlier request), or
            // expired after being confirmed — this exact code is dead. A
            // fresh code/QR sidesteps having to tell the wearer to
            // rescan/retype rather than just handling it.
            _statusText = "Code expired, refreshing...";
            regenerateCode();
        } else if (responseCode == 503) {
            _statusText = "Pairing unavailable";
        } else {
            // Negative responseCode: a BLE/network-layer failure
            // (BLE_CONNECTION_UNAVAILABLE, NETWORK_REQUEST_TIMED_OUT,
            // etc.) — nothing to do but retry on the next tick.
            _statusText = "Waiting for phone...";
        }
        WatchUi.requestUpdate();
    }
}
