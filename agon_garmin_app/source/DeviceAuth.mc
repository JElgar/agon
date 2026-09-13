import Toybox.Lang;
import Toybox.Application;

//! Persists the device's own access token — the credential
//! `POST /devices/pair` hands back once someone confirms this watch's
//! pairing code (see PairingView, agon_service's `PairDeviceOutput`).
//! Once this exists, `agonApp.getInitialView()` skips straight past
//! PairingView to the sport picker on every future launch. There's no
//! "log out"/re-pair path yet — see docs/garmin-live-scoring.md's
//! "what's left to build".
class DeviceAuth {

    const STORAGE_KEY_ACCESS_TOKEN = "device_access_token";

    static function getAccessToken() as String or Null {
        return Application.Storage.getValue(STORAGE_KEY_ACCESS_TOKEN);
    }

    static function setAccessToken(token as String) as Void {
        Application.Storage.setValue(STORAGE_KEY_ACCESS_TOKEN, token);
    }
}
