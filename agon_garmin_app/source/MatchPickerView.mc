import Toybox.Lang;
import Toybox.WatchUi;
import Toybox.Graphics;
import Toybox.System;
import Toybox.Time;
import Toybox.Time.Gregorian;

//! Shown after picking "Football" on the sport menu, before the score
//! screen — lets the wearer pick which of their matches to actually
//! score. This is a plain text status View (loading / empty / error);
//! once matches actually load it immediately hands off to a real
//! `WatchUi.Menu2` (`buildMatchMenu`/`MatchMenuDelegate`) rather than
//! rendering the list itself.
//!
//! Uses `WatchUi.Menu2`, not the legacy `WatchUi.Menu` this — and every
//! other menu in this app — originally used. `Menu2` is the
//! round-display-aware widget (title and item text both stay clear of
//! the bezel); the legacy `Menu` clipped both a long match name and even
//! the "Select match" title itself against the edge of a real round
//! screen (confirmed from a screenshot), which is what motivated
//! switching every menu in the app over (see `agonMenuDelegate.mc`,
//! `GoalFlow.mc`). It's also why this file needs no slot-symbol
//! indirection trick the way the legacy `Menu` forced — unlike it, a
//! `Menu2` `MenuItem` takes any `Object` as its id, so a match's own
//! string id can be used directly.
const MAX_MATCH_ITEMS = 10;

//! `MenuItem` id for the "More matches..." sentinel `buildMatchMenu` adds
//! as the last row whenever the page just loaded left a `next_cursor` —
//! a Symbol so it can never collide with a real match's (`String`) id.
//! Selecting it is how the wearer pages past the first 3 matches — see
//! `MatchMenuDelegate.onSelect`/`onMoreMatches`.
const LOAD_MORE_ID = :match_picker_load_more;

//! Extract up to `MAX_MATCH_ITEMS` `{"id" => .., "name" => .., "starts_at"
//! => .., "status" => ..}` entries from a `GET /matches` response's
//! `items` array (each a `SearchMatch`). A blank `name` (a match created
//! without one) falls back to a placeholder — an empty menu label would
//! otherwise be unselectable-looking. `starts_at`/`status` are carried
//! through as-is (an RFC-3339 string and a `MatchStatus` wire value like
//! `"in_progress"`) for `sortMatches`/`matchSubLabel` to read.
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
            matches = matches.add({
                "id" => id,
                "name" => name,
                "starts_at" => item.get("starts_at"),
                "status" => item.get("status")
            });
        }
        i += 1;
    }
    return matches;
}

//! Weekday abbreviations for `matchSubLabel`, indexed by
//! `Gregorian.Info.day_of_week - 1` (that field is 1 = Sunday).
const WEEKDAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

//! `starts_at` parsed into a `Time.Moment`, or `null` if it's missing or
//! unparseable — see `parseIso` (`LiveApiClient.mc`), reused here rather
//! than duplicated.
function matchStartsAtMoment(match as Dictionary) as Time.Moment? {
    var iso = match.get("starts_at");
    if (iso == null) {
        return null;
    }
    return parseIso(iso as String);
}

//! Whether the server already considers this match live — the wire value
//! `MatchStatus::InProgress` serializes to.
function matchIsLive(match as Dictionary) as Boolean {
    var status = match.get("status");
    return status != null && (status as String).equals("in_progress");
}

//! Whether `starts_at` falls on the watch's current local calendar day.
function matchIsToday(match as Dictionary) as Boolean {
    var moment = matchStartsAtMoment(match);
    if (moment == null) {
        return false;
    }
    var info = Gregorian.info(moment as Time.Moment, Time.FORMAT_SHORT);
    var nowInfo = Gregorian.info(Time.now(), Time.FORMAT_SHORT);
    return info.year == nowInfo.year && info.month == nowInfo.month && info.day == nowInfo.day;
}

//! The menu row's sub-label: "Live now" for an in-progress match
//! (status, not time, is what the wearer needs to see — the kickoff time
//! of a match already underway isn't useful), otherwise the local
//! kickoff time, "Today HH:MM" for today's matches and "<weekday>
//! HH:MM" otherwise. "" if `starts_at` is missing/unparseable.
function matchSubLabel(match as Dictionary) as String {
    if (matchIsLive(match)) {
        return "Live now";
    }
    var moment = matchStartsAtMoment(match);
    if (moment == null) {
        return "";
    }
    var info = Gregorian.info(moment as Time.Moment, Time.FORMAT_SHORT);
    var timePart = Lang.format("$1$:$2$", [info.hour.format("%02d"), info.min.format("%02d")]);
    if (matchIsToday(match)) {
        return "Today " + timePart;
    }
    return WEEKDAY_NAMES[info.day_of_week - 1] + " " + timePart;
}

//! Orders the picker list: today's matches first, then live matches,
//! then everything else by `starts_at` ascending (soonest first). A
//! plain insertion sort, since `matches` is at most a handful of pages'
//! worth and the rest of this file already favors manual loops over
//! higher-order `Array` methods.
function sortMatches(matches as Array) as Array {
    var sorted = matches.slice(0, null);
    var i = 1;
    while (i < sorted.size()) {
        var j = i;
        while (j > 0 && matchBefore(sorted[j] as Dictionary, sorted[j - 1] as Dictionary)) {
            var tmp = sorted[j];
            sorted[j] = sorted[j - 1];
            sorted[j - 1] = tmp;
            j -= 1;
        }
        i += 1;
    }
    return sorted;
}

//! True if `a` should sort before `b` under `sortMatches`'s ordering.
function matchBefore(a as Dictionary, b as Dictionary) as Boolean {
    var aToday = matchIsToday(a);
    var bToday = matchIsToday(b);
    if (aToday != bToday) {
        return aToday;
    }
    var aLive = matchIsLive(a);
    var bLive = matchIsLive(b);
    if (aLive != bLive) {
        return aLive;
    }
    var aMoment = matchStartsAtMoment(a);
    var bMoment = matchStartsAtMoment(b);
    if (aMoment == null || bMoment == null) {
        return false;
    }
    return (aMoment as Time.Moment).value() < (bMoment as Time.Moment).value();
}

//! Builds the match-picker menu from every match loaded so far (the
//! first page, plus any further pages appended via the "More matches..."
//! sentinel), ordered by `sortMatches` rather than fetch order. `hasMore`
//! — whether the page that produced `matches` came back with a
//! `next_cursor` — controls whether that sentinel is appended as the
//! last row (always after every real match, regardless of sort order).
function buildMatchMenu(matches as Array, hasMore as Boolean) as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => "Select match" });
    var sorted = sortMatches(matches);
    var i = 0;
    while (i < sorted.size()) {
        var match = sorted[i] as Dictionary;
        menu.addItem(new WatchUi.MenuItem(
            match.get("name") as String,
            matchSubLabel(match),
            match.get("id") as String,
            {}
        ));
        i += 1;
    }
    if (hasMore) {
        menu.addItem(new WatchUi.MenuItem("More matches...", null, LOAD_MORE_ID, {}));
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
    //! Set once `onUserId` resolves, so `onMatches` can hand it to
    //! `MatchMenuDelegate` — every later page fetch (via the "More
    //! matches..." sentinel) reuses it rather than re-resolving `/users/me`.
    var _userId as String?;

    function initialize() {
        View.initialize();
        _apiClient = new MatchApiClient();
        _statusLine1 = "Loading matches...";
        _statusLine2 = "";
        _userId = null;
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
                    _userId = id as String;
                    _apiClient.fetchMatches(_userId as String, null, method(:onMatches));
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
                    var nextCursor = dict.get("next_cursor") as String or Null;
                    WatchUi.switchToView(
                        buildMatchMenu(matches, nextCursor != null),
                        new MatchMenuDelegate(matches, _userId as String, nextCursor),
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
//!
//! Also owns paging: `_matches` is every match loaded so far (across all
//! pages) and `_nextCursor` is the cursor for the page after that, so
//! selecting the "More matches..." sentinel (`LOAD_MORE_ID`) can fetch
//! the next `limit=3` page and rebuild the menu from the combined list —
//! see `onMoreMatches`.
class MatchMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _apiClient as MatchApiClient;
    var _matches as Array;
    var _userId as String;
    var _nextCursor as String?;

    function initialize(matches as Array, userId as String, nextCursor as String?) {
        Menu2InputDelegate.initialize();
        _apiClient = new MatchApiClient();
        _matches = matches;
        _userId = userId;
        _nextCursor = nextCursor;
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var id = item.getId();
        if (id == null) {
            return;
        }
        if (id == LOAD_MORE_ID) {
            _apiClient.fetchMatches(_userId, _nextCursor, method(:onMoreMatches));
            return;
        }
        _apiClient.fetchMatch(id as String, method(:onMatchDetails));
    }

    //! Response to selecting "More matches...": fetches the next page
    //! after `_nextCursor`, appends it to `_matches`, and switches to a
    //! freshly built menu (`buildMatchMenu`) covering every match loaded
    //! so far, with a new sentinel appended if the server says there's
    //! still more. On failure this just leaves the current menu — and its
    //! sentinel — in place, so selecting it again retries.
    function onMoreMatches(responseCode as Number, data as Dictionary or String or Null) as Void {
        System.println("[match-picker] /matches (more) responseCode=" + responseCode);
        if (responseCode != 200 || data == null) {
            return;
        }
        var dict = data as Dictionary;
        var items = dict.get("items");
        if (items == null) {
            return;
        }
        var newMatches = buildMatchList(items as Array);
        var i = 0;
        while (i < newMatches.size()) {
            _matches = _matches.add(newMatches[i]);
            i += 1;
        }
        _nextCursor = dict.get("next_cursor") as String or Null;
        WatchUi.switchToView(
            buildMatchMenu(_matches, _nextCursor != null),
            new MatchMenuDelegate(_matches, _userId, _nextCursor),
            WatchUi.SLIDE_UP
        );
    }

    function onMatchDetails(responseCode as Number, data as Dictionary or String or Null) as Void {
        if (responseCode == 200 && data != null) {
            var match = data as Dictionary;
            // Before populateFrom, so its own orientSides call has
            // something to orient by — see MatchContext.myUserId's doc
            // comment on why this is set once here rather than passed
            // into populateFrom itself.
            getApp().matchContext.setMyUserId(_userId);
            getApp().matchContext.populateFrom(match);
            getApp().liveApiClient.setMatch(getApp().matchContext.matchId);
            // Start acquiring a GPS fix now, while the wearer waits for
            // kick-off — see ActivityRecorder.enableGps.
            getApp().activityRecorder.enableGps();
            WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_LEFT);
            return;
        }
        // Failure: nothing better to do in this basics-only prototype than
        // go back to the picker and let the wearer retry or pick again.
        var pickerView = new MatchPickerView();
        WatchUi.switchToView(pickerView, new MatchPickerDelegate(pickerView), WatchUi.SLIDE_RIGHT);
    }
}
