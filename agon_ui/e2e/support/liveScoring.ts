import { type Page, type Locator, expect } from '@playwright/test'

/** Opens a match from the feed by the name it was logged under (see
 *  `logFootballMatch`) and lands on its detail page. */
export async function openMatchFromFeed(page: Page, matchName: string): Promise<void> {
  await page.goto('/feed')
  await page.getByText(matchName, { exact: true }).first().click()
  await expect(page).toHaveURL(/\/matches\/[^/]+$/)
}

/**
 * From a football match's detail page, starts live scoring: "Score live" →
 * the track-preferences setup screen → "Start scoring", which itself
 * records the kickoff period marker (see `LiveScoringSetupPage`). Lands on
 * `/matches/:id/live` with the clock already running (1st half).
 */
export async function startFootballScoring(page: Page): Promise<void> {
  await page.getByRole('link', { name: 'Score live' }).click()
  await expect(page).toHaveURL(/\/live\/setup$/)
  await page.getByRole('button', { name: 'Start scoring', exact: true }).click()
  await expect(page).toHaveURL(/\/live$/)
  await expect(phaseLabel(page)).toContainText('1st half')
}

/** The live-scoring screen's own score box, e.g. "1–0" — just digits and a
 *  dash with no accessible name to target, hence the `data-testid` (see
 *  `LiveScoringPage`'s `live-score` element and `e2e/README.md`'s locator
 *  guidance). */
export function liveScoreBox(page: Page): Locator {
  return page.getByTestId('live-score')
}

/** The running-clock/phase line under the score, e.g. "12' · 1st half" or
 *  just "Half-time" once the clock stops — see `LiveScoringPage`'s
 *  `live-phase` element. */
export function phaseLabel(page: Page): Locator {
  return page.getByTestId('live-phase')
}

/**
 * Whether `sideName` is the side shown first (left digit of the score box,
 * first name in the "{nameA} vs {nameB}" heading) on the live-scoring page.
 * The API returns a match's sides in a stable-but-otherwise-arbitrary order
 * (sorted by internal side id, not creation order — see
 * `agon_core/src/dao/match_ops.rs`'s `sorted_sides` doc comment), so a test
 * can't assume "the side logged first" ends up first here; it has to read
 * the actual order off the page.
 */
export async function isFirstSide(page: Page, sideName: string): Promise<boolean> {
  const heading = (await page.getByRole('heading', { level: 1 }).innerText()).trim()
  return heading.startsWith(sideName)
}

/**
 * Opens the goal dialog from a quick-action tap, fills side/scorer/(assist),
 * and submits — the same steps a scorer taps through live. `scorer` and
 * `assist` are matched by the exact name shown in the roster picker (see
 * `logFootballMatch`'s returned names).
 */
export async function recordGoal(
  page: Page,
  { side, scorer, assist }: { side: string; scorer: string; assist?: string },
): Promise<void> {
  await page.getByRole('button', { name: 'Goal', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()

  await dialog.getByRole('button', { name: side, exact: true }).click()
  await dialog.getByRole('button', { name: scorer, exact: true }).click()
  if (assist) {
    // Once a scorer is picked, the dialog shows the (still unfiltered)
    // Scorer picker *and* the Assist picker at once — if the assist happens
    // to still be an option in the Scorer list too (i.e. isn't the player
    // just picked as scorer), the same name matches a button in both
    // sections. The Assist section always renders after Scorer, so its
    // match is the last one.
    await dialog.getByRole('button', { name: assist, exact: true }).last().click()
  }
  await dialog.getByRole('button', { name: 'Add goal', exact: true }).click()
  await expect(dialog).toBeHidden()
}

/** Undo the most recently recorded live event, confirming the destructive
 *  prompt — see `UndoLastEventButton`. */
export async function undoLastEvent(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Undo last' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'Undo the last event?' })).toBeVisible()
  await dialog.getByRole('button', { name: 'Yes, undo it' }).click()
  await expect(dialog).toBeHidden()
}

/** Taps the clock-advance tile (kickoff/half-time/full-time and their
 *  extra-time equivalents — see `nextPhaseActionLabel`) by its current
 *  label, e.g. "End 1st half" or "Start 2nd half". */
export async function advanceClock(page: Page, label: string): Promise<void> {
  await page.getByRole('button', { name: label, exact: true }).click()
}

/** Whether a goal/card/sub quick-action is currently offered — false during
 *  a break (half-time, full-time) or before kickoff. */
export function goalButton(page: Page): Locator {
  return page.getByRole('button', { name: 'Goal', exact: true })
}
