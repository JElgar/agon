# Garmin live scoring

Goal: a Garmin watch app that lets someone score a football match live
(goals, cards, subs, period markers) from their wrist, while — ideally —
still recording their own GPS/HR activity for that session like any other
Garmin workout. This doc lays out the options considered, the approach
chosen, and what's actually built so far vs. left as follow-up.

## What Agon already has

Nothing about live scoring itself needed inventing: `agon_service` already
has a full event-sourced live-scoring API (`agon_service/src/live_score/`,
`agon_service/src/detailed_score/`) —

- `POST /matches/:id/live/events` appends a batch of events
  (`AppendLiveEventsInput`), guarded by an `expected_last_seq` so a client
  that was offline can catch up in one call and a genuine conflict (two
  devices, a double-entry) is rejected rather than silently reordered.
- `GET /matches/:id/score` reads the derived `Score` back — live or
  finished, same shape either way.
- `DELETE /matches/:id/live/events/:seq` undoes the most recent event.
- Football's event vocabulary (`FootballLiveEvent`: `Goal`, `Card`,
  `Substitution`, `Period`, `PenaltyShootoutKick`) is exactly the button
  grid a scorer needs, already typed and validated server-side.

In other words, the watch app doesn't need a bespoke backend — it needs to
become an HTTP client of an API that was already designed for exactly this
"intermittently-connected device catching up on a log" shape. The one thing
that *didn't* exist yet, and blocks a watch from using any of it, is
**authentication**.

## The actual blocker: how does a watch log in?

Every existing Agon endpoint is a bearer-token API, and the only way to get
a token is Supabase's normal auth flow (email/password or OAuth, in a
browser). A Garmin watch has no browser, no keyboard beyond a handful of
buttons, and Connect IQ apps have no way to embed a real OAuth flow. So
before any scoring logic matters, something has to answer: *how does a
Garmin watch identify itself as "this Agon user", without ever handling
that user's real password?*

Options considered:

1. **Type credentials on the watch.** Rejected outright — a 5-button watch
   with no keyboard makes typing an email+password (or worse, an OAuth
   redirect) painful to the point of unusable.
2. **A device-linking API key set once, in the Agon web/mobile app,
   copy-pasted into the watch's Connect IQ settings.** Workable, but a
   long-lived API key an ordinary user has to copy correctly by hand is
   exactly the kind of thing that leaks into a screenshot or a support
   message; a short one-time code is safer and just as easy to type.
3. **Pairing code, originated by the device and confirmed by an
   already-logged-in client.** Chosen approach — but see below for the two
   different directions this can flow, and why the device-originated one
   won out. Either way, it never exposes the user's real credentials to
   the device, and the thing being typed/scanned is short-lived and
   single-use rather than a durable secret.

### What's built: device-originated, QR-first pairing

The device (not an already-logged-in phone/web client) starts pairing:

- The watch generates its own short, human-typeable code locally (6
  characters, the same 32-symbol alphabet the backend used to generate —
  visually ambiguous characters dropped — before code generation moved to
  the device; not yet implemented, see "What's left to build" below) and
  displays it two ways: a QR code (fetched from `GET
  /devices/pairing-codes/:code/qr.png`, a pure rendering endpoint — no
  auth, no database access at all, see `agon_service/src/qr.rs`) encoding
  a link to `agon_ui`'s pairing page, and the bare code as a scan-fails
  fallback.
- Someone scans (or types the code into) `agon_ui`'s `/pair` page, logs in
  if needed, and confirms — this calls `POST
  /devices/pairing-codes/:code/confirm` (bearer auth, the logged-in
  user's own session), which is the **first** point anything gets written
  to the database: it creates the pairing record in full (code, the
  confirming user's id, a freshly reserved `device_sub`) in one shot.
  There's no separate "pending, no owner yet" state — nothing exists
  before confirmation.
- The watch, meanwhile, has been polling `POST /devices/pair` with its
  code the whole time. Before confirmation that comes back `202 Pending`
  (expected — not an error, just "nobody's confirmed this yet"); once
  confirmed, it succeeds and returns a long-lived `access_token`, which
  the watch stores and sends as a normal `Authorization: Bearer` header on
  every request from then on — the exact same header every other client
  uses.

This is the opposite direction from the more obvious "already-logged-in
phone mints a code, watch types it in" design, and it's opposite on
purpose: that version needs the code typed into the watch (via Garmin
Connect Mobile's per-app settings, since the watch itself has no
keyboard), which is real friction and — with no camera on a watch — has no
path to a QR-scan shortcut at all. Flipping it so the watch originates the
code is what makes "show a QR code" possible in the first place, since
now it's the watch's own screen displaying something to be scanned, not
receiving something already decided elsewhere.

**Security trade-off vs. the phone-mints/watch-types design**: entropy and
exposure window are unaffected by which side generates the code — a
6-character code is equally guessable (or not) regardless of who picked
it, so that's not a reason to favor either direction. What *does* differ
is the failure mode if someone else's guess wins the race: in the
phone-mints direction, an attacker's device would end up bound to the
*victim's real account* (full impersonation); here, an attacker who
confirms first only binds the *victim's watch* to the *attacker's own*
account (the victim's real account is never touched — just misdirected
data, not compromised access). The new direction does introduce one thing
the old one didn't have: the code is now visible on a screen worn in
public (a watch), whereas the old code only ever appeared in a phone's own
settings UI. Net assessment: a modest improvement, not a free one — worth
rate-limiting `POST /devices/pair` regardless (see "What's left to build").

Under the hood (`agon_core::dao::device_pairing`), confirming a code
doesn't create some special "device session" concept — it reserves a
synthetic auth identity (`device_sub`, shaped like `device:<id>`) and
gives it its own `AUTH#<device_sub>` guard mapping to the confirming
user's internal id. That's the same guard shape every real login already
uses (`AUTH#<sub>`, see `agon_core::dao::keys::Pk::AuthGuard`) — a device
is just a second identity on the same account, exactly like linking a
second OAuth provider would be. `require_uid` and everything built on it
needed *zero* changes to support this.

The returned token is a real ES256 JWT, signed by a dedicated key
(`agon_service::auth::DeviceTokenSigner`, `AGON_DEVICE_JWT_PRIVATE_KEY` /
`AGON_DEVICE_JWKS` — see `local/README.md`'s "The device-signing key") —
not Supabase's, since the device never talks to Supabase at all. `JwtVerifier`
trusts that key exactly like it trusts Supabase's JWKS or the test key, so
verifying a device token needed no new code path either — see
`agon_service/src/auth.rs`.

**Formerly a v1 limitation, now fixed**: the minted token used to be a
full, undifferentiated account credential — `JwtClaims` carried no
scope/device marker, so a paired watch could call *any* endpoint the
account owner could. `JwtClaims` now carries `device_scope` (`None` for
every real login; `"live_scoring"` for a device token —
`DeviceTokenSigner::mint`), and `main.rs::check_scope` enforces it:
`require_uid` (the default nearly every handler already calls) rejects any
scoped token outright, and only the handful of endpoints a paired watch
actually needs (`GET /users/me`, `GET /matches`, `GET /matches/:id`,
`GET /matches/:id/live/seq`, `POST /matches/:id/live/events`,
`DELETE /matches/:id/live/events/:seq`) call the new
`require_uid_scoped(..., SCOPE_LIVE_SCORING)` instead, which additionally
accepts that one scope. Revocation exists too now — see "manage paired
devices" below. **Residual gap**: a handful of read-only endpoints
(`list_match_likes`/`list_match_comments`/`search_teams`/
`list_team_members`/`get_invitation`/`list_score_submissions`/
`get_asset`) call `AuthSchema` directly without going through
`require_uid`/`check_scope` at all — same "any authenticated token"
pattern they already had before scope existed, just now that also
includes a device token. (`list_live_events` used to be on this list
too — closed once the watch app's own undo feature needed it, see
below.) Lower stakes than the write paths (no per-caller permission
logic on any of them to begin with), but worth closing
properly rather than leaving implicit.

**Manage paired devices**: `dao::paired_device` (`PairedDeviceRecord`,
`USER#<uid>`/`PAIREDDEV#<device_sub>`) is written alongside the
`AUTH#<device_sub>` guard the moment a pairing is claimed.
`GET /devices/paired` lists them, `DELETE /devices/paired/:device_sub`
revokes one — deletes the record and the guard atomically, so an
already-minted token stops resolving immediately, no blocklist needed.
Both are full-access only (`require_uid`, not the scoped variant): a
device can't manage its own pairing, only the real logged-in account
owner can. `agon_ui`'s `PairedDevicesPage` (`/devices`, linked from the
profile page's Account section) is the UI for this.

## Architecture options for the watch app itself

Connect IQ (Garmin's SDK, Monkey C language) apps come in a few flavors —
Watch App, Widget, Data Field, Watch Face — and network access comes in a
few flavors too. The combination that matters here:

| Option | How it talks to Agon | Works while recording an activity? | Verdict |
|---|---|---|---|
| **A. Watch App, direct HTTPS** | `Communications.makeWebRequest` straight from the watch, over its own WiFi or LTE (cellular Garmins only) | Yes — a Watch App can run `ActivityRecording` and make web requests concurrently | **Chosen** |
| B. Watch App, phone-relayed | `Communications.transmit` to the paired Connect Mobile app over BLE, which forwards the HTTP call | Yes | Needed for watches with no WiFi/LTE; bigger lift (a mobile-side relay), deferred |
| C. Data Field (runs inside another app, e.g. Garmin's own Run activity) | Same web-request APIs, but confined to a small on-screen tile and someone else's activity lifecycle | Limited screen space for a football-scoring button grid | Rejected — not enough UI room for goal/card/sub/period controls |
| D. Pull-based: Garmin Connect's Activity/Health API webhooks | N/A — those APIs surface a *finished* activity upload, not an in-progress one | No live hook at all | Rejected — can't drive live scoring from a post-hoc upload |

**Chosen: A — a standalone Watch App**, so it owns the full screen for a
scoring UI (a button grid for goal/card/sub/period, a mini score header) and
can start its own `ActivityRecording.Session` in parallel, targeting
WiFi/LTE-capable devices for v1. Option B (phone relay for non-connected
watches) is the natural v2 once A is validated — it reuses the exact same
event queue and API calls, just swaps the transport.

### Recording + scoring, concurrently

The watch app's lifecycle:

1. An `ActivityRecording.Session` (sport `SOCCER`) records exactly like
   any workout app's — this is what makes it show up as a normal recorded
   activity in Garmin Connect afterwards. Kick-off starts it on every watch
   that has the match open at that moment, whichever device recorded the
   kick-off (a remote one is picked up by the next 5s score poll, with a
   buzz). A watch that opens a match already under way doesn't auto-start;
   its wearer uses the main menu's Start activity item. Until it's
   recording (and whenever it's paused), both match pages draw a thick red
   ring around the screen edge (`RecordingRing.mc`). The GPS is switched on
   separately, continuously, as soon as a match is opened
   (`ActivityRecorder.enableGps`) — a `Session` never turns it on by itself,
   and the first real-match recordings, made before that call existed,
   saved a distance far short of what was actually covered.
2. Show the scoring UI (score header + Goal/Card/Sub/Period buttons) as the
   foreground view for the rest of the match.
3. Each button tap appends one `FootballLiveEvent` to a local queue
   (`Application.Storage`, so it survives a crash/reboot) and immediately
   tries to flush the queue via `POST /matches/:id/live/events`, using the
   `expected_last_seq` it last saw for this match.
4. If the request fails (no signal, timeout), the event just stays queued —
   the next successful flush sends everything queued since the last
   accepted `seq`, one batch call, which is precisely what
   `AppendLiveEventsInput` was built for (see its doc comment in
   `agon_service/src/live_score/mod.rs`).
5. Pausing, resuming and finishing the activity recording are independent
   of scoring — a ref can keep the recording running through a match that's
   already been marked full-time on the score side, or vice versa. The menu
   item toggles Start/Pause/Resume; End match asks Save or Discard (Discard
   confirmed a second time) and then exits the app, posting nothing to the
   live score (`EndMatchFlow.mc`).

Recording and the HTTP calls don't compete for the same resource in any way
that needs special handling — `Position`/`ActivityRecording` own the GPS/HR
sensors (the GPS only once `Position.enableLocationEvents` turns it on, see
step 1), `Communications` owns the radio; Connect IQ runs both concurrently in
plenty of existing third-party apps (any app that both records a workout
and posts live updates to a service does exactly this).

### Match selection + auth on the device

Built as an in-watch picker fetching from the API directly, not the
companion-phone-settings approach floated earlier in this doc's first
draft (a settings screen with a hand-typed match id) — nicer for the
wearer, and the picker's two lookups (`GET /users/me` for the account's
own id, `GET /matches?participant=<id>&match_type=football&limit=10` for
the list) were already endpoints the API needed to expose for other
reasons. Selecting a sport ("Football") on launch goes to
`MatchPickerView` instead of straight to the score screen; picking a
match there fetches its full roster (`GET /matches/:id`) into
`MatchContext` (real `side_id`s and player ids — see below) before
finally handing off to the score screen. See "What the watch side does"
below for the exact flow.

## What's left to build

Backend: done — the device-originated/QR-first pairing endpoints
(`GET /devices/pairing-codes/:code/qr.png`,
`POST /devices/pairing-codes/:code/confirm`, and the updated
`POST /devices/pair`), device-signing infrastructure, unit tests
(`agon_service/src/auth.rs`, `agon_service/src/qr.rs`,
`agon_core::dao::device_pairing`). Not yet run through
`make generate-schema` (needs `openapi-generator-cli`, not available in
this sandbox) — run that once to regenerate `schema.json` and
`openapi_client` before wiring the `agon_ui` confirm page against the
generated client types.

Watch app: a real Connect IQ (Monkey C) project, `agon_garmin_app/`,
created via the SDK's own Project Wizard, with the pairing screen
compiler-verified on a real device (fr955 simulator) and everything since
(match picking, real live-scoring calls) checked against the official API
docs the same way but not yet run through the real `monkeybrains.jar` —
see the file headers for exactly which signatures were verified.
`MockApiClient`/`MockRoster` are gone, replaced by `LiveApiClient`/
`MatchApiClient`/`MatchContext` (real network calls, real `side_id`/player
ids) — pairing is no longer the only thing that talks to the real API. A
now-superseded hand-written scaffold (`garmin_app/`, written before a real
SDK install was available to compile against) was removed early on in
favor of this one.

What the watch side does: `agonApp.getInitialView()` checks
`DeviceAuth.getAccessToken()` (`Application.Storage`) and shows
`PairingView` instead of the sport picker whenever there's no token yet.
`PairingView` generates a code (`PairingCode` — same 6-char, 32-symbol
alphabet as the server), fetches its QR via
`Communications.makeImageRequest` (falling back to drawing the bare code
as text if that fails or is still loading), and polls
`POST /devices/pair` (`PairingApiClient`) every few seconds: `200` stores
the token (`agonApp.onPaired`) and switches to the sport picker; `202` is
the expected "nobody's confirmed it yet" steady state; `400` (confirmed
code already claimed/expired) or the client's own give-up timeout
(`CODE_LIFETIME_MS`, comfortably under the server's confirmed-code TTL)
regenerates a fresh code; `503` means pairing isn't configured on this
deployment. `PairingDelegate.onSelect` toggles a plain-text screen (the
pairing site + the code, large) over the QR — useful whenever the QR is
hard to scan, or to just read/type the code instead; `onMenu` forces a
manual regenerate (e.g. if the QR image failed to load), a secondary
action since it depends on a gesture not every device offers.

Once paired, picking "Football" on the sport menu goes to
`MatchPickerView`: it resolves the account's own id
(`MatchApiClient.fetchMyUserId`, `GET /users/me`), lists that account's
football matches (`fetchMatches`, `GET /matches?participant=...`) as a
plain-text loading/empty/error state that hands off to a real
`WatchUi.Menu` the moment matches actually load, and — once one's picked
— fetches its full roster (`fetchMatch`, `GET /matches/:id`) into
`MatchContext` before finally `switchToView`ing to the score screen.
`MatchContext` holds the two sides' real ids/names and each side's real
player roster (id + display name, resolved from the match's `Member`
union the same way `agon_ui` does), replacing the old `MockRoster`'s fake
fixed names — `GoalFlow.mc`'s side/scorer/assist menus now read from it
instead. `LiveApiClient` (replacing `MockApiClient`) is what
`FootballScore` actually posts to: `POST /matches/:id/live/events` for
goals and period markers, seeded with the real `expected_last_seq` from
`GET /matches/:id/live/seq` the moment a match is picked, and re-fetched
on a `409 Conflict`, which then retries the conflicting event itself
once with the corrected seq (see `onSeqForRetry`) rather than dropping
it — this prototype still has no real offline queue (a second event
recorded while the first's retry is in flight overwrites it — see
"what's left to build"), and every `Communications.makeWebRequest` call
in this class is funneled through a small request queue
(`enqueueRequest`/`pumpQueue`) rather than fired directly, after
concurrent score-poll/append/conflict-recovery requests were found to
silently clobber each other's callbacks on a real device. `FootballScore`'s own call shape into the api
client is unchanged from `MockApiClient`'s, per that class's original
design; only what the client does with it changed. `refreshSeq`
(alongside `refreshScore`, on the same poll timer in `agonView`) keeps
`_lastSeq` current proactively too, not just reactively after a
conflict — another device's undo bumps the counter exactly the same way
an append does (see `Dao::delete_live_event`'s doc comment on the
backend), and this device has no other way to learn about it before
its own next attempt.

**A second, deeper bug behind all of the above**: even with every fix
described so far, a real conflict *still* never actually reached
`onAppendResponse` as `409` — it came back as `Communications.
NETWORK_RESPONSE_TOO_LARGE`'s sibling, `-400`/
`INVALID_HTTP_BODY_IN_NETWORK_RESPONSE`, confirmed from a real device's
`System.println` trace. Root cause: `Communications.makeWebRequest`
checks the response's actual `Content-Type` header against the
`:responseType` the caller requested, and refuses to deliver the real
HTTP status at all when they don't match — every error response on
`POST /matches/:id/live/events` (and the other endpoints a paired
device calls) was `PlainText` (`text/plain`) while the device requests
JSON, for every non-2xx outcome including `409 Conflict`. Fixed by
introducing `ErrorMessage` (a one-field `Json` wrapper) and switching
every error variant on the endpoints a device can reach
(`GetUserResponse`, `ListMatchesResponse`, `GetMatchResponse`,
`GetLiveSeqResponse`, `AppendLiveEventsResponse`,
`DeleteLiveEventResponse`, `GetMatchScoreResponse`, and — added
alongside undo below — `ListLiveEventsResponse`) from
`PlainText<String>` to it — see `ErrorMessage`'s own doc comment on
`agon_service::main`. **Not fixed**: `check_scope`'s own rejection (and
any other generic `poem::Error` a handler propagates via `?` rather
than a named response variant) still renders as `text/plain` by Poem's
own default, so an expired/invalid/wrong-scope token would still hit
this same masking on any of these endpoints; and every *other*
`PlainText` response elsewhere in the API has the identical latent bug,
just not yet reachable by any current device client.

**Undo** (`LiveApiClient.undoLast`, `DELETE /matches/:id/live/events/
:seq`) needed its own sequence-counter care, separate from
`_lastSeq`/`expected_last_seq`: an append never creates a gap, so
`_lastSeq` doubles as the log's real physical tip right after one, but
an undo bumps the counter *past* the deleted event (see
`Dao::delete_live_event`'s doc comment on the backend) — so `_lastSeq`
and the physical tip permanently disagree from that point until the
next append re-aligns them. `agon_ui`'s own `useUndoTargetSeq` hook
documents this trap in detail and was the reference for getting it
right here too: a separate `_undoTargetSeq`, seeded (`refreshUndoTargetSeq`)
by draining `GET /matches/:id/live/events` and taking the highest `seq`
actually present — never by trusting `/live/seq`'s raw counter, which
counts deletes too. Re-derived whenever this device (re-)learns
`/live/seq` from scratch (`setMatch`, or a conflict retry) rather than
assumed to still agree, and again after this device's own undo, since
that same gap-creating bump applies to it too. Exposed as an "Undo
last" item on the main menu, shown only once `canUndo()` is true (a
real seq is known) — matching `agon_ui`'s own `UndoLastEventButton`
hiding outright rather than disabling. Two things `agon_ui` does that
this doesn't: a confirmation dialog before undoing (deliberately
skipped — a menu item is already two button presses of friction), and
retrying against a corrected seq on failure (deliberately never done —
unlike an append conflict, where retrying resends the exact same
intended write, retrying a delete against a since-moved tip would
delete a *different*, unintended event; a failed undo just re-syncs
state for next time instead).

`API_BASE_URL` (`ApiConfig.mc`, shared by `PairingApiClient`/
`MatchApiClient`/`LiveApiClient` — file-scope, not a per-class constant,
so there's exactly one place to change it) is a hardcoded constant
(`https://agon.staging.get-agon.com/api` — the `/api` matters: that's
`agon-api-ingress` publishing `agon_service`'s own `/`-mounted routes, same
convention `agon_ui`'s dev proxy and `make test-staging` both use, not a
path `agon_service` itself knows about) rather than a real App Setting
(`resources/settings/*.xml`) — hand-writing that resource XML with no
compiler available in this sandbox to check it against wasn't worth the
risk for a prototype. For local dev instead, point it at
`http://localhost:7000` (no `/api` — a local `agon_service` has no ingress
in front of it) and run `make run`; the simulator proxies `Communications`
calls through the desktop it runs on, so `localhost` reaches it directly.
Make it configurable (a real App Setting, editable from the Garmin Connect
Mobile companion settings screen — see below) once switching between the
two is more than a one-line edit.

Pointing at staging gets the QR image and `confirm` working, but the final
`POST /devices/pair` claim will `503 NotConfigured` there until item 9
below (provisioning `AGON_DEVICE_JWT_PRIVATE_KEY`/`AGON_DEVICE_JWKS` on the
staging deployment) is actually done — `agon_infra/index.ts` doesn't wire
either into the service yet, so `DeviceTokenSigner::from_env()` comes back
`None` on staging today.

Still to do, roughly in order:

1. **Build the watch app in the actual Connect IQ simulator** and fix
   whatever the real compiler flags — every API used (pairing, match
   picking, live scoring alike) was checked against the official docs
   while writing this, the same discipline that caught the `Menu`-vs-
   `Menu2`/`SPORT_GENERIC` mistakes early on and the class-`const`-in-a-
   `static function` one later (see `DeviceAuth.mc`'s doc comment) — but
   the match-picker/live-scoring code specifically hasn't been run
   through `monkeybrains.jar` yet (pairing has, on a real fr955).
2. ~~Wire `MockApiClient` up to the real API~~ — done: `LiveApiClient`
   posts real `POST /matches/:id/live/events` calls (goals and period
   markers), seeded from `GET /matches/:id/live/seq`. A `409 Conflict`
   (another writer moved the log on) re-syncs the seq and retries the
   same event once (`onSeqForRetry`) rather than dropping it outright —
   confirmed necessary from a real device: without the retry, the score
   visibly flicked up (the local optimistic tally) then immediately back
   down (the server's score, still missing the goal, arriving via the
   poll timer) every time a device recorded a goal while out of sync.
   Still no real offline queue — a second event recorded while the first
   one's retry is in flight overwrites it — worth building once this
   sees real flaky-connectivity use.
3. ~~The `agon_ui` confirm page~~ — done: `PairDevicePage` (`/pair`,
   reading `?code=` or offering manual entry) calls
   `POST /devices/pairing-codes/:code/confirm` and reuses the existing
   "log in, then return to this deep link" mechanism (`pendingInvite.ts`,
   extended with a `pair` kind alongside the existing `invite`/`join`
   ones — same idea as the match-join-link flow, just a query-string route
   instead of a path param since that's the URL shape the watch's QR code
   and fallback text actually show). `npx tsc --noEmit`, eslint, and a full
   `npm run build` all pass; not yet exercised against a real confirm
   (needs the watch side actually running, or a manual `curl`, to produce
   a real code to confirm).
4. ~~Fetch and render the real score~~ — done: `LiveApiClient.refreshScore`
   polls `GET /matches/:id/score` and applies it onto `FootballScore`
   (`applyServerState`) — otherwise the local tally only ever reflected
   *this* device's own `recordGoal`/`setPeriod` calls, so another device
   scoring the same match had no way to reach the screen. Called once
   when a match is picked (`setMatch`), on every 5s while `agonView` is
   visible (a `Timer`, same pattern as `PairingView`'s poll loop), and
   once more on an append `409 Conflict` (the one case this device
   already knows something changed elsewhere). Still local-tally-first in
   one sense: a fetch failure or an in-flight period between polls just
   leaves the last-known value on screen rather than resetting to 0-0.
5. ~~Device-scoped tokens~~ — done: `device_scope`/`check_scope` (see
   above). The residual gap flagged there (a handful of read-only
   endpoints not checking scope at all) is real follow-up, not a full
   close-out.
6. ~~A "manage paired devices" screen~~ — done: `GET`/`DELETE
   /devices/paired/:device_sub` plus `agon_ui`'s `PairedDevicesPage` (see
   above).
7. **Cards and substitutions** on the watch, once there's a player picker
   for them — `MatchContext`'s roster (added for goals) already has the
   ids these would need too.
8. **Phone-relay transport** (Option B above) for watches without direct
   WiFi/LTE.
9. ~~Provision the device-signing key via `agon_infra`~~ — done, but as a
   config-based keypair (`agonDeviceJwtPrivateKey`/`agonDeviceJwks`,
   `config.requireSecret`/`config.get`), mirroring the existing
   `agonTestJwtPrivateKey`/`agonStaticJwks` test-signing-key pattern rather
   than the CloudFront one: unlike CloudFront's signed URLs, nothing here
   needs an external cloud resource registered against the public key, just
   `agon_service` itself trusting/minting with it — the same shape as the
   test key's problem, not the CloudFront one's. Generated with the same
   `openssl ecparam`/`pkcs8` recipe as `local/agon-device-key.pem`; the
   private half was handed to @jamesnelgar directly (never printed to any
   transcript) rather than committed anywhere, since — unlike the test
   key — this one actually needs to stay secret. Still needs
   `pulumi config set --secret agonDeviceJwtPrivateKey ...` and
   `pulumi config set agonDeviceJwks ...` run against the staging stack,
   then a redeploy, before it takes effect there.
10. ~~An activity stats screen~~ — done: `ActivityStatsView` (the second
    match page, paged to with up/down from the score screen like a native
    activity's data screens — it replaced an "Activity stats" menu item,
    and polls the score every 5s the same as the score screen), showing the score,
    current-half time (`ActivityRecorder.currentHalfTimerTimeMs`, marked
    at each kick-off — separate from the whole match's own `timerTime`,
    shown smaller underneath it), distance, and heart rate, via
    `Activity.getActivityInfo()`. No calories (dropped — not useful
    enough to earn a line on a small screen already showing five things).
    No prebuilt widget for this exists to reuse — `WatchUi.
    SimpleDataField`/`DataField` are locked to the separate `datafield`
    app type and aren't usable from a `watchApp`-type project like this
    one — so it's hand-drawn the same way the score screen is, at the
    once-a-second cadence a real data field's own `compute()` runs at.
    Redrawn as a boxed 2x2 grid (half time/total time/distance/HR, each
    its own field with divider lines) under a small score/period header,
    the same gridded-fields look a stock Garmin running/multisport
    activity's own data screens use, rather than the original single
    column of five centered lines — the quarter-screen box each field
    gets now can afford a much bigger value than that could. Time fields
    use `Graphics.FONT_NUMBER_MILD` (a number font — only has digit/colon
    glyphs); distance/HR keep a regular text font since their values
    carry a unit suffix a number font can't render.
    Real `Session.addLap()` boundaries at half-time/second-half
    kick-off/full-time (`ActivityRecorder.markLap`) can't drive this live
    clock either, for the same underlying reason: checked `Activity.
    Info`'s full field list and there's no live "current lap time"
    exposed at all — a lap split only ever shows up later, as the saved
    activity's own per-half pace/HR/distance breakdown once viewed in
    Garmin Connect. Real value, just a separate one from the live clock,
    which keeps its own baseline instead (see `markHalfStart`'s doc
    comment) — so both exist, doing two different jobs.
11. **Rate-limit the pairing/confirm/qr endpoints** — flagged during
    design (see the security-comparison discussion): none of them have any
    throttling today, which matters most for `POST /devices/pair` (an
    unauthenticated, guessable-code-shaped surface).
12. **Make `apiBaseUrl` a real App Setting** instead of
    `PairingApiClient`'s hardcoded constant, once there's an actual
    deployed URL worth pointing a real watch at.
13. **Laps and `ActivityStatsView`'s own half clock only follow this
    watch's own period taps.** `markLap`/`markHalfStart` run from the main
    menu's period items, not from `FootballScore.applyServerState`, so a
    watch that's recording while *another* device records half-time/
    second-half kick-off gets no lap splits, and `ActivityStatsView`'s
    current-half reading (still `ActivityRecorder.currentHalfTimerTimeMs`,
    a device-local clock) never resets (kick-off itself is handled — see
    `ActivityRecorder.startForKickOff`). More visible now that recording
    and scoring are separate: opening a match someone else is scoring is
    a supported flow. The score screen's own current-half clock doesn't
    have this problem anymore — see item 14 — but the FIT lap splits and
    `ActivityStatsView`'s reading still do. Fix: mark laps on any observed
    period transition, taking care that undo's period rollback doesn't add
    a spurious one; `ActivityStatsView` could also just switch onto the
    same server-sourced value item 14 added, rather than fixing its own
    local one.
14. ~~A current-half clock that doesn't depend on this device's own
    recording~~ — done, on the score screen only:
    `FootballScore.currentHalfStartedAt` (`LiveApiClient.
    currentHalfStartMoment`, parsed from the server's own `Score.
    period_times` — the same timestamps `period_times` in the backend's
    `FootballScore` struct already carried, just not read by the watch
    before now) drives `agonView`'s current-half clock, ticking once a
    second (`agonView.POLL_INTERVAL_MS`, split from the 5s server-poll
    cadence the same way `ActivityStatsView` already does) independent of
    `ActivityRecorder`'s local half-tracking — correct even for a watch
    that opened a match someone else is scoring, or isn't recording an
    activity at all. `ActivityStatsView` still shows its own, separately
    derived current-half reading — see item 13's now-narrowed scope.
