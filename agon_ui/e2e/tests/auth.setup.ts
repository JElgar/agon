import { test as setup, expect } from '@playwright/test'
import path from 'path'
import { fileURLToPath } from 'url'
import { requireEnv } from '../support/env'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const authFile = path.join(__dirname, '../.auth/user.json')

/**
 * Signs in once as the fixed e2e test account and saves the resulting
 * storage state (Supabase persists its session in `localStorage`) for every
 * other test to reuse — see `playwright.config.ts`'s `setup` project. Doing
 * this once up front, rather than per-test, keeps the suite fast and avoids
 * hammering Supabase's password-grant rate limit.
 *
 * Deliberately drives the real `LoginForm` email/password path (Supabase
 * `signInWithPassword`), not Google OAuth — OAuth needs a real Google account
 * and consent screen a headless browser can't complete, and the app already
 * offers email/password as a first-class sign-in method (LoginForm's "Or
 * continue with email" section), not just a test-only backdoor. This is also
 * why these tests need a *real* Supabase test user rather than a locally
 * minted JWT: `agon_tests` mints its own tokens against a static JWKS the
 * service trusts (see `agon_tests/tests/api.rs`), which is a fast way to test
 * the API, but never touches the browser's Supabase session or the
 * login/profile-gate UI this suite exists to cover.
 */
setup('log in as the fixed e2e test user', async ({ page }) => {
  const email = requireEnv('E2E_TEST_EMAIL')
  const password = requireEnv('E2E_TEST_PASSWORD')

  await page.goto('/')

  await page.getByLabel('Email').fill(email)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign In', exact: true }).click()

  // A brand-new Supabase account has no Agon profile yet — `CreateProfileForm`
  // gates on it (see App.tsx's `AuthenticatedApp`). Only the very first sign-in
  // of this test account anywhere hits this branch; every run after that lands
  // straight on the feed. Handled here (not per-test) since it's a one-time
  // account setup step, not part of what any individual test is verifying.
  // Both the desktop sidebar and the mobile bottom nav render their own
  // "Feed" link (only one is visible at a given viewport — see
  // `AppSidebar`/`MobileBottomNav`), so scope to one to avoid a strict-mode
  // violation from Playwright matching both.
  const profileHeading = page.getByRole('heading', { name: 'Complete your profile' })
  const feedLink = page.getByRole('link', { name: 'Feed' }).first()
  await expect(profileHeading.or(feedLink)).toBeVisible({ timeout: 20_000 })

  if (await profileHeading.isVisible()) {
    // CreateProfileForm collects first/last name separately (both required
    // to enable the submit button) — there is no single "Display name" field.
    await page.getByLabel('First name').fill('Agon')
    await page.getByLabel('Last name').fill('E2E Bot')
    await page.getByRole('button', { name: 'Create profile' }).click()
    await expect(feedLink).toBeVisible({ timeout: 20_000 })
  }

  await page.context().storageState({ path: authFile })
})
