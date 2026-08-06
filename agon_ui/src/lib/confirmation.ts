import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']

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
 * Also accepts a feed's `FeedMatch`, which carries no roster to scan — it
 * denormalizes the same fact server-side onto `viewer_side_id` instead (see
 * that field's doc comment), used directly when `players` isn't present.
 */
export function mySideId(
  match: Partial<Pick<Match, 'players'>> & Partial<Pick<FeedMatch, 'viewer_side_id'>>,
  currentUserId: string | undefined,
): string | undefined {
  if (!currentUserId) return undefined
  if (match.players) {
    const mine = match.players.find(
      (p) => p.member.type === 'User' && p.member.user_id === currentUserId,
    )
    return mine?.side_id
  }
  return match.viewer_side_id ?? undefined
}

/**
 * Compute the current user's confirmation state for a match's pending score.
 * Drives whether to show the Confirm/Dispute prompt, a passive awaiting state,
 * or nothing (no pending score / not a participant). Accepts a `Match` or a
 * feed's `FeedMatch` — see `mySideId`.
 */
export function confirmationState(
  match: Partial<Pick<Match, 'players'>> &
    Partial<Pick<FeedMatch, 'viewer_side_id'>> &
    Pick<Match, 'pending_score'>,
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
