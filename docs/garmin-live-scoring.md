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
3. **Pairing code, minted by an already-logged-in client and claimed once
   by the device.** This is what phones do to link a TV app, a smart
   speaker, a CLI tool, etc. — chosen approach. It never exposes the user's
   real credentials to the device, and the thing being typed is short-lived
   and single-use rather than a durable secret.

### What's built: `POST /devices/pairing-codes` + `POST /devices/pair`

- `POST /devices/pairing-codes` (normal bearer auth, called from the
  already-logged-in web/mobile app) mints a 6-character, human-typeable code
  (`agon_service::new_pairing_code`, a 32-symbol alphabet with visually
  ambiguous characters dropped) valid for 10 minutes, tied to the caller.
- `POST /devices/pair` (**no** bearer token — the device has none yet) takes
  that code and, on success, returns a long-lived `access_token` the watch
  stores and sends as a normal `Authorization: Bearer` header on every
  request from then on — the exact same header every other client uses.

Under the hood (`agon_core::dao::device_pairing`), claiming a code doesn't
create some special "device session" concept — it reserves a synthetic auth
identity (`device_sub`, shaped like `device:<id>`) and gives it its own
`AUTH#<device_sub>` guard mapping to the same internal user id the code's
owner already has. That's the same guard shape every real login already
uses (`AUTH#<sub>`, see `agon_core::dao::keys::Pk::AuthGuard`) — a device is
just a second identity on the same account, exactly like linking a second
OAuth provider would be. `require_uid` and everything built on it needed
*zero* changes to support this.

The returned token is a real ES256 JWT, signed by a dedicated key
(`agon_service::auth::DeviceTokenSigner`, `AGON_DEVICE_JWT_PRIVATE_KEY` /
`AGON_DEVICE_JWKS` — see `local/README.md`'s "The device-signing key") —
not Supabase's, since the device never talks to Supabase at all. `JwtVerifier`
trusts that key exactly like it trusts Supabase's JWKS or the test key, so
verifying a device token needed no new code path either — see
`agon_service/src/auth.rs`.

**Known v1 limitation, deliberately not fixed yet**: the minted token is a
full, undifferentiated account credential. `JwtClaims` carries no
scope/device marker, so a paired watch can call *any* endpoint the account
owner can, not just live-scoring ones — and there's no revocation endpoint
(cutting one off means deleting its `AUTH#<device_sub>` guard item by hand).
Properly scoping this — a `scope` claim plus a check in sensitive handlers,
plus a "manage paired devices" UI — is real follow-up work, not something
to gloss over. It's an acceptable prototype cut because the actual
capability being delegated (append live-scoring events for matches the
account can already score) is low-stakes compared to, say, payment or
account-deletion endpoints — but it should be tightened before this ships
to real users' watches.

Also not done yet: provisioning the device-signing key via `agon_infra`
(Pulumi) the way `agon_infra/index.ts` already does for the CloudFront
signing key (`assetSigningKey`) — right now it's an env var you set by
hand, fine for a prototype, not fine for a real deployment.

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

1. On start, open an `ActivityRecording.Session` (sport `SOCCER` if the SDK
   exposes it for the device, else `GENERIC`) exactly like any workout app
   — this is what makes the session show up as a normal recorded activity
   in Garmin Connect afterwards, independent of anything scoring-related.
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
5. Stopping the activity recording (end of match) is independent of
   scoring — a ref can keep the recording running through a match that's
   already been marked full-time on the score side, or vice versa.

Recording and the HTTP calls don't compete for the same resource in any way
that needs special handling — `ActivityRecording` owns the GPS/HR sensors,
`Communications` owns the radio; Connect IQ runs both concurrently in
plenty of existing third-party apps (any app that both records a workout
and posts live updates to a service does exactly this).

### Match selection + auth on the device

Two small pieces of setup state the watch needs, beyond the paired
`access_token`: which match it's scoring. For a v1 prototype, both are
simplest to enter the same way pairing itself does — through the Connect IQ
app's **companion phone settings** (Garmin Connect Mobile lets a paired
Connect IQ app expose a settings screen with text fields, edited from the
phone's own keyboard rather than the watch's buttons): one field for the
pairing code, one for the match id. A later version could instead have the
watch fetch "matches I can score" from the API itself and pick from a list
— nicer, but not needed to prove the concept.

## What's left to build

Backend: done — pairing endpoints, device-signing infrastructure, unit
tests (`agon_service/src/auth.rs`, `agon_core::dao::device_pairing`). Not
yet run through `make generate-schema` (needs `openapi-generator-cli`, not
available in this sandbox) — run that once to regenerate `schema.json` and
`openapi_client` before wiring a UI "pair a device" screen into `agon_ui`
against the generated client types.

Watch app: a real Connect IQ (Monkey C) project, `agon_garmin_app/`,
created via the SDK's own Project Wizard and building/running in the
simulator. Currently covers goal + period-marker scoring (via an on-watch
menu) and a concurrent `ActivityRecording.Session`, both compiler-verified
— no pairing, no real network calls yet: `MockApiClient` just logs what
would be sent instead of calling `agon_service`. A now-superseded
hand-written scaffold (`garmin_app/`, written before a real SDK install
was available to compile against) has been removed in favor of this one.

Still to do, roughly in order:

1. **Wire `MockApiClient` up to the real API** — pairing (via App
   Settings, claiming a code from `POST /devices/pairing-codes`) and an
   offline-safe queue against `POST /matches/:id/live/events`, replacing
   the mock with real `Communications.makeWebRequest` calls. `FootballScore`
   and the UI shouldn't need to change for this — see `MockApiClient`'s
   doc comment.
2. **A "pair a device" screen in `agon_ui`** calling
   `POST /devices/pairing-codes` and showing the code + a countdown — the
   only way to actually get a code today is calling the endpoint directly.
3. **Fetch and render the real score** (`GET /matches/:id/score`) on the
   watch instead of `FootballScore`'s session-local tally.
4. **Device-scoped tokens** (the v1 limitation flagged above) — a `scope`
   claim plus enforcement in sensitive handlers, before this goes anywhere
   near a non-trivial number of real users' watches.
5. **A "manage paired devices" screen** — list + revoke, once there's
   more than one code path creating `AUTH#device:*` guards to manage.
6. **Cards and substitutions** on the watch, once there's a roster
   fetch + player picker to attribute them to.
7. **Phone-relay transport** (Option B above) for watches without direct
   WiFi/LTE.
8. **Provision the device-signing key via `agon_infra`** instead of a
   hand-set env var, mirroring the existing CloudFront signing-key pattern.
