import Toybox.Lang;
import Toybox.Application;
import Toybox.Math;
import Toybox.System;

//! File-scope, not class-scope: a `const` inside a class isn't reachable
//! from that class's own `static function`s, neither bare nor as
//! `PairingCode.ALPHABET` (real compiler errors on both forms — see
//! DeviceAuth.mc's doc comment for the exact messages). Same pattern
//! MatchPickerView.mc's `MAX_MATCH_ITEMS` already uses successfully.
const PAIRING_CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const PAIRING_CODE_LENGTH = 6;
const PAIRING_CODE_STORAGE_KEY = "pairing_code";

//! Generates and persists the short, human-typeable pairing code the watch
//! displays (as a QR code — see PairingApiClient/PairingView — plus this
//! bare string as a scan-fails fallback). Matches the backend's own scheme
//! (see agon_service's `confirm_device_pairing`/`pair_device` handlers,
//! and `agon_core::dao::device_pairing`): 6 characters drawn from a
//! 32-symbol alphabet that drops visually ambiguous characters (0/O,
//! 1/I/L) — case doesn't matter, since the server upper-cases whatever
//! it's given.
//!
//! Persisted in Application.Storage (not just held in memory) so the same
//! code survives the watch app being backgrounded/relaunched while
//! someone is mid-scan — regenerating on every launch would invalidate a
//! code someone's about to type in.
class PairingCode {

    //! The current code, generating and persisting a new one on first call
    //! (or after a previous `regenerate()`/PairingView give-up has cleared
    //! it — see PairingView.CODE_LIFETIME_MS).
    static function getOrCreate() as String {
        var existing = Application.Storage.getValue(PAIRING_CODE_STORAGE_KEY);
        if (existing != null) {
            return existing as String;
        }
        return regenerate();
    }

    //! Discard whatever code is stored and generate + persist a fresh one
    //! — called when a code goes stale (the client-side give-up timeout)
    //! or the server rejects it as already used/expired.
    static function regenerate() as String {
        // Seeded once per call is fine here — pairing codes are generated
        // at most every few minutes, not in a tight loop where reseeding
        // from the same millisecond would repeat a sequence.
        Math.srand(System.getTimer());
        var code = "";
        for (var i = 0; i < PAIRING_CODE_LENGTH; i += 1) {
            var index = Math.rand() % PAIRING_CODE_ALPHABET.length();
            code += PAIRING_CODE_ALPHABET.substring(index, index + 1);
        }
        Application.Storage.setValue(PAIRING_CODE_STORAGE_KEY, code);
        return code;
    }
}
