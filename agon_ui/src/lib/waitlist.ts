import type { components } from '@/types/api'

export type RosterConflict = components['schemas']['RosterConflict']
export type WaitlistEntry = components['schemas']['WaitlistEntry']

/** Thrown by a roster join/accept call that hit a full match/side — carries
 *  the typed conflict (see `RosterConflict`) so the UI can offer joining the
 *  waitlist instead of showing a generic failure. Never thrown for
 *  `already_on_roster`; callers that can hit that case check `response.data`
 *  directly (see `JoinLinkBanner`/`TeamJoinBanner`/`JoinMatchPage`), since
 *  there's nothing for the waitlist to offer there.
 */
export class RosterConflictError extends Error {
  conflict: RosterConflict

  constructor(conflict: RosterConflict) {
    super(conflict.message)
    this.name = 'RosterConflictError'
    this.conflict = conflict
  }
}

/** Whether a conflict is one the waitlist can actually help with — the match
 *  or side has no room, as opposed to the caller already being on the roster
 *  (nothing to queue for there). */
export function offersWaitlist(conflict: RosterConflict): boolean {
  return conflict.kind === 'side_full' || conflict.kind === 'match_full'
}
