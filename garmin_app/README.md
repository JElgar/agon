# Agon Live Scoring — Garmin Connect IQ app

A Garmin watch app that scores a football match live against the Agon API
while the wearer's own activity keeps recording normally alongside it. See
`docs/garmin-live-scoring.md` (repo root) for the full design and the
options considered; this directory is the "watch app" piece from that
doc's "What's left to build" list.

**Status: hand-written scaffolding, not yet compiled or run.** This
sandbox has no Connect IQ SDK (`monkeyc`), so nothing here has gone through
the real compiler or simulator — the API calls are written against
`agon_service`'s actual endpoints/JSON shapes (checked directly against
the Rust source, not guessed), but the Monkey C language/Toybox API
specifics have not been compiler-verified. **Before relying on this**, open
it with a real Connect IQ SDK install and expect to fix it up — see
"Getting it building" below for exactly what that means.

## What it does (v1 scope)

- Pairs to an Agon account via a one-time code (see the backend's
  `POST /devices/pairing-codes` / `POST /devices/pair`), entered through
  the app's Garmin Connect Mobile settings screen — no typing on the watch
  itself.
- Starts a normal `ActivityRecording` session (GPS/HR) alongside the
  scoring UI, so the wearer's own activity records exactly like any other
  Garmin workout, independent of scoring.
- Lets the wearer log **goals** (home/away) and **period markers**
  (kick-off, half-time, full-time, extra time, penalties) via an on-watch
  menu, queuing them locally and flushing to
  `POST /matches/:id/live/events` in batches — the same
  `expected_last_seq`-guarded batch endpoint an offline-then-reconnected
  client is meant to use (see `agon_service/src/live_score/mod.rs`'s doc
  comment), so a spell with no signal just queues up and flushes later.

**Deliberately not in v1**: cards and substitutions. Both need a specific
player id (`FootballCardEvent`/`FootballSubstitutionEvent` require
`player_id` — see `agon_service/src/detailed_score/football.rs`), which
means fetching the match roster and picking a name on a five-button watch
— a real feature, not a quick add. Goals also go in side-unattributed
(`scorer_player_id: null`) for the same reason. This is the natural next
increment once the goal/period flow is proven out.

## Project layout

```
manifest.xml              App id, permissions, supported devices.
monkey.jungle             Build config (source + resource paths).
resources/
  strings/strings.xml     User-visible text.
  settings/settings.xml   App Settings schema (pairing code, match id, ...).
  settings/properties.xml Default values for the above.
source/
  AgonLiveScoringApp.mc   Application entry point + settings-changed handler.
  AgonApiClient.mc        HTTP calls: pair, append live events, get live seq.
  EventQueue.mc           Offline-safe local queue + expected_last_seq tracking.
  FootballEvents.mc       Builds the JSON dictionaries the API expects.
  ActivityRecorder.mc     ActivityRecording.Session lifecycle wrapper.
  ScoringView.mc          The one screen: pairing prompt / missing-config
                          prompt / score + sync status, and the periodic
                          flush loop while it's visible.
  ScoringDelegate.mc      Input handling — opens the scoring + period menus.
```

## Configuration (App Settings)

Set via Garmin Connect Mobile → the app's own settings screen, once
installed on a paired watch (see `resources/settings/settings.xml`):

- **Pairing code** — the 6-character code from Agon's "pair a device"
  screen (`POST /devices/pairing-codes`). Entering it triggers
  `AgonApiClient.pairDevice` automatically; once it succeeds the stored
  access token replaces the need to re-enter this.
- **API base URL** — e.g. `https://api.get-agon.com` in prod, or a local
  tunnel URL for dev (`agon_service` on `localhost:7000` isn't reachable
  from a real watch — only from the simulator on the same machine).
- **Match id**, **home side id**, **away side id** — which match to score
  and which `Match.sides[].id` each goal button counts for. There's no
  on-watch match/roster browser yet (see "Deliberately not in v1" above,
  same reasoning) — these come from the match's page in `agon_ui` /
  `agon_service`'s API for now.

## Getting it building

1. Install the Connect IQ SDK Manager and SDK
   (https://developer.garmin.com/connect-iq/sdk/), plus a device simulator.
2. Register an app id in the Garmin Connect IQ developer portal and put it
   in `manifest.xml`'s `iq:application id="..."` (currently a placeholder).
3. Add a real launcher icon PNG under `resources/drawables/` and wire it
   into `manifest.xml`'s `launcherIcon` — omitted here since this sandbox
   can't produce a binary asset; the SDK's Project Wizard generates a
   placeholder one if you'd rather regenerate the resource scaffolding
   from scratch and copy the `source/` files in.
4. `monkeyc -f monkey.jungle -o bin/agon_live_scoring.prg -y <your_developer_key.der>`
   (or open the project in VS Code with the Monkey C extension and build
   from there) and fix whatever the compiler flags — expect some: this was
   written from API knowledge, not a live compiler in this sandbox.
5. Run in the Connect IQ simulator first
   (`connectiq` / the VS Code extension's "Run" command), then sideload to
   a real WiFi-capable device (Developer Mode → drop the `.prg` in
   `GARMIN/APPS/`) for a real network + `ActivityRecording` test.

## Known gaps beyond "cards/substitutions" (tracked, not fixed here)

- The on-watch score (`ScoringView`'s big number) is a session-local tally
  of goals tapped on *this* device, incremented client-side the moment
  you tap — not fetched from `GET /matches/:id/score`. It'll disagree
  with reality the moment another device/scorer is also recording this
  match, or after an app restart. Fetching and rendering the real
  server-derived score is a natural next step, just not built here.
- No retry/backoff tuning on `Communications.makeWebRequest` failures
  beyond "leave it queued, flush on the next tick" — fine for a prototype,
  worth a proper backoff before real use.
- No UI for re-pairing if the stored access token is ever revoked
  server-side (there's no revoke endpoint yet either — see
  `docs/garmin-live-scoring.md`'s limitations section) — the app would
  just start getting 401s with no on-watch explanation. A "pairing
  expired, re-enter a code" state is a natural follow-up once revocation
  exists.
- `apiBaseUrl` must be a real internet-reachable HTTPS URL for a physical
  device (Connect IQ's `Communications.makeWebRequest` goes out over the
  watch's own WiFi/LTE or the phone's connection, never `localhost`) — the
  simulator is the only place a local `agon_service` is reachable at all,
  and even then only because the simulator runs on the same machine.
