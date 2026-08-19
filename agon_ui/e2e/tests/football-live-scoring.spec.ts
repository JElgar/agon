import { test, expect } from '@playwright/test'
import { logFootballMatch, uniqueSuffix } from '../support/logMatch'
import {
  advanceClock,
  goalButton,
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
    const { homeName, selfName, teammateName, awayPlayerName } = await logFootballMatch(page, {
      opponentName,
    })

    await openMatchFromFeed(page, opponentName)
    await startFootballScoring(page)

    // --- First half: a goal with an assist, then undo it ---------------------
    await recordGoal(page, { side: homeName, scorer: teammateName, assist: selfName })
    await expect(liveScoreBox(page)).toHaveText(/1\s*–\s*0/)
    await expect(page.getByText(`Goal — ${teammateName} (${selfName})`)).toBeVisible()

    await undoLastEvent(page)
    await expect(liveScoreBox(page)).toHaveText(/0\s*–\s*0/)
    await expect(page.getByText('No events recorded yet.')).toBeVisible()

    // Record it again so the rest of the match has something on the board —
    // also exercises that a goal can be re-recorded cleanly after an undo.
    await recordGoal(page, { side: homeName, scorer: teammateName, assist: selfName })
    await expect(liveScoreBox(page)).toHaveText(/1\s*–\s*0/)

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
