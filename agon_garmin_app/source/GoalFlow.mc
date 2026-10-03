import Toybox.Lang;
import Toybox.WatchUi;

//! The "Goal" menu flow: pick a side, then (optionally) who scored, then
//! (optionally) who assisted — three chained screens rather than one,
//! since a 5-button watch can't usefully show "side + scorer + assist" as
//! a single list. Built programmatically (not a menu.xml resource)
//! because the side names and scorer/assist rosters come from
//! `MatchContext` (the picked match's real data — see MatchPickerView),
//! which isn't knowable at resource-compile time the way a fixed menu.xml
//! is.
//!
//! Side -> scorer and scorer -> assist each use `WatchUi.pushView` to
//! layer the next step on top of the one before it, so Back steps
//! backward through side/scorer/assist one screen at a time instead of
//! exiting straight past the whole flow. The final step (assist picked)
//! instead `switchToView`s all the way back to a fresh `agonView`,
//! collapsing the whole pushed chain at once — same mechanism
//! `SportMenuDelegate` uses to get from the sport picker into the score
//! screen, appropriate here too since the flow is actually finished and
//! there's nothing left to step back into.
//!
//! An earlier version of this chain also used `pushView` but assumed each
//! menu would pop itself on selection, landing the flow back where it
//! started once the last one did the same — confirmed wrong on a real
//! device: it oscillated between the scorer and assist screens instead.
//! The difference here is that nothing is ever popped mid-flow (`Menu2`
//! doesn't self-dismiss on select — see `agonMenuDelegate.onSelect`'s own
//! doc comment); each step only ever pushes forward, so there's no pop to
//! race against.
//!
//! Uses `WatchUi.Menu2` (not the legacy `WatchUi.Menu` this flow used
//! originally) — `Menu2` is the round-display-aware widget (see
//! `MatchPickerView.mc`'s doc comment for the screenshot-confirmed
//! clipping the legacy `Menu` caused there); a long player name here
//! would hit the same bezel-clipping bug. It's also why the old
//! `PLAYER_SLOT_SYMBOLS`/`resolvePlayerSlot` index-based indirection is
//! gone: unlike the legacy `Menu`, a `Menu2` `MenuItem` takes any
//! `Object` as its id, so a player's own real id can be used directly —
//! `:unknown`/`:none` sentinel symbols still stand in for "skip this".

//! `null` if `id` is the "skip this" placeholder sentinel (`:unknown` on
//! the scorer menu, `:none` on the assist menu — see `buildPlayerMenu`),
//! otherwise the real player id a `MenuItem` carries directly.
function playerIdFromSelection(id as Object?, placeholder as Symbol) as String? {
    if (id == null || id == placeholder) {
        return null;
    }
    return id as String;
}

function buildSideMenu() as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => "Goal" });
    menu.addItem(new WatchUi.MenuItem(getApp().matchContext.sideNameFor(:home), null, :home, {}));
    menu.addItem(new WatchUi.MenuItem(getApp().matchContext.sideNameFor(:away), null, :away, {}));
    return menu;
}

//! Shared builder for the scorer/assist menus — same roster, different
//! title and "skip this" placeholder (Unknown vs. No assist). The skip
//! option always sits last in the list — the only control guaranteed to
//! exist on every device this app targets (including the touchscreen-only
//! ones with no extra buttons) is select/back, and repurposing Back for
//! "skip" would be an undiscoverable, screen-dependent shortcut; an
//! explicit trailing list item needs no explanation.
//!
//! `excludePlayerId` drops one player from the list — used so the assist
//! menu can't offer the same player who was just picked as scorer (a
//! player can't assist their own goal). `null` shows the full roster, as
//! the scorer menu always does (nothing to exclude yet).
function buildPlayerMenu(
    title as String,
    side as Symbol,
    placeholderLabel as String,
    placeholderId as Symbol,
    excludePlayerId as String?
) as WatchUi.Menu2 {
    var menu = new WatchUi.Menu2({ :title => title });
    var players = getApp().matchContext.playersFor(side);
    var i = 0;
    while (i < players.size()) {
        var player = players[i] as Dictionary;
        var id = player.get("id") as String;
        if (excludePlayerId == null || !id.equals(excludePlayerId)) {
            menu.addItem(new WatchUi.MenuItem(player.get("name") as String, null, id, {}));
        }
        i += 1;
    }
    menu.addItem(new WatchUi.MenuItem(placeholderLabel, null, placeholderId, {}));
    return menu;
}

class GoalSideMenuDelegate extends WatchUi.Menu2InputDelegate {

    function initialize() {
        Menu2InputDelegate.initialize();
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        // The id IS :home or :away — that's the side value the rest of
        // the flow needs, no resolving required.
        var side = item.getId() as Symbol;
        WatchUi.pushView(
            buildPlayerMenu("Scorer", side, "Unknown", :unknown, null),
            new GoalScorerMenuDelegate(side),
            WatchUi.SLIDE_UP
        );
    }
}

class GoalScorerMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _side as Symbol;

    function initialize(side as Symbol) {
        Menu2InputDelegate.initialize();
        _side = side;
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var scorer = playerIdFromSelection(item.getId(), :unknown);
        WatchUi.pushView(
            // Exclude the scorer — a player can't assist their own goal.
            // scorer is null when they were left "Unknown", in which case
            // there's nothing to exclude.
            buildPlayerMenu("Assist", _side, "No assist", :none, scorer),
            new GoalAssistMenuDelegate(_side, scorer),
            WatchUi.SLIDE_UP
        );
    }
}

class GoalAssistMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _side as Symbol;
    var _scorer as String?;

    function initialize(side as Symbol, scorer as String?) {
        Menu2InputDelegate.initialize();
        _side = side;
        _scorer = scorer;
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var assist = playerIdFromSelection(item.getId(), :none);
        getApp().score.recordGoal(_side, _scorer, assist);
        // Back to the score screen — a fresh agonView/agonDelegate is
        // fine, it reads app.score straight from the singleton app on
        // every onUpdate rather than carrying its own state.
        WatchUi.switchToView(new agonView(), new agonDelegate(), WatchUi.SLIDE_DOWN);
    }
}
