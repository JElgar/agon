import { test, expect } from '@playwright/test'
import { logFootballMatch, uniqueSuffix } from '../support/logMatch'

test.describe('creating a match', () => {
  test('a newly logged match appears in the feed and opens', async ({ page }) => {
    const opponentName = `E2E Opposition ${uniqueSuffix()}`
    const { name } = await logFootballMatch(page, { opponentName })

    // LogMatchPage navigates to /feed on a successful post. The feed also
    // shows this match instantly from a client-side overlay ahead of the
    // async fan-out worker landing it in `GET /feed` (see
    // `hooks/usePendingMatches`), so there's nothing to wait on here.
    const card = page.getByText(name, { exact: true }).first()
    await expect(card).toBeVisible()
    await expect(page.getByText(opponentName, { exact: true }).first()).toBeVisible()

    await card.click()
    await expect(page).toHaveURL(/\/matches\/[^/]+$/)
    await expect(page.getByText(name, { exact: true }).first()).toBeVisible()
    await expect(page.getByText(opponentName, { exact: true }).first()).toBeVisible()
  })
})
