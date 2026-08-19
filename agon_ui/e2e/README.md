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
environment's `VITE_SUPABASE_URL` points at (staging, typically):

- Supabase dashboard → Authentication → Users → **Add user**, with email
  confirmed, or
- the [Supabase Admin API](https://supabase.com/docs/reference/api/introduction)
  (`POST /auth/v1/admin/users` with `email_confirm: true`) using the
  project's service role key.

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
