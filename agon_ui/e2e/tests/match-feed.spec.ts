import { test, expect } from '@playwright/test'
import { logFootballMatch, uniqueSuffix } from '../support/logMatch'
import { escapeRegExp } from '../support/util'

test.describe('creating a match', () => {
  test('a newly logged match appears in the feed and opens', async ({ page }) => {
    const opponentName = `E2E Opposition ${uniqueSuffix()}`
    const { name } = await logFootballMatch(page, { opponentName })

    // LogMatchPage navigates to /feed on a successful post. The feed also
    // shows this match instantly from a client-side overlay ahead of the
    // async fan-out worker landing it in `GET /feed` (see
    // `hooks/usePendingMatches`), so there's nothing to wait on here.
    await expect(page.getByText(name, { exact: true }).first()).toBeVisible()
    await expect(page.getByText(opponentName, { exact: true }).first()).toBeVisible()

    // The card's header line (team names + sport badge) is what's actually
    // wired to open the match — the name/title text above is inert (see
    // `MatchCard`; `openMatchFromFeed` in support/liveScoring.ts has the
    // same reasoning). Matching on `opponentName` (unique per run) picks out
    // this card's header button specifically.
    await page.getByRole('button', { name: new RegExp(escapeRegExp(opponentName)) }).first().click()

    await expect(page).toHaveURL(/\/matches\/[^/]+$/)
    await expect(page.getByText(name, { exact: true }).first()).toBeVisible()
    await expect(page.getByText(opponentName, { exact: true }).first()).toBeVisible()
  })
})
