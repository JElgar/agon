import type { components } from '@/types/api'

type Member = components['schemas']['Member']
type MatchPlayer = components['schemas']['MatchPlayer']
type MatchSide = components['schemas']['MatchSide']
type Match = components['schemas']['Match']
type Invitation = components['schemas']['Invitation']
// The generated `invitation.kind` type erases the discriminant (`Omit<…,"type">
// & unknown`), so `.type` won't narrow. Use the real union for token extraction.
type InvitationKind = components['schemas']['InvitationKind']

/**
 * The bearer invite token for a member with a pending token-invitation, else
 * null. Only token-kind invitations (external people invited by name) carry a
 * shareable link; user-kind invites are accepted in-app by the target account.
 */
export function memberInviteToken(member: Member): string | null {
  const invitation = member.invitation
  if (!invitation || invitation.status !== 'pending') return null
  const kind = invitation.kind as InvitationKind
  return kind.type === 'Token' ? kind.invite_token : null
}

/** Absolute invite-link URL for a token, matching the `/invite/:token` route. */
export function inviteLink(token: string): string {
  return `${window.location.origin}/invite/${encodeURIComponent(token)}`
}

/**
 * The viewer's own pending invitation to a match, if they've been invited (as a
 * known Agon user) and haven't yet responded — else null. Lets match views show
 * the viewer their invite and an accept/decline action, mirroring the inbox.
 * Only user-kind invites apply: the viewer is a signed-in account, matched by id.
 */
export function myPendingInvitation(
  match: Pick<Match, 'players'>,
  currentUserId: string | undefined,
): Invitation | null {
  if (!currentUserId) return null
  for (const player of match.players) {
    if (player.member.type !== 'User') continue
    if (player.member.user_id !== currentUserId) continue
    const invitation = player.member.invitation
    if (invitation && invitation.status === 'pending') return invitation
  }
  return null
}

/**
 * Display name for a match player. A linked Agon user's name is resolved from
 * their account (not present on the member itself yet), so until profiles are
 * hydrated we fall back to a short id; an external player carries a display name
 * directly.
 */
export function memberName(member: Member): string {
  if (member.type === 'External') return member.display_name
  // UserMember has only user_id here; the display name comes from the account.
  // Callers that have the resolved profile should pass its name instead.
  return 'Player'
}

/** Initials for an avatar, from a display name (e.g. "Sofia Lindqvist" → "SL"). */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

/** The players assigned to a given side. */
export function playersOnSide(
  players: MatchPlayer[],
  side: MatchSide,
): MatchPlayer[] {
  return players.filter((p) => p.side_id === side.id)
}

/** The stable member id for a player, used to key rows and match score events. */
export function playerId(player: MatchPlayer): string {
  return player.member.id
}
