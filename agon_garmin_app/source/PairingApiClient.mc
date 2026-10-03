import Toybox.Lang;
import Toybox.Communications;

//! Talks to the two device-pairing endpoints (see
//! `agon_service`'s `render_pairing_qr`/`pair_device` handlers and
//! docs/garmin-live-scoring.md) — kept separate from `MatchApiClient`/
//! `LiveApiClient` (both authenticated, used only once pairing has
//! actually produced a token) since this is the one client that runs
//! before there's any token to send at all.
//!
//! Uses the shared `API_BASE_URL` (see ApiConfig.mc) — same server as
//! `MatchApiClient`/`LiveApiClient`, just unauthenticated (there's no
//! token to send until pairing actually succeeds).
class PairingApiClient {

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
