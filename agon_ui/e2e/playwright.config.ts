import { defineConfig, devices } from '@playwright/test'
import path from 'path'
import { fileURLToPath } from 'url'

// `agon_ui` is an ESM package ("type": "module" in package.json), so this
// config has no `__dirname` — derive it from `import.meta.url` instead.
const __dirname = path.dirname(fileURLToPath(import.meta.url))

/**
 * Full end-to-end tests: a real browser drives the real UI (real Supabase
 * login, real API calls, real DynamoDB/Meilisearch on the far end), unlike
 * `agon_tests`, which talks to the API directly via the generated client and
 * bypasses the browser and Supabase entirely. See `e2e/README.md` for how the
 * two suites complement each other and how to configure a target environment.
 *
 * Two things point this at a target:
 *   - `E2E_BASE_URL` — the UI to test. Unset by default, which starts the
 *     local Vite dev server (see the `webServer` block below) — it already
 *     proxies `/api` to staging (see `vite.config.ts`), so `npm run test:e2e`
 *     works with nothing beyond a configured `.env`. Set it to test a
 *     deployed UI (e.g. a preview/staging URL) instead of spawning a local
 *     server.
 *   - `E2E_TEST_EMAIL` / `E2E_TEST_PASSWORD` — the fixed Supabase test
 *     account the `setup` project logs in as (see `e2e/tests/auth.setup.ts`).
 */
const baseURL = process.env.E2E_BASE_URL ?? 'http://localhost:5173'
const usingLocalDevServer = !process.env.E2E_BASE_URL

const authFile = path.join(__dirname, '.auth/user.json')

export default defineConfig({
  testDir: './tests',
  // Every test signs in as the same fixed account and mutates real shared
  // state (matches, live scores) against one backend — running in parallel
  // would let tests race each other's data, so keep it to one worker.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'dot' : 'list',
  timeout: 60_000,
  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  webServer: usingLocalDevServer
    ? {
        command: 'npm run dev',
        url: baseURL,
        reuseExistingServer: !process.env.CI,
        timeout: 60_000,
      }
    : undefined,
  projects: [
    {
      name: 'setup',
      testMatch: /auth\.setup\.ts/,
    },
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], storageState: authFile },
      dependencies: ['setup'],
    },
  ],
})
