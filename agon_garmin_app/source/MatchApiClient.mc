import Toybox.Lang;
import Toybox.Communications;

//! Authenticated calls the watch makes once paired, to pick a match to
//! score: resolving the account's own id (`GET /users/me`), listing
//! matches it can pick from (`GET /matches`), and loading one match's
//! full roster (`GET /matches/:id`) — see MatchPickerView. Kept separate
//! from `PairingApiClient` (unauthenticated, runs before there's any
//! token to send) and `LiveApiClient` (posts live-scoring events) even
//! though all three ultimately hit the same `agon_service` — each has a
//! distinct auth/lifecycle shape.
//!
//! All three endpoints here are scoped `live_scoring` on the server (see
//! `agon_service::main::check_scope`), so the device's own access token
//! (not just any bearer token) is what makes these calls succeed.
class MatchApiClient {

    //! Same base URL as `PairingApiClient` — see its own doc comment on
    //! the `/api` prefix and the local-dev alternative.
    const API_BASE_URL = "https://agon.staging.get-agon.com/api";

    //! `GET /users/me` — resolves the paired account's own internal user
    //! id, needed to filter `GET /matches` to "matches I'm on". `callback`
    //! is invoked as `(responseCode as Number, data as Dictionary or
    //! String or Null)`.
    function fetchMyUserId(callback as Method) as Void {
        var url = API_BASE_URL + "/users/me";
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => authHeaders(),
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        Communications.makeWebRequest(url, null, options, callback);
    }

    //! `GET /matches?participant=<userId>&match_type=football&limit=3[&cursor=..]`
    //! — one page of the picker's list. Football only, and deliberately
    //! small: each `SearchMatch` item carries description/photos/side
    //! rosters/social counts, and a `limit=10` response tripped
    //! `Communications.NETWORK_RESPONSE_TOO_LARGE` (response code -402,
    //! confirmed from a real simulator run — that's a
    //! `Communications`-layer error, not an HTTP status). Rather than
    //! trying to ask the server for a slimmer shape, this keeps the small
    //! page size and instead pages through as many as the wearer actually
    //! scrolls to (see `MatchPickerView`/`MatchMenuDelegate`'s "More
    //! matches..." sentinel), passing back the previous page's
    //! `next_cursor` unchanged — same opaque-cursor contract as
    //! `LiveApiClient.fetchLiveEventsPage`. `cursor` is null for the
    //! first page.
    function fetchMatches(userId as String, cursor as String?, callback as Method) as Void {
        var url = API_BASE_URL + "/matches";
        var params = {
            "participant" => userId,
            "match_type" => "football",
            "limit" => "3"
        };
        if (cursor != null) {
            params.put("cursor", cursor);
        }
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => authHeaders(),
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        Communications.makeWebRequest(url, params, options, callback);
    }

    //! `GET /matches/:id` — the full match, for its sides + player roster
    //! (`MatchContext.populateFrom`).
    function fetchMatch(matchId as String, callback as Method) as Void {
        var url = API_BASE_URL + "/matches/" + matchId;
        var options = {
            :method => Communications.HTTP_REQUEST_METHOD_GET,
            :headers => authHeaders(),
            :responseType => Communications.HTTP_RESPONSE_CONTENT_TYPE_JSON
        };
        Communications.makeWebRequest(url, null, options, callback);
    }

    function authHeaders() as Dictionary {
        return { "Authorization" => "Bearer " + DeviceAuth.getAccessToken() };
    }
}
