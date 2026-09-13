import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.System;

//! Shown after picking "Football" on the sport menu, before the score
//! screen — lets the wearer pick which of their matches to actually
//! score. This is a plain text status View (loading / empty / error);
//! once matches actually load it immediately hands off to a real
//! `WatchUi.Menu` (`buildMatchMenu`/`MatchMenuDelegate`) rather than
//! rendering the list itself.
//!
//! Item ids are fixed symbols (`:match_0`.. `:match_9`), the same
//! index-indirection trick `GoalFlow.mc`'s `PLAYER_SLOT_SYMBOLS` uses —
//! Monkey C has no way to mint a `Symbol` from a string at runtime, so a
//! `Menu` item id can't just be the match's own id.
const MATCH_SLOT_SYMBOLS = [
    :match_0, :match_1, :match_2, :match_3, :match_4,
    :match_5, :match_6, :match_7, :match_8, :match_9
];

//! Extract up to `MATCH_SLOT_SYMBOLS.size()` `{"id" => .., "name" => ..}`
//! entries from a `GET /matches` response's `items` array. A blank
//! `name` (a match created without one) falls back to a placeholder —
//! an empty menu label would otherwise be unselectable-looking.
function buildMatchList(items as Array) as Array {
    var matches = [];
    var i = 0;
    while (i < items.size() && i < MATCH_SLOT_SYMBOLS.size()) {
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

function buildMatchMenu(matches as Array) as WatchUi.Menu {
    var menu = new WatchUi.Menu();
    menu.setTitle("Select match");
    var i = 0;
    while (i < matches.size()) {
        var match = matches[i] as Dictionary;
        menu.addItem(match.get("name") as String, MATCH_SLOT_SYMBOLS[i]);
        i += 1;
    }
    return menu;
}

function resolveMatchSlot(item as Symbol, matches as Array) as String? {
    var i = 0;
    while (i < MATCH_SLOT_SYMBOLS.size()) {
        if (MATCH_SLOT_SYMBOLS[i] == item && i < matches.size()) {
            var match = matches[i] as Dictionary;
            return match.get("id") as String;
        }
        i += 1;
    }
    return null;
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
                        new MatchMenuDelegate(matches),
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

//! Input handling for the real match list (`buildMatchMenu`) — resolves
//! the selection back to a match id, fetches that match's full roster,
//! and hands off to the score screen once it's loaded.
class MatchMenuDelegate extends WatchUi.MenuInputDelegate {

    var _matches as Array;
    var _apiClient as MatchApiClient;

    function initialize(matches as Array) {
        MenuInputDelegate.initialize();
        _matches = matches;
        _apiClient = new MatchApiClient();
    }

    function onMenuItem(item as Symbol) as Void {
        var matchId = resolveMatchSlot(item, _matches);
        if (matchId == null) {
            return;
        }
        _apiClient.fetchMatch(matchId, method(:onMatchDetails));
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
