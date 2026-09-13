import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.System;

//! Shown after picking "Football" on the sport menu, before the score
//! screen — lets the wearer pick which of their matches to actually
//! score. This is a plain text status View (loading / empty / error);
//! once matches actually load it immediately hands off to a real
//! `WatchUi.Menu2` (`buildMatchMenu`/`MatchMenuDelegate`) rather than
//! rendering the list itself.
//!
//! Uses `Menu2`, not the legacy `WatchUi.Menu` every other menu in this
//! app still uses (`agonMenuDelegate`, `GoalFlow`) — `Menu2` is the
//! round-display-aware widget (title and item text both stay clear of
//! the bezel); the legacy `Menu` clipped both a long match name and even
//! the "Select match" title itself against the edge of a real round
//! screen (confirmed from a screenshot). It's also why this file no
//! longer needs `GoalFlow.mc`'s `PLAYER_SLOT_SYMBOLS` slot-symbol
//! indirection trick — unlike the legacy `Menu`, a `Menu2` `MenuItem`
//! takes any `Object` as its id, so a match's own string id can be used
//! directly.
const MAX_MATCH_ITEMS = 10;

//! Extract up to `MAX_MATCH_ITEMS` `{"id" => .., "name" => ..}` entries
//! from a `GET /matches` response's `items` array. A blank `name` (a
//! match created without one) falls back to a placeholder — an empty
//! menu label would otherwise be unselectable-looking.
function buildMatchList(items as Array) as Array {
    var matches = [];
    var i = 0;
    while (i < items.size() && i < MAX_MATCH_ITEMS) {
        var item = items[i] as Dictionary;
        var id = item.get("id");
        if (id != null) {
            var name = item.get("name");
            if (name == null || (name as String).length() == 0) {
                name = "Match";
            }
            matches = matches.add({ "id" => id, "name" => name });
        }
        i += 1;
    }
    return matches;
}

function buildMatchMenu(matches as Array) as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => "Select match" });
    var i = 0;
    while (i < matches.size()) {
        var match = matches[i] as Dictionary;
        menu.addItem(new WatchUi.MenuItem(match.get("name") as String, null, match.get("id") as String, {}));
        i += 1;
    }
    return menu;
}

class MatchPickerView extends WatchUi.View {

    var _apiClient as MatchApiClient;
    //! Two lines, not one — a single `drawText` with a long sentence
    //! ("Couldn't load matches. Select to retry.") ran off the edge of a
    //! real round screen (confirmed from a screenshot); `Dc.drawText`
    //! doesn't wrap on its own (same lesson as `PairingView`'s fallback
    //! text). `_statusLine2` is "" for the loading state, which just
    //! skips drawing that line.
    var _statusLine1 as String;
    var _statusLine2 as String;

    function initialize() {
        View.initialize();
        _apiClient = new MatchApiClient();
        _statusLine1 = "Loading matches...";
        _statusLine2 = "";
    }

    function onLayout(dc as Dc) as Void {
    }

    function onShow() as Void {
        loadMatches();
    }

    function onUpdate(dc as Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();
        var centerX = dc.getWidth() / 2;
        var centerY = dc.getHeight() / 2;
        dc.drawText(
            centerX, centerY - 15, Graphics.FONT_XTINY,
            _statusLine1,
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
        );
        if (_statusLine2.length() > 0) {
            dc.drawText(
                centerX, centerY + 15, Graphics.FONT_XTINY,
                _statusLine2,
                Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER
            );
        }
    }

    //! Exposed so `MatchPickerDelegate.onSelect` can retry from the
    //! empty/error state.
    function retry() as Void {
        loadMatches();
    }

    function setStatus(line1 as String, line2 as String) as Void {
        _statusLine1 = line1;
        _statusLine2 = line2;
        WatchUi.requestUpdate();
    }

    function loadMatches() as Void {
        setStatus("Loading matches...", "");
        _apiClient.fetchMyUserId(method(:onUserId));
    }

    function onUserId(responseCode as Number, data as Dictionary or String or Null) as Void {
        // Temporary — see PairingView's matching comment: this and
        // onMatches/onMatchDetails collapse every failure into the same
        // on-screen message, so the real cause (a network error code vs.
        // a real HTTP status) is only visible in this log.
        System.println("[match-picker] /users/me responseCode=" + responseCode);
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var profile = dict.get("profile");
            if (profile != null) {
                var id = (profile as Dictionary).get("id");
                if (id != null) {
                    _apiClient.fetchMatches(id as String, method(:onMatches));
                    return;
                }
            }
        }
        setStatus("Couldn't load matches.", "Select to retry.");
    }

    function onMatches(responseCode as Number, data as Dictionary or String or Null) as Void {
        System.println("[match-picker] /matches responseCode=" + responseCode);
        if (responseCode == 200 && data != null) {
            var dict = data as Dictionary;
            var items = dict.get("items");
            if (items != null) {
                var matches = buildMatchList(items as Array);
                if (matches.size() > 0) {
                    WatchUi.switchToView(
                        buildMatchMenu(matches),
                        new MatchMenuDelegate(),
                        WatchUi.SLIDE_UP
                    );
                    return;
                }
                setStatus("No matches found.", "Select to retry.");
                return;
            }
        }
        setStatus("Couldn't load matches.", "Select to retry.");
    }
}

//! Input handling for `MatchPickerView`'s loading/empty/error text state
//! — `onSelect` retries. Back falls through to `BehaviorDelegate`'s
//! default (exits the app), same as every other top-level view in this
//! app.
class MatchPickerDelegate extends WatchUi.BehaviorDelegate {

    var _view as MatchPickerView;

    function initialize(view as MatchPickerView) {
        BehaviorDelegate.initialize();
        _view = view;
    }

    function onSelect() as Boolean {
        _view.retry();
        return true;
    }
}

//! Input handling for the real match list (`buildMatchMenu`) — reads the
//! selected match id directly off the `MenuItem` (see `buildMatchMenu`;
//! `Menu2` needs no slot-symbol resolution the way the legacy `Menu`
//! did), fetches that match's full roster, and hands off to the score
//! screen once it's loaded.
class MatchMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _apiClient as MatchApiClient;

    function initialize() {
        Menu2InputDelegate.initialize();
        _apiClient = new MatchApiClient();
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var matchId = item.getId();
        if (matchId == null) {
            return;
        }
        _apiClient.fetchMatch(matchId as String, method(:onMatchDetails));
    }

    function onMatchDetails(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var match = data as Dictionary;
            getApp().matchContext.populateFrom(match);
            getApp().liveApiClient.setMatch(getApp().matchContext.matchId);
            WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_LEFT);
            return;
        }
        // Failure: nothing better to do in this basics-only prototype than
        // go back to the picker and let the wearer retry or pick again.
        var pickerView = new MatchPickerView();
        WatchUi.switchToView(pickerView, new MatchPickerDelegate(pickerView), WatchUi.SLIDE_RIGHT);
    }
}
