import { type Page, expect } from '@playwright/test'

/**
 * Signs in as a second, independent Supabase test account — distinct from
 * the primary `E2E_TEST_EMAIL`/`E2E_TEST_PASSWORD` account the suite's
 * `setup` project logs in as — and returns their Agon display name.
 *
 * Only needed by a test that must search for and select a *real* registered
 * user (as opposed to tagging a guest): the primary account can't play that
 * role, since `PlayerSideEditor`/`TeamPicker` search excludes the signed-in
 * caller themselves (see their `currentUserId` prop) so a person can't add
 * themselves twice. Run this against its own `browser.newContext()`, never
 * the shared `page` fixture — signing in there would overwrite the primary
 * account's saved storage state for every other test in the run.
 *
 * First run only: like `auth.setup.ts`, a brand-new account has no Agon
 * profile yet, so this completes `CreateProfileForm` with a fixed name.
 * Every run after that finds the profile already there and just reads the
 * same name back — the account is provisioned once, the same way the
 * primary one is (see e2e/README.md).
 */
export async function signInSecondAccount(
  page: Page,
  { email, password }: { email: string; password: string },
): Promise<string> {
  await page.goto('/')

  await page.getByLabel('Email').fill(email)
  await page.getByLabel('Password').fill(password)
  await page.getByRole('button', { name: 'Sign In', exact: true }).click()

  const profileHeading = page.getByRole('heading', { name: 'Complete your profile' })
  const feedLink = page.getByRole('link', { name: 'Feed' }).first()
  await expect(profileHeading.or(feedLink)).toBeVisible({ timeout: 20_000 })

  if (await profileHeading.isVisible()) {
    await page.getByLabel('First name').fill('Agon')
    await page.getByLabel('Last name').fill('Search Target')
    await page.getByRole('button', { name: 'Create profile' }).click()
    await expect(feedLink).toBeVisible({ timeout: 20_000 })
  }

  // Read the name back from their own profile page rather than assuming how
  // first/last name combine into `profile.name` — same reasoning as
  // `logMatch.ts`'s `selfName`.
  await page.goto('/profile')
  const heading = page.locator('h1').first()
  await expect(heading).toBeVisible()
  return (await heading.innerText()).trim()
}
