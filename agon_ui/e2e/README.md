# UI end-to-end tests

Full browser tests: [Playwright](https://playwright.dev) drives the real UI
against a real running Agon environment — real Supabase login, real API
calls, real DynamoDB/Meilisearch on the far end. This is deliberately
different from `agon_tests`, which talks to the API directly via the
generated OpenAPI client and never touches a browser or Supabase at all (see
`agon_tests/tests/api.rs`'s doc comment) — that suite is faster and covers
far more of the API surface, but can't catch a UI bug (wrong endpoint called,
a broken form, a gate that doesn't unlock). This suite exists for the things
only a real browser session can catch: login → profile gate → the actual
click-through flows a player would use.

## How auth works here

`LoginForm` already offers a real email/password path
(`supabase.auth.signInWithPassword`) alongside Google OAuth — Google isn't
the only way in, just the primary one for real users. OAuth needs a real
Google account and an interactive consent screen a headless browser can't
complete, so these tests drive the email/password path instead, against a
**fixed test account that actually exists in the target Supabase project**.
That's a deliberate choice over the alternatives:

- **Not** a locally-minted JWT (what `agon_tests` does via a static JWKS the
  service trusts — see `AGON_TEST_JWT_PRIVATE_KEY` in the root
  `.env.example`). That bypasses Supabase entirely, which means it also
  bypasses the browser-side auth code this suite is here to exercise
  (`useAuth`, the profile gate, `supabase.auth.getSession()` wiring in
  `lib/api-client.ts`). Fine for testing the API; not for testing the UI.
- **Not** injecting a fabricated Supabase session into `localStorage`. That
  would skip `LoginForm` (and the profile-gate screen on a fresh account)
  altogether — the exact surface this suite exists to cover.

A single Playwright **setup project** (`tests/auth.setup.ts`) logs in once
via the real form and saves the resulting storage state (Supabase persists
its session in `localStorage`) to `.auth/user.json`; every other test reuses
it, so the suite doesn't repeat a real Supabase password grant per test.

### Provisioning the test user

Create the account once, in whichever Supabase project the target
environment's `VITE_SUPABASE_URL` points at (staging, typically) — either
Supabase dashboard → Authentication → Users → **Add user**, with email
confirmed, or the [Admin API](https://supabase.com/docs/reference/api/introduction)
directly:

```bash
curl -X POST "https://gkebmzhvdbsktamfdsil.supabase.co/auth/v1/admin/users" \
  -H "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
  -H "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "email": "agon-e2e-bot@example.com",
    "password": "<a real password>",
    "email_confirm": true
  }'
```

`email_confirm: true` is what makes the account usable immediately, without
clicking a confirmation link `signUp` would otherwise send. The URL above is
staging's project (`supabaseUrl` in `Pulumi.staging.yaml`) — swap it for
whichever project the target environment actually uses.

`SUPABASE_SERVICE_ROLE_KEY` is the project's **service role key** (Supabase
dashboard → Project Settings → API), not the anon key — it bypasses every
RLS policy and has full admin auth access. Export it in your shell just for
this one call; **don't** put it in Pulumi config, a `.env` file, or anywhere
else persistent — nothing in this codebase needs it on an ongoing basis,
only this one-off account bootstrap, so there's no case for storing it
long-term.

Pick a password, then store both in **Pulumi config** on the target stack —
not a GitHub secret, and not committed anywhere. This is the same pattern
`agonTestJwtPrivateKey` already uses (see `agon_infra/index.ts`): one
encrypted source of truth that CI reads live via `pulumi config get`
(`.github/workflows/test-ui-e2e.yml`), rather than duplicating the value into
a second secret store that can drift out of sync:

```bash
cd agon_infra
pulumi stack select staging
pulumi config set e2eTestEmail agon-e2e-bot@example.com
pulumi config set --secret e2eTestPassword '<a real password>'
```

`e2eTestEmail`/`e2eTestPassword` are `config.require`/`requireSecret`d in
`index.ts` (exported, like `agonTestJwtPrivateKey`, purely so CI can fetch
them — nothing deploys them anywhere), which means **`pulumi up`/`preview`
on a stack fails until it has both set**. Set them on staging before the
next deploy after pulling in this change.

The very first login creates the account's Agon profile too (via
`CreateProfileForm`, driven automatically by `auth.setup.ts`) — nothing else
to provision by hand.

## Running

Locally, the suite just reads plain env vars — Pulumi is only wired into the
CI job (`test-ui-e2e.yml`), not into `npm run test:e2e` itself:

```bash
cd agon_ui
npm install
npx playwright install --with-deps chromium   # once, to fetch the browser
E2E_TEST_EMAIL=... E2E_TEST_PASSWORD=... npm run test:e2e
```

Pull the values straight from Pulumi instead of retyping them:

```bash
E2E_TEST_EMAIL="$(cd ../agon_infra && pulumi config get e2eTestEmail --stack staging)" \
E2E_TEST_PASSWORD="$(cd ../agon_infra && pulumi config get e2eTestPassword --stack staging)" \
npm run test:e2e
```

By default this starts the local Vite dev server (`npm run dev`), which
already proxies `/api` to `https://agon.staging.get-agon.com` and points at
staging Supabase via `.env` (see `vite.config.ts`) — so against staging,
that's all you need. To test an already-deployed UI instead of spawning a
local server, set `E2E_BASE_URL` (e.g. a preview URL):

```bash
E2E_BASE_URL=https://agon.staging.get-agon.com \
E2E_TEST_EMAIL=... E2E_TEST_PASSWORD=... \
npm run test:e2e
```

`npm run test:e2e:ui` opens Playwright's UI mode for interactively stepping
through a run.

## Running fully local

Everything above targets staging: staging Supabase for auth, staging
`agon_service` for the API. To run the whole thing — UI, auth, API, worker —
against your own machine instead:

1. **Bring up the backing services** — Meilisearch, a local Supabase Auth
   (Postgres + GoTrue + Kong), a local Temporal, and DynamoDB Local + SQS
   (ElasticMQ), via the `full` docker-compose profile (see `local/README.md`
   for what's actually in it):

   ```bash
   docker compose --profile full up -d
   ```

2. **Seed the test account** against the local Supabase Auth stack (the local
   counterpart to "Provisioning the test user" above — no Pulumi, no real
   Supabase project involved):

   ```bash
   node agon_ui/e2e/local-seed-user.mjs
   ```

3. **Run `agon_service`** pointed at the local stack instead of staging — add
   to the root `.env` (see `.env.example`, and `local/README.md` for the
   DynamoDB/SQS pieces specifically):

   ```
   SUPABASE_JWKS_URL=http://localhost:8000/auth/v1/.well-known/jwks.json
   AWS_ACCESS_KEY_ID=local
   AWS_SECRET_ACCESS_KEY=local
   AWS_ENDPOINT_URL=http://localhost:8002
   ```

   then `make run` as usual.

4. **Run `agon_worker`** too — `tests/match-feed.spec.ts` depends on it: a
   logged match only shows up in the feed after the fan-out workflow
   (`agon_worker/src/temporal`) runs, which only starts once agon_worker
   actually receives the DynamoDB change event over SQS. That needs three
   things uncommented in `.env` (all in `.env.example`'s `agon_worker`
   section): `TEMPORAL_ADDRESS`, the two queue URLs, and
   `AWS_ENDPOINT_URL_SQS` — then:

   ```bash
   make run-worker
   ```

   (Needs `protobuf-compiler` installed — `agon_worker`'s Temporal
   dependencies need `protoc` to build, unlike `agon_service`.)

5. **Run the local stream bridge** — DynamoDB Streams are enabled on the
   local table, but nothing forwards those records to SQS by itself (that's
   an EventBridge Pipe in production, with no compose equivalent — see
   `local/README.md`'s `local/dynamodb-stream-bridge/` section). Without
   this running, step 4's `agon_worker` starts fine but never actually
   receives anything, so a logged match never reaches the feed:

   ```bash
   make run-stream-bridge
   ```

6. **Point the UI at the local stack** — add to `agon_ui/.env` (see
   `agon_ui/.env.example`):

   ```
   VITE_SUPABASE_URL=http://localhost:8000
   VITE_SUPABASE_ANON_KEY=local-dev-anon-key
   AGON_API_PROXY_TARGET=http://localhost:7000
   ```

7. **Run the tests**, same as always — the local Supabase Auth stack has no
   Pulumi-stored secret, so the test account's email/password are whatever
   you passed `local-seed-user.mjs` (or its defaults, `e2e@example.com` /
   `local-e2e-test-password`, if you ran it with none):

   ```bash
   E2E_TEST_EMAIL=e2e@example.com E2E_TEST_PASSWORD=local-e2e-test-password \
   npm run test:e2e
   ```

`docker compose --profile full up -d` (no flags at all) only starts
Meilisearch, same as always — the `full` profile is additional weight (two
Postgres instances, GoTrue, Kong, Temporal, DynamoDB Local, ElasticMQ) most
day-to-day work doesn't need, so it's opt-in.

`make test-ui-e2e-local` wraps steps 2 and 7 (seed + run, with matching
defaults) — steps 1, 3–6 (the backing services and the four app processes:
agon_service, agon_worker, the stream bridge, the UI dev server) still need
to be up first.

## What's covered

- `tests/match-feed.spec.ts` — logging a match through the real "Log a
  match" form and confirming it shows up in the feed and opens correctly.
- `tests/football-live-scoring.spec.ts` — a full football live-scoring pass:
  recording a goal with an assist, undoing it, half-time (and that scoring
  is unavailable while the clock is stopped), resuming the second half, a
  second goal, full-time, and finishing the match.

Test data isn't cleaned up afterwards — created matches accumulate against
the test account like any other match would, named `E2E football …` / `E2E
Opposition …` etc. so they're easy to spot.

## Notes for adding more tests

- Locator priority, in order: `getByRole` / `getByLabel` / `getByPlaceholder`
  first — most of the app's markup is accessible enough (labelled inputs,
  buttons with real text) that these work throughout and stay meaningful if
  the visual design changes. Where an element genuinely has no accessible
  name (e.g. the bare live-score digits, a tagged player's name — which is
  just text, not a control), add a `data-testid` to the source component
  instead of reaching for a CSS selector — see `live-score`/`live-phase` in
  `LiveScoringPage.tsx` and `tagged-player-name` in `PlayerSideEditor.tsx`
  for the pattern. **Never** select on Tailwind utility classes (`text-xs`,
  `text-3xl`, `mt-0.5`, …): those are styling, not identity — a font-size or
  spacing tweak in a totally unrelated redesign would silently break the
  test. A `data-testid` only changes if someone deliberately renames it.
- Tests run serially against one shared account and one shared backend
  (`fullyParallel: false`, `workers: 1` in `playwright.config.ts`) — two
  tests racing live-scoring mutations on the same match would be flaky by
  construction. Keep new tests independent by creating their own match
  rather than reusing one from another test.
