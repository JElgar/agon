import Toybox.Lang;

//! Placeholder squad list, standing in for a real roster fetch that
//! doesn't exist yet (e.g. GET /matches/:id/players) — same "same shape a
//! real thing would have" idea as MockApiClient's doc comment.
//!
//! Fixed at 5 names per side so the goal-scoring menu's item ids
//! (:player_0.. :player_4 — see GoalFlow.mc) can stay plain compile-time
//! symbols rather than something generated dynamically per roster size.
//! A real roster fetch would need a richer id scheme than this fixed-slot
//! one; fine for a 5-name mock.
module MockRoster {
    const HOME_PLAYERS = ["Alex", "Sam", "Jordan", "Taylor", "Casey"];
    const AWAY_PLAYERS = ["Morgan", "Riley", "Drew", "Cameron", "Quinn"];

    function playersFor(side as Symbol) as Array<String> {
        if (side == :home) {
            return HOME_PLAYERS;
        }
        return AWAY_PLAYERS;
    }
}
