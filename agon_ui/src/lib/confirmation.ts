import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/** The current user's relationship to a match's pending score. */
export interface ConfirmationState {
  /** The pending submission id to respond to (present only when one exists). */
  submissionId?: string
  /** The side id the current user plays for, if they're a participant. */
  mySideId?: string
  /** There is a pending score AND the user's side has NOT yet confirmed it —
   *  i.e. show them the Confirm / Dispute prompt. */
  canRespond: boolean
  /** There is a pending score the user has already confirmed (or submitted) —
   *  i.e. show a passive "awaiting the other side" state. */
  awaitingOthers: boolean
}

/**
 * Resolve which side the current user plays for on a match: the side of the
 * `MatchPlayer` whose linked account matches `currentUserId`. `undefined` if the
 * user isn't a participant (or isn't identified).
 *
 * Accepts a detail-view `Match` (scans `players`), a feed's `FeedMatch`
 * (denormalizes the same fact server-side onto `viewer_side_id` — no roster
 * to scan), or a search/profile-activity `SearchMatch` (neither — a search
 * hit isn't derived from a per-viewer fan-out, so there's no cheap source
 * for this; always `undefined`).
 */
export function mySideId(
  match: Match | FeedMatch | SearchMatch,
  currentUserId: string | undefined,
): string | undefined {
  if (!currentUserId) return undefined
  if ('players' in match) {
    const mine = match.players.find(
      (p) => p.member.type === 'User' && p.member.user_id === currentUserId,
    )
    return mine?.side_id
  }
  if ('viewer_side_id' in match) {
    return match.viewer_side_id ?? undefined
  }
  return undefined
}

/**
 * Compute the current user's confirmation state for a match's pending score.
 * Drives whether to show the Confirm/Dispute prompt, a passive awaiting state,
 * or nothing (no pending score / not a participant). Accepts a `Match`, a
 * feed's `FeedMatch`, or a `SearchMatch` — see `mySideId`.
 */
export function confirmationState(
  match: Match | FeedMatch | SearchMatch,
  currentUserId: string | undefined,
): ConfirmationState {
  const pending = match.pending_score
  const side = mySideId(match, currentUserId)

  if (!pending || !side) {
    return { submissionId: pending?.submission_id, mySideId: side, canRespond: false, awaitingOthers: false }
  }

  const mySideConfirmed = pending.confirmations.some((c) => c.side_id === side)
  return {
    submissionId: pending.submission_id,
    mySideId: side,
    canRespond: !mySideConfirmed,
    awaitingOthers: mySideConfirmed,
  }
}
