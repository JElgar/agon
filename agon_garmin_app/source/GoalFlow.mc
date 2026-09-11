import Toybox.Lang;
import Toybox.WatchUi;

//! The "Goal" menu flow: pick a side, then (optionally) who scored, then
//! (optionally) who assisted — three chained `WatchUi.Menu` screens
//! rather than one, since a 5-button watch can't usefully show
//! "side + scorer + assist" as a single list. Built programmatically
//! (not a menu.xml resource) because the scorer/assist lists come from
//! `MockRoster` today and a real per-match roster fetch tomorrow —
//! neither is knowable at resource-compile time the way a fixed menu.xml
//! is.
//!
//! Relies on the same per-selection auto-dismiss behavior every menu in
//! this app already depends on (see `agonMenuDelegate` — no explicit
//! `popView` calls there either): a `WatchUi.Menu` pops itself the moment
//! an item is chosen, so having the next menu already pushed in its place
//! by then is what keeps the view stack from growing at every step —
//! after the final (assist) selection, that self-dismiss lands you back
//! on the score screen, not three menus deep. If that assumption turns
//! out to be wrong once this actually runs (extra menus stack up, or a
//! step closes when it shouldn't), the fix is explicit `WatchUi.popView`
//! calls here rather than relying on it.
//!
//! Item ids are fixed symbols (`:player_0`.. `:player_4`, `:unknown`,
//! `:none`) rather than the player's own name, so `onMenuItem`'s
//! signature can stay a plain `Symbol` — matching
//! `WatchUi.MenuInputDelegate` exactly — with the symbol resolved back to
//! a name via `MockRoster`'s array index.

const PLAYER_SLOT_SYMBOLS = [:player_0, :player_1, :player_2, :player_3, :player_4];

//! `null` for `:unknown`/`:none` (the "skip this" placeholder both the
//! scorer and assist menus use), otherwise the resolved player name.
function resolvePlayerSlot(side as Symbol, item as Symbol) as String? {
    if (item == :unknown || item == :none) {
        return null;
    }
    var players = MockRoster.playersFor(side);
    var i = 0;
    while (i < PLAYER_SLOT_SYMBOLS.size()) {
        if (PLAYER_SLOT_SYMBOLS[i] == item && i < players.size()) {
            return players[i];
        }
        i += 1;
    }
    return null;
}

function buildSideMenu() as WatchUi.Menu {
    var menu = new WatchUi.Menu({ :title => "Goal" });
    menu.addItem(new WatchUi.MenuItem("Home", null, :home, {}));
    menu.addItem(new WatchUi.MenuItem("Away", null, :away, {}));
    return menu;
}

//! Shared builder for the scorer/assist menus — same roster, different
//! title and "skip this" placeholder (Unknown vs. No assist).
function buildPlayerMenu(
    title as String,
    side as Symbol,
    placeholderLabel as String,
    placeholderId as Symbol
) as WatchUi.Menu {
    var menu = new WatchUi.Menu({ :title => title });
    var players = MockRoster.playersFor(side);
    var i = 0;
    while (i < players.size() && i < PLAYER_SLOT_SYMBOLS.size()) {
        menu.addItem(new WatchUi.MenuItem(players[i], null, PLAYER_SLOT_SYMBOLS[i], {}));
        i += 1;
    }
    menu.addItem(new WatchUi.MenuItem(placeholderLabel, null, placeholderId, {}));
    return menu;
}

class GoalSideMenuDelegate extends WatchUi.MenuInputDelegate {

    function initialize() {
        MenuInputDelegate.initialize();
    }

    function onMenuItem(item as Symbol) as Void {
        // item is :home or :away — that symbol IS the side value the rest
        // of the flow needs, no resolving required.
        WatchUi.pushView(
            buildPlayerMenu("Scorer", item, "Unknown", :unknown),
            new GoalScorerMenuDelegate(item),
            WatchUi.SLIDE_UP
        );
    }
}

class GoalScorerMenuDelegate extends WatchUi.MenuInputDelegate {

    var _side as Symbol;

    function initialize(side as Symbol) {
        MenuInputDelegate.initialize();
        _side = side;
    }

    function onMenuItem(item as Symbol) as Void {
        var scorer = resolvePlayerSlot(_side, item);
        WatchUi.pushView(
            buildPlayerMenu("Assist", _side, "No assist", :none),
            new GoalAssistMenuDelegate(_side, scorer),
            WatchUi.SLIDE_UP
        );
    }
}

class GoalAssistMenuDelegate extends WatchUi.MenuInputDelegate {

    var _side as Symbol;
    var _scorer as String?;

    function initialize(side as Symbol, scorer as String?) {
        MenuInputDelegate.initialize();
        _side = side;
        _scorer = scorer;
    }

    function onMenuItem(item as Symbol) as Void {
        var assist = resolvePlayerSlot(_side, item);
        getApp().score.recordGoal(_side, _scorer, assist);
        WatchUi.requestUpdate();
    }
}
