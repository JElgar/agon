import { test, expect } from '@playwright/test'
import { logFootballMatch, uniqueSuffix } from '../support/logMatch'
import {
  advanceClock,
  goalButton,
  isFirstSide,
  liveScoreBox,
  openMatchFromFeed,
  phaseLabel,
  recordGoal,
  startFootballScoring,
  undoLastEvent,
} from '../support/liveScoring'

test.describe('football live scoring', () => {
  test('goals with an assist, undo, half-time, and finishing the match', async ({ page }) => {
    const opponentName = `E2E Away ${uniqueSuffix()}`
    const { name, homeName, selfName, teammateName, awayPlayerName } = await logFootballMatch(page, {
      opponentName,
    })

    await openMatchFromFeed(page, name)
    await startFootballScoring(page)

    // The API's side order isn't tied to which side was logged first (see
    // `isFirstSide`'s doc comment) — read it off the page once, up front,
    // rather than assuming "Home" is always the left-hand score digit.
    const homeScoresFirst = await isFirstSide(page, homeName)
    const oneNil = homeScoresFirst ? /1\s*–\s*0/ : /0\s*–\s*1/

    // --- First half: a goal with an assist, then undo it ---------------------
    await recordGoal(page, { side: homeName, scorer: teammateName, assist: selfName })
    await expect(liveScoreBox(page)).toHaveText(oneNil)
    await expect(page.getByText(`Goal — ${teammateName} (${selfName})`)).toBeVisible()

    await undoLastEvent(page)
    await expect(liveScoreBox(page)).toHaveText(/0\s*–\s*0/)
    await expect(page.getByText('No events recorded yet.')).toBeVisible()

    // Record it again so the rest of the match has something on the board —
    // also exercises that a goal can be re-recorded cleanly after an undo.
    await recordGoal(page, { side: homeName, scorer: teammateName, assist: selfName })
    await expect(liveScoreBox(page)).toHaveText(oneNil)

    // --- Half-time: the clock stops and scoring is unavailable ---------------
    await advanceClock(page, 'End 1st half')
    await expect(phaseLabel(page)).toContainText('Half-time')
    await expect(goalButton(page)).toHaveCount(0)

    await advanceClock(page, 'Start 2nd half')
    await expect(phaseLabel(page)).toContainText('2nd half')
    await expect(goalButton(page)).toBeVisible()

    // --- Second half: the other side equalises --------------------------------
    await recordGoal(page, { side: opponentName, scorer: awayPlayerName })
    await expect(liveScoreBox(page)).toHaveText(/1\s*–\s*1/)

    // --- Full time, then finish the match -------------------------------------
    await advanceClock(page, 'End match')
    await expect(phaseLabel(page)).toContainText('Full-time')
    await page.getByRole('button', { name: 'Finish match', exact: true }).click()

    await expect(page).toHaveURL(/\/matches\/[^/]+$/)
    await expect(page.getByText(teammateName).first()).toBeVisible()
    await expect(page.getByText(awayPlayerName).first()).toBeVisible()
    await expect(page.getByText(/Confirmed|Unconfirmed/).first()).toBeVisible()
  })
})
