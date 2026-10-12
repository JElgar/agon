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
 * Drives the real three-step "Log a match" flow (`LogMatchPage`) to create an upcoming
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

  // Step 1: sport & time. The match is upcoming by default, an hour out.
  await page.getByRole('button', { name: 'Football' }).click()
  await page.getByLabel('Match name').fill(name)
  await page.getByRole('button', { name: 'Continue', exact: true }).click()

  // Step 2: players. Both sides become one-off sides with typed names.
  await page
    .getByRole('radiogroup', { name: 'Your side is' })
    .getByRole('radio', { name: 'One-off side' })
    .click()

  // The signed-in user is seeded onto "Your side" as soon as their profile
  // loads. Their chip reads "You", so read the real name off its
  // `data-player-name` while it's still the only tagged player.
  const selfRow = page.getByTestId('tagged-player-name').first()
  await expect(selfRow).toBeVisible()
  const selfName = ((await selfRow.getAttribute('data-player-name')) ?? '').trim()

  const sideNameInputs = page.getByLabel('Side name')
  await sideNameInputs.nth(0).fill(homeName)
  await sideNameInputs.nth(1).fill(opponentName)

  // The search dropdown is a Base UI combobox: its rows are `role="option"`.
  const inviteInputs = page.getByPlaceholder('Invite a player or add a guest')
  await inviteInputs.nth(0).fill(teammateName)
  await page.getByRole('option', { name: `Add "${teammateName}" as guest` }).click()

  await inviteInputs.nth(1).fill(awayPlayerName)
  await page.getByRole('option', { name: `Add "${awayPlayerName}" as guest` }).click()

  await page.getByRole('button', { name: 'Continue', exact: true }).click()

  // Step 3: review. Guests get no invite, so the button just creates it.
  await page.getByRole('button', { name: 'Create match', exact: true }).click()
  await expect(page).toHaveURL(/\/feed$/, { timeout: 20_000 })

  return { name, homeName, opponentName, selfName, teammateName, awayPlayerName }
}
