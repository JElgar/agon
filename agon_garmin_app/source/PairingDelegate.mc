import Toybox.Lang;
import Toybox.WatchUi;

//! Input handling for PairingView. Only `onSelect` is meaningful here —
//! lets the wearer force a fresh code/QR without waiting for the
//! client-side give-up timeout (e.g. if the QR image failed to load).
//! Back falls through to BehaviorDelegate's default (exits the app), same
//! as every other top-level view in this app.
class PairingDelegate extends WatchUi.BehaviorDelegate {

    var _view as PairingView;

    function initialize(view as PairingView) {
        BehaviorDelegate.initialize();
        _view = view;
    }

    function onSelect() as Boolean {
        _view.regenerateCode();
        return true;
    }

}
