import { type Page, expect } from '@playwright/test'

/** A short, run-unique suffix so repeated suite runs never collide on match
 *  names — there's no test-data cleanup step, so created matches just
 *  accumulate in the feed like any other match would. */
export function uniqueSuffix(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
}

export interface LoggedFootballMatch {
  name: string
  /** Side A's custom name, as typed into the form. */
  homeName: string
  /** Side B's custom name, as typed into the form. */
  opponentName: string
  /** The signed-in test account's own display name, read back from its
   *  auto-seeded roster entry — see the function doc comment for why this
   *  is discovered rather than hardcoded. */
  selfName: string
  teammateName: string
  awayPlayerName: string
}

/**
 * Drives the real "Log a match" flow (`LogMatchPage`) to create a scheduled
 * football match with a full-enough roster for live-scoring tests to record
 * goals, an assist, and events on both sides:
 *   - Side A ("Home"): the signed-in user (seeded automatically) plus one
 *     guest teammate — two players, so the goal dialog's assist picker has
 *     someone to offer (`RecordEventDialog` only shows it once
 *     `roster.length > 1`).
 *   - Side B (`opponentName`): one guest player, so a second-half goal can
 *     be attributed to a real player rather than needing an own-goal.
 *
 * The signed-in user's own display name isn't something this suite controls
 * (it's whatever the fixed e2e Supabase account was named on signup — see
 * `auth.setup.ts`), so it's read back from the seeded roster row rather than
 * assumed, for tests that need to pick them as an assist.
 *
 * Leaves the browser on the feed, which is where a successful submit
 * navigates to.
 */
export async function logFootballMatch(
  page: Page,
  { opponentName }: { opponentName: string },
): Promise<LoggedFootballMatch> {
  const name = `E2E football ${uniqueSuffix()}`
  const homeName = 'Home'
  const teammateName = 'Guest Teammate'
  const awayPlayerName = 'Away Striker'

  await page.goto('/matches/new')

  await page.getByRole('button', { name: 'Football' }).click()
  await page.getByPlaceholder('e.g. Tuesday night singles').fill(name)

  // The signed-in user is seeded onto "Your side" as soon as their profile
  // loads — read their name back before adding anyone else, while there's
  // still exactly one tagged-player row on the page to disambiguate (see
  // `PlayerSideEditor`'s `tagged-player-name` element).
  const selfRow = page.getByTestId('tagged-player-name').first()
  await expect(selfRow).toBeVisible()
  const selfName = (await selfRow.innerText()).trim()

  const sideNameInputs = page.getByPlaceholder('Name this side (optional)')
  await sideNameInputs.nth(0).fill(homeName)
  await sideNameInputs.nth(1).fill(opponentName)

  await page.getByPlaceholder('Add a teammate…').fill(teammateName)
  await page.getByRole('button', { name: `Add "${teammateName}" as guest` }).click()

  await page.getByPlaceholder('Add an opponent…').fill(awayPlayerName)
  await page.getByRole('button', { name: `Add "${awayPlayerName}" as guest` }).click()

  await page.getByRole('button', { name: 'Post match', exact: true }).click()
  await expect(page).toHaveURL(/\/feed$/, { timeout: 20_000 })

  return { name, homeName, opponentName, selfName, teammateName, awayPlayerName }
}
