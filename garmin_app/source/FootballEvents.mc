using Toybox.Lang;
using Toybox.Time;
using Toybox.Time.Gregorian;

//! Builds the JSON-shaped dictionaries `agon_service` expects on
//! `POST /matches/:id/live/events` — see `agon_service/src/live_score/`
//! (the outer `sport`/`kind` discriminators) and
//! `agon_service/src/detailed_score/football.rs` (the field names/enum
//! wire values, all checked directly against that Rust source rather than
//! guessed). Deliberately dumb: this module only shapes data, never talks
//! to the network or touches storage — see `AgonApiClient`/`EventQueue`
//! for those.
//!
//! v1 only builds `Goal` and `Period` events — see the README's
//! "Deliberately not in v1" section for why `Card`/`Substitution` (which
//! both require picking a specific player) aren't here yet.
module FootballEvents {

    // Wire values for `FootballPeriod` — must match
    // `agon_service/src/detailed_score/football.rs`'s `Display`/`FromStr`
    // impl (snake_case) exactly, not the Rust variant names.
    const PERIOD_KICK_OFF = "kick_off";
    const PERIOD_HALF_TIME = "half_time";
    const PERIOD_SECOND_HALF_KICK_OFF = "second_half_kick_off";
    const PERIOD_FULL_TIME = "full_time";
    const PERIOD_EXTRA_TIME_KICK_OFF = "extra_time_kick_off";
    const PERIOD_EXTRA_TIME_HALF_TIME = "extra_time_half_time";
    const PERIOD_EXTRA_TIME_SECOND_HALF_KICK_OFF = "extra_time_second_half_kick_off";
    const PERIOD_EXTRA_TIME_FULL_TIME = "extra_time_full_time";
    const PERIOD_PENALTIES_COMPLETE = "penalties_complete";

    //! A `FootballLiveEvent::Goal` — the inner (`kind`-discriminated) event,
    //! not yet wrapped in the `NewLiveEventInput` envelope (see
    //! `wrapNewEvent`). `scorer_player_id`/`assist_player_id` are always
    //! null in v1 (no on-watch roster picker yet); `minute` is left null
    //! too since the server derives the displayed minute from the
    //! envelope's own `occurred_at`, not this field (see
    //! `FootballGoalEvent::occurred_at`'s doc comment on the Rust side).
    function goalEvent(sideId as String, ownGoal as Boolean) as Dictionary {
        return {
            "sport" => "Football",
            "kind" => "Goal",
            "side_id" => sideId,
            "scorer_player_id" => null,
            "assist_player_id" => null,
            "own_goal" => ownGoal,
            "penalty" => false,
            "minute" => null,
            "occurred_at" => null,
        };
    }

    //! A `FootballLiveEvent::Period` marker — one of the `PERIOD_*`
    //! constants above.
    function periodEvent(period as String) as Dictionary {
        return {
            "sport" => "Football",
            "kind" => "Period",
            "period" => period,
        };
    }

    //! Wrap an inner event (from `goalEvent`/`periodEvent`) in the
    //! `NewLiveEventInput` envelope `POST /matches/:id/live/events` expects,
    //! stamping `occurred_at` with the current UTC instant.
    function wrapNewEvent(event as Dictionary) as Dictionary {
        return {
            "occurred_at" => nowIso8601(),
            "event" => event,
        };
    }

    //! The current UTC instant as an RFC-3339 string
    //! (`"2026-06-01T10:00:00Z"`) — what `chrono::DateTime<Utc>` parses on
    //! the server side. Monkey C has no built-in RFC-3339 formatter, so
    //! this builds it by hand from `Gregorian.utcInfo` (explicitly UTC, not
    //! the device's local-timezone `Gregorian.info` — the server assumes
    //! UTC and getting this wrong would silently shift every event's
    //! displayed minute).
    function nowIso8601() as String {
        var info = Gregorian.utcInfo(Time.now(), Time.FORMAT_SHORT);
        return Lang.format(
            "$1$-$2$-$3$T$4$:$5$:$6$Z",
            [
                info.year.format("%04d"),
                info.month.format("%02d"),
                info.day.format("%02d"),
                info.hour.format("%02d"),
                info.min.format("%02d"),
                info.sec.format("%02d"),
            ]
        );
    }
}
