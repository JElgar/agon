import type { components } from '@/types/api'

type Member = components['schemas']['Member']
type MatchPlayer = components['schemas']['MatchPlayer']
type MatchSide = components['schemas']['MatchSide']

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
