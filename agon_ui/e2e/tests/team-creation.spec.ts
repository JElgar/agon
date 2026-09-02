import { test, expect } from '@playwright/test'
import { uniqueSuffix } from '../support/logMatch'
import { signInSecondAccount } from '../support/secondAccount'

/**
 * Regression coverage for a bug where clicking a *real* Agon user in
 * `PlayerSideEditor`'s search dropdown silently did nothing: Radix's
 * `Dialog` sets `document.body.style.pointerEvents = 'none'` while open and
 * opts only its own content back in, but the combobox's popup portals to
 * `document.body` as a *sibling* of the dialog, not a descendant — so it
 * inherited `none` and every click on a result passed straight through to
 * the dialog behind it. Search results still rendered fine (that's
 * unaffected by `pointer-events`), which is what made it easy to miss.
 *
 * The only existing coverage of this search box (`logMatch.ts`) only ever
 * exercises the "Add as guest" option — a different code path that never
 * opens a real-user popup row — which is exactly how this shipped unnoticed.
 * This test drives the real-user path specifically: search, click a real
 * result, confirm it's tagged, create the team, and confirm the invite
 * actually went out server-side (not just a client-side list that looked
 * right).
 */
test.describe('team creation — real user search', () => {
  const targetEmail = process.env.E2E_SEARCH_TARGET_EMAIL
  const targetPassword = process.env.E2E_SEARCH_TARGET_PASSWORD

  test('selecting a searched user tags and invites them', async ({ page, browser }) => {
    test.skip(
      !targetEmail || !targetPassword,
      'E2E_SEARCH_TARGET_EMAIL/E2E_SEARCH_TARGET_PASSWORD not set — see e2e/README.md',
    )

    // A second, independent signed-in account so there's a real registered
    // user (other than the caller) to search for — the caller is excluded
    // from their own search results by design. Its own throwaway context, so
    // it never touches the primary account's saved storage state.
    const targetContext = await browser.newContext()
    const targetPage = await targetContext.newPage()
    const targetName = await signInSecondAccount(targetPage, {
      email: targetEmail!,
      password: targetPassword!,
    })
    await targetContext.close()

    const teamName = `E2E Team ${uniqueSuffix()}`

    await page.goto('/teams')
    await page.getByRole('button', { name: 'Create team' }).click()
    await page.getByPlaceholder('e.g. Kent CC').fill(teamName)

    await page.getByPlaceholder('Add a teammate…').fill(targetName)
    // The regression lived here: before the fix, this click landed on the
    // dialog behind the popup and did nothing. A generous timeout — on a
    // fresh environment the target account's profile may only just have been
    // created above and can take a moment to reach the search index.
    await page.getByRole('option', { name: targetName }).click({ timeout: 20_000 })
    await expect(
      page.getByTestId('tagged-player-name').filter({ hasText: targetName }),
    ).toBeVisible()

    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'Create team', exact: true })
      .click()
    await expect(page.getByRole('dialog')).toHaveCount(0)

    // ...and the invite actually went out, not just the client-side tag.
    await page.getByRole('button', { name: teamName }).click()
    await expect(page).toHaveURL(/\/teams\/[^/]+$/)
    await expect(page.getByText(targetName).first()).toBeVisible()
    await expect(page.getByText('· invited')).toBeVisible()
  })
})
