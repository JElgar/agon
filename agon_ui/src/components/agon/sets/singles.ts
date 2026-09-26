import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']

/** The lone player on each side when it's a 1v1 match, else nothing — 1v1
 *  sports show profile photos instead of kit discs. */
export function singlesPlayers(match: Match): [MatchPlayer, MatchPlayer] | null {
  const [a, b] = match.sides
  if (!a || !b || match.sides.length !== 2) return null
  const pa = match.players.filter((p) => p.side_id === a.id)
  const pb = match.players.filter((p) => p.side_id === b.id)
  return pa.length === 1 && pb.length === 1 ? [pa[0], pb[0]] : null
}
