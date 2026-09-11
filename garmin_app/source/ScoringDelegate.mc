using Toybox.WatchUi;
using Toybox.Lang;

//! Input handling for `ScoringView`. Select opens the scoring menu — but
//! only once the device is paired *and* configured with a match, since
//! there's nothing meaningful to act on before that (the view itself is
//! showing the pairing/config prompt at that point).
//!
//! `WatchUi.MenuItem`'s exact constructor arity hasn't been checked
//! against a real SDK build in this sandbox — verify against
//! `Toybox.WatchUi.MenuItem`'s docs for your `minSdkVersion` if the
//! compiler flags it.
class ScoringDelegate extends WatchUi.BehaviorDelegate {

    var _view as ScoringView;

    function initialize(view as ScoringView) {
        BehaviorDelegate.initialize();
        _view = view;
    }

    function onSelect() as Boolean {
        if (!AgonLiveScoringApp.get().isConfigured()) {
            return true;
        }

        var menu = new WatchUi.Menu2({ :title => WatchUi.loadResource(Rez.Strings.AppName) });
        menu.addItem(new WatchUi.MenuItem(WatchUi.loadResource(Rez.Strings.MenuGoalHome), null, :goalHome, {}));
        menu.addItem(new WatchUi.MenuItem(WatchUi.loadResource(Rez.Strings.MenuGoalAway), null, :goalAway, {}));
        menu.addItem(new WatchUi.MenuItem(WatchUi.loadResource(Rez.Strings.MenuPeriod), null, :period, {}));
        menu.addItem(new WatchUi.MenuItem(WatchUi.loadResource(Rez.Strings.MenuSyncNow), null, :syncNow, {}));
        WatchUi.pushView(menu, new ScoringMenuDelegate(_view), WatchUi.SLIDE_UP);
        return true;
    }
}

//! The top-level scoring menu (Goal — Home/Away, period marker submenu,
//! manual sync). Card/Substitution aren't here — see the README's
//! "Deliberately not in v1" section.
class ScoringMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _view as ScoringView;

    function initialize(view as ScoringView) {
        Menu2InputDelegate.initialize();
        _view = view;
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        var id = item.getId();
        var app = AgonLiveScoringApp.get();

        if (id == :goalHome) {
            _view.recordGoal(app.homeSideId(), false);
        } else if (id == :goalAway) {
            _view.recordGoal(app.awaySideId(), false);
        } else if (id == :period) {
            WatchUi.pushView(buildPeriodMenu(), new PeriodMenuDelegate(_view), WatchUi.SLIDE_UP);
        } else if (id == :syncNow) {
            _view.flushQueue();
        }
    }

    //! One item per `FootballEvents.PERIOD_*` constant, in match order.
    //! The item id *is* the wire-value string, so `PeriodMenuDelegate`
    //! needs no further mapping.
    function buildPeriodMenu() as WatchUi.Menu2 {
        var menu = new WatchUi.Menu2({ :title => WatchUi.loadResource(Rez.Strings.MenuPeriod) });
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodKickOff), null, FootballEvents.PERIOD_KICK_OFF, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodHalfTime), null, FootballEvents.PERIOD_HALF_TIME, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodSecondHalfKickOff), null,
            FootballEvents.PERIOD_SECOND_HALF_KICK_OFF, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodFullTime), null, FootballEvents.PERIOD_FULL_TIME, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodExtraTimeKickOff), null,
            FootballEvents.PERIOD_EXTRA_TIME_KICK_OFF, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodExtraTimeHalfTime), null,
            FootballEvents.PERIOD_EXTRA_TIME_HALF_TIME, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodExtraTimeSecondHalfKickOff), null,
            FootballEvents.PERIOD_EXTRA_TIME_SECOND_HALF_KICK_OFF, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodExtraTimeFullTime), null,
            FootballEvents.PERIOD_EXTRA_TIME_FULL_TIME, {}));
        menu.addItem(new WatchUi.MenuItem(
            WatchUi.loadResource(Rez.Strings.PeriodPenaltiesComplete), null,
            FootballEvents.PERIOD_PENALTIES_COMPLETE, {}));
        return menu;
    }
}

//! The period submenu's selection handler.
class PeriodMenuDelegate extends WatchUi.Menu2InputDelegate {

    var _view as ScoringView;

    function initialize(view as ScoringView) {
        Menu2InputDelegate.initialize();
        _view = view;
    }

    function onSelect(item as WatchUi.MenuItem) as Void {
        _view.recordPeriod(item.getId());
        WatchUi.popView(WatchUi.SLIDE_DOWN);
    }
}
