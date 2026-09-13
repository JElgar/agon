import Toybox.Lang;

//! Real per-match data — sides and per-side player rosters — fetched once
//! a match is picked in MatchPickerView (`MatchApiClient.fetchMatch`).
//! Replaces MockRoster now that a real scoring event needs real ids
//! (`side_id`, `scorer_player_id`, `assist_player_id` — see
//! `agon_service::detailed_score::football::FootballGoalEvent`), not
//! MockRoster's display-only fake names.
//!
//! `side0`/`side1` map onto GoalFlow's existing `:home`/`:away` Symbol
//! convention purely as a stable "first/second side" UI label —
//! `Match.sides` has no actual home/away concept, so this is just array
//! position (`sides[0]`/`sides[1]`), not a meaningful distinction on the
//! server side.
//!
//! An instance held on `agonApp` (`getApp().matchContext`), not a
//! `module`/`static`-anything: this needs plain mutable state written
//! once (by the picker) and read from several other files afterwards,
//! and instance vars accessed through `self` are the one pattern in this
//! app already proven to work everywhere (unlike a class's own `const`
//! from a `static function` — see DeviceAuth.mc's doc comment).
class MatchContext {

    var matchId as String;
    var side0Id as String;
    var side0Name as String;
    var side1Id as String;
    var side1Name as String;
    //! Each entry is a Dictionary{"id" => String, "name" => String}.
    var side0Players as Array;
    var side1Players as Array;

    function initialize() {
        matchId = "";
        side0Id = "";
        side0Name = "Home";
        side1Id = "";
        side1Name = "Away";
        side0Players = [];
        side1Players = [];
    }

    //! Parse a `GET /matches/:id` response body into this context. Assumes
    //! exactly two sides — `Match.sides` is documented as "always present
    //! — score needs them", and football always has exactly two.
    function populateFrom(match as Dictionary) as Void {
        var id = match.get("id");
        if (id != null) {
            matchId = id as String;
        }

        var sides = match.get("sides");
        if (sides != null) {
            var sidesArray = sides as Array;
            if (sidesArray.size() > 0) {
                populateSide(sidesArray[0] as Dictionary, true);
            }
            if (sidesArray.size() > 1) {
                populateSide(sidesArray[1] as Dictionary, false);
            }
        }

        side0Players = [];
        side1Players = [];
        var players = match.get("players");
        if (players != null) {
            var playersArray = players as Array;
            var i = 0;
            while (i < playersArray.size()) {
                addPlayer(playersArray[i] as Dictionary);
                i += 1;
            }
        }
    }

    function playersFor(side as Symbol) as Array {
        if (side == :home) {
            return side0Players;
        }
        return side1Players;
    }

    function sideIdFor(side as Symbol) as String {
        if (side == :home) {
            return side0Id;
        }
        return side1Id;
    }

    function sideNameFor(side as Symbol) as String {
        if (side == :home) {
            return side0Name;
        }
        return side1Name;
    }

    function populateSide(side as Dictionary, isFirst as Boolean) as Void {
        var id = side.get("id");
        var name = side.get("name");
        if (isFirst) {
            if (id != null) {
                side0Id = id as String;
            }
            if (name != null) {
                side0Name = name as String;
            }
        } else {
            if (id != null) {
                side1Id = id as String;
            }
            if (name != null) {
                side1Name = name as String;
            }
        }
    }

    //! Appends to `side0Players`/`side1Players` — a no-op for a player with
    //! no `side_id` yet (invited but unassigned) or one on neither known
    //! side.
    function addPlayer(player as Dictionary) as Void {
        var sideId = player.get("side_id");
        if (sideId == null) {
            return;
        }
        var entry = playerEntry(player);
        if (entry == null) {
            return;
        }
        var sideIdStr = sideId as String;
        if (sideIdStr.equals(side0Id)) {
            side0Players = side0Players.add(entry);
        } else if (sideIdStr.equals(side1Id)) {
            side1Players = side1Players.add(entry);
        }
    }

    //! `null` for a malformed `member` entry — shouldn't happen against a
    //! real API response, but this parses arbitrary JSON, so it's not
    //! assumed. `member` is a discriminated union (see `agon_service`'s
    //! `Member`): a linked account's display name is `name`, an external
    //! (no-account) player's is `display_name`.
    function playerEntry(player as Dictionary) as Dictionary? {
        var member = player.get("member");
        if (member == null) {
            return null;
        }
        var memberDict = member as Dictionary;
        var id = memberDict.get("id");
        if (id == null) {
            return null;
        }
        var name = memberName(memberDict);
        if (name == null) {
            name = "Player";
        }
        return { "id" => id, "name" => name };
    }

    //! The display name out of a `member` Dictionary — `name` for a linked
    //! account, `display_name` for an external (no-account) player. A
    //! plain function (not inlined into `playerEntry`) so `name`'s local
    //! type is inferred from this function's own declared return type
    //! rather than from a bare `null` literal — Monkey C doesn't allow
    //! explicit `as Type` annotations on local variables at all ("Local
    //! variable types are inferred" — a real compiler error hit while
    //! writing this), so a local that starts out possibly-null needs to
    //! infer that from somewhere with a real declared type.
    function memberName(member as Dictionary) as Object? {
        var type = member.get("type");
        if (type != null && (type as String).equals("User")) {
            return member.get("name");
        }
        return member.get("display_name");
    }
}
