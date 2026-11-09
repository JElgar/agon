import type { components } from '@/types/api'
import { sportLabel, type MatchType } from './sports'

export type LadderRating = components['schemas']['LadderRating']
export type LadderPlacement = components['schemas']['LadderPlacement']

/**
 * The rating ladder a `sport`'s ranked results count towards, or `null` for a
 * sport that has none — `other`, which `POST /matches` refuses to rank at all
 * (400). That refusal is deliberate rather than an oversight:
 * `agon_core::rating::ladder` points out `other` isn't one sport, it's the
 * bucket of every sport we haven't modelled, so a single number over it would
 * imply a hockey result and a chess result are comparable.
 *
 * Phase 1 keys a ladder on the sport tag alone, so a sport's ladder id *is*
 * its own id. A later split into sub-ladders (`"tennis:doubles"` — the
 * `Ladder` newtype exists to keep that split additive) would give one sport
 * several, and this is the function that would grow a format argument: every
 * lookup below goes through it rather than comparing strings itself.
 */
export function ratingLadderFor(sport: MatchType): string | null {
  return sport === 'other' ? null : sport
}

/** Whether a match in this sport can be created ranked at all. */
export function canBeRanked(sport: MatchType): boolean {
  return ratingLadderFor(sport) !== null
}

/**
 * A profile's published rating on `sport`'s ladder, if it has one. Published
 * ratings are public — everybody who can see the profile sees these.
 */
export function ratingFor(
  ratings: LadderRating[] | undefined,
  sport: MatchType,
): LadderRating | undefined {
  const ladder = ratingLadderFor(sport)
  return ladder ? ratings?.find((r) => r.ladder === ladder) : undefined
}

/**
 * The viewer's own progress towards a published rating on `sport`'s ladder.
 *
 * Only ever available for the viewer themselves: `User::rating_placements`
 * comes from `GET /users/me` and nothing exposes anybody else's, because "a
 * stranger reading it would be reading a hint about a number being withheld".
 * Callers pass `undefined` for someone else's profile, which is why the UI
 * treats "no placements" and "not my profile" identically.
 */
export function placementFor(
  placements: LadderPlacement[] | undefined,
  sport: MatchType,
): LadderPlacement | undefined {
  const ladder = ratingLadderFor(sport)
  return ladder ? placements?.find((p) => p.ladder === ladder) : undefined
}

/** Ranked games still to play before this rating is published. */
export function placementsLeft(placement: LadderPlacement): number {
  return Math.max(0, placement.required - placement.played)
}

/**
 * What a row says about a rating that exists but is still being withheld —
 * the same phrase everywhere, with no count in it.
 *
 * The count is left out on purpose (it's on the canvas's own boards this way):
 * a list row is the wrong place to start explaining the placement gate, and
 * "2 to go" invites the reader to treat five games as a countdown to a
 * trustworthy number when the `±` on the far side of it is still ±700. The
 * explicit "4 of 5" belongs only where the screen is already about the
 * viewer's own progress — the sport page's placements card, and the create
 * flow's.
 */
export const PLACEMENTS_LABEL = 'Still in placements'

/** The `±` half-width shown beside a rating, e.g. `"±704"`. */
export function confidenceLabel(confidence: number): string {
  return `±${confidence}`
}

/**
 * Screen-reader text for a rating and its `±`. "1642 ±704" reads as two
 * unrelated numbers otherwise, and the `±` is the half that carries the
 * caveat.
 */
export function ratingAriaLabel(rating: LadderRating): string {
  return `Rating ${rating.rating}, give or take ${rating.confidence}`
}

/**
 * "1 more ranked game and your tennis rating appears." — the line under a
 * placements bar.
 *
 * `placementsLeft() === 0` shouldn't be reachable (a ladder leaves
 * `rating_placements` the moment it publishes, so `played` is always short of
 * `required` here), but a worker that hasn't caught up yet would show it, and
 * "0 more ranked games" would read as a bug.
 */
export function placementsRemainingLine(placement: LadderPlacement, sport: MatchType): string {
  const left = placementsLeft(placement)
  const ladder = sportLabel(sport).toLowerCase()
  if (left === 0) return `Your ${ladder} rating appears with the next result.`
  return `${left} more ranked game${left === 1 ? '' : 's'} and your ${ladder} rating appears.`
}
