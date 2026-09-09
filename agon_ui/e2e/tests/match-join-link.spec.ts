import { test, expect } from '@playwright/test'
import { requireEnv } from '../support/env'
import { logFootballMatch, uniqueSuffix } from '../support/logMatch'
import { signInSecondAccount } from '../support/secondAccount'

/**
 * Regression coverage for a bug in the join-link landing screen
 * (`JoinMatchPage`): for a link scoped to "Any side, or unassigned", the
 * side picker defaults to (and visually shows) "Unassigned — pick a side
 * later" pre-selected, but the Join button stayed disabled regardless — an
 * untouched picker and an explicit "unassigned" pick were indistinguishable
 * (`sideId === undefined` either way), and the button's `canSubmit` check
 * only ever treated an *explicit side* as ready to submit.
 *
 * Drives the real flow end to end: create a match (default
 * `allow_unassigned: true`), mint a general join link from the match's "Join
 * links" dialog, then land on it as a *different*, real signed-in account and
 * confirm joining unassigned actually works.
 */
test.describe('match join links', () => {
  test('joining unassigned via a general join link', async ({ page, browser }) => {
    const opponentName = `E2E Away ${uniqueSuffix()}`
    const { name } = await logFootballMatch(page, { opponentName })

    await page.getByText(name, { exact: true }).first().click()
    await expect(page).toHaveURL(/\/matches\/[^/]+$/)

    // Open "Join links" and mint one with the default scope — "Any side, or
    // unassigned" (`sidesChoice: 'any'`, `allowUnassigned: true`) — the exact
    // shape that triggered the bug.
    await page.getByRole('button', { name: 'Join links' }).click()
    const [createResponse] = await Promise.all([
      page.waitForResponse(
        (res) => res.request().method() === 'POST' && res.url().includes('/join-links'),
      ),
      page.getByRole('button', { name: 'Create link', exact: true }).click(),
    ])
    const { token } = await createResponse.json()
    expect(token).toBeTruthy()
    await page.getByRole('button', { name: 'Done', exact: true }).click()

    // A second, independent signed-in account lands on the link itself — its
    // own throwaway context, never touching the primary account's saved
    // storage state (see e2e/README.md's "Provisioning the secondary
    // account").
    const joinerContext = await browser.newContext()
    const joinerPage = await joinerContext.newPage()
    const joinerName = await signInSecondAccount(joinerPage, {
      email: requireEnv('E2E_SECONDARY_EMAIL'),
      password: requireEnv('E2E_TEST_PASSWORD'),
    })

    await joinerPage.goto(`/join/${token}`)
    await expect(joinerPage.getByRole('heading', { name: 'Join this game' })).toBeVisible()

    const joinButton = joinerPage.getByRole('button', { name: 'Join', exact: true })
    // The regression lived here: landing unassigned is the picker's own
    // default (nothing needs picking), so the button should already be
    // enabled — before the fix it stayed disabled until a specific side was
    // chosen, which "Unassigned" itself never counted as.
    await expect(joinButton).toBeEnabled()

    // Also exercise picking it explicitly, not just relying on the default.
    await joinerPage
      .getByLabel('Which side?')
      .selectOption({ label: 'Unassigned — pick a side later' })
    await expect(joinButton).toBeEnabled()

    await joinButton.click()
    await expect(joinerPage).toHaveURL(/\/matches\/[^/]+$/)
    // Confirms both that the join landed the account unassigned, and that
    // the match page's own "Unassigned" roster (shown below the two side
    // columns on this default desktop viewport — see `RosterTabs`) actually
    // surfaces them.
    await expect(joinerPage.getByText(joinerName).first()).toBeVisible()

    await joinerContext.close()
  })
})
