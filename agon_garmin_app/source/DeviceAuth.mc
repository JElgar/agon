import Toybox.Lang;
import Toybox.Application;

//! A plain `const` declared inside a class isn't reachable from that same
//! class's own `static function`s — not bare (no `self` in a static
//! function) and not even as `DeviceAuth.STORAGE_KEY_ACCESS_TOKEN` (real
//! compiler errors on both: "Cannot find symbol ':STORAGE_KEY_ACCESS_TOKEN'
//! on type 'self'", then "...on type '$.DeviceAuth'"). File-scope consts
//! (outside any class) don't have this problem — same pattern
//! MatchPickerView.mc's `MAX_MATCH_ITEMS` already uses successfully.
const DEVICE_AUTH_STORAGE_KEY_ACCESS_TOKEN = "device_access_token";

//! Persists the device's own access token — the credential
//! `POST /devices/pair` hands back once someone confirms this watch's
//! pairing code (see PairingView, agon_service's `PairDeviceOutput`).
//! Once this exists, `agonApp.getInitialView()` skips straight past
//! PairingView to the sport picker on every future launch. There's no
//! "log out"/re-pair path yet — see docs/garmin-live-scoring.md's
//! "what's left to build".
class DeviceAuth {

    static function getAccessToken() as String or Null {
        return Application.Storage.getValue(DEVICE_AUTH_STORAGE_KEY_ACCESS_TOKEN);
    }

    static function setAccessToken(token as String) as Void {
        Application.Storage.setValue(DEVICE_AUTH_STORAGE_KEY_ACCESS_TOKEN, token);
    }
}
