import { fetchClient } from './api-client'
import type { components } from '@/types/api'
import { RosterConflictError, type RosterConflict } from './waitlist'

type InvitationResponse = components['schemas']['InvitationResponse']

/**
 * Respond to an invitation by id — shared by every accept/decline surface
 * (`InviteBanner`, `MatchCard`, `NotificationsPage`, `TeamPage`) so they don't
 * each re-implement the same 409 classification. Throws `RosterConflictError`
 * when accepting hit a full match/side (only ever possible for a match
 * invite — a team invite's `respond` call just never gets a 409 here), or a
 * plain `Error` for anything else.
 */
export async function respondToInvitation(
  invitationId: string,
  response: InvitationResponse,
): Promise<void> {
  const { error, response: httpResponse } = await fetchClient.POST(
    '/invitations/{invitation_id}/respond',
    { params: { path: { invitation_id: invitationId } }, body: { response } },
  )
  if (!error) return
  if (httpResponse.status === 409) {
    throw new RosterConflictError(error as RosterConflict)
  }
  throw new Error('Failed to respond to invitation')
}

/** `respondToInvitation`'s by-token counterpart (`AcceptInvitePage`). */
export async function respondToInvitationByToken(
  inviteToken: string,
  response: InvitationResponse,
): Promise<void> {
  const { error, response: httpResponse } = await fetchClient.POST(
    '/invitations/respond-by-token',
    { body: { invite_token: inviteToken, response } },
  )
  if (!error) return
  if (httpResponse.status === 409) {
    throw new RosterConflictError(error as RosterConflict)
  }
  throw new Error('Failed to respond to invitation')
}
