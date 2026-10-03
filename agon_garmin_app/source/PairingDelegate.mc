import Toybox.Lang;
import Toybox.WatchUi;

//! Input handling for PairingView.
//!
//! `onSelect` toggles a plain-text help screen (the pairing site + the
//! code, big and legible) over the QR — the universally-reachable action
//! (see agonDelegate's own onSelect for why: several devices in this app's
//! product list are touchscreen-only with no menu gesture at all), useful
//! whenever the QR itself is hard to scan or someone would rather just
//! read/type the code.
//!
//! `onMenu` forces a fresh code/QR without waiting for the client-side
//! give-up timeout (e.g. if the QR image failed to load) — a secondary
//! action, so it's fine that it depends on a gesture (often long-press)
//! that isn't available on every device; the 8-minute give-up timeout
//! covers those regardless.
//!
//! Back falls through to BehaviorDelegate's default (exits the app), same
//! as every other top-level view in this app.
class PairingDelegate extends WatchUi.BehaviorDelegate {

    var _view as PairingView;

    function initialize(view as PairingView) {
        BehaviorDelegate.initialize();
        _view = view;
    }

    function onSelect() as Boolean {
        _view.toggleHelp();
        return true;
    }

    function onMenu() as Boolean {
        _view.regenerateCode();
        return true;
    }

}
