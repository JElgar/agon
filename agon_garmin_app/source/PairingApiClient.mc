import Toybox.Lang;
import Toybox.Communications;

//! Talks to the two device-pairing endpoints (see
//! `agon_service`'s `render_pairing_qr`/`pair_device` handlers and
//! docs/garmin-live-scoring.md) — kept separate from MockApiClient, which
//! stays entirely mocked for scoring. Pairing is the one thing this
//! prototype actually calls a real API for: there's no other way to get a
//! device credential onto the watch at all.
//!
//! `API_BASE_URL` is a hardcoded constant for now rather than a real App
//! Setting (`resources/settings/*.xml`) — hand-writing that resource XML
//! with no compiler available to check it against isn't worth the risk for
//! a prototype. Revisit once this needs to point at more than one
//! environment from the same build.
class PairingApiClient {

    //! `agon_service` mounts routes at `/` — every deployed environment
    //! (staging included) fronts it with an ingress that publishes that
    //! under `/api` and strips the prefix before it reaches the service
    //! (see `agon-api-ingress` in `agon_infra/index.ts`, and the same
    //! `/api` convention `agon_ui`'s dev proxy and `make test-staging` both
    //! use) — hence the `/api` here despite `agon_service`'s own routes
    //! never mentioning it. For local dev instead, point this at
    //! `http://localhost:7000` (no `/api` — a local `agon_service` has no
    //! ingress in front of it) and run `make run`; the Connect IQ
    //! simulator's network requests are proxied through the desktop it
    //! runs on, so `localhost` reaches it directly.
    const API_BASE_URL = "https://agon.staging.get-agon.com/api";

    //! Fetch the QR code PNG for `code` (see `qr::render_pairing_qr` on
    //! the server). `callback` is invoked as
    //! `(responseCode as Number, image as WatchUi.BitmapResource or
    //! Graphics.BitmapReference or Null)` — `image` is `null` on any
    //! failure (network error or non-200), same as a real "couldn't load"
    //! case; the caller falls back to the bare code text either way.
    function fetchQrCode(code as String, callback as Method) as Void {
        var url = API_BASE_URL + "/devices/pairing-codes/" + code + "/qr.png";
        Communications.makeImageRequest(url, null, {}, callback);
    }

    //! Poll `POST /devices/pair` once. `callback` is invoked as
    //! `(responseCode as Number, data as Dictionary or String or Null)`
    //! exactly as Communications hands it back — see PairingView for how
    //! each status (200/202/400/503, or a negative BLE/network error
    //! code) is interpreted.
    function pair(code as String, callback as Method) as Void {
        var url = API_BASE_URL + "/devices/pair";
        var params = { "code" => code };
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_POST,
            :headers => { "Content-Type" => Communications.REQUEST_CONTENT_TYPE_JSON },
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        Communications.makeWebRequest(url, params, options, callback);
    }
}
