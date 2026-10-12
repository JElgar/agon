import type { TaggedPlayer } from '@/components/agon/PlayerSideEditor'
import type { MatchType } from './sports'

/** A stable key for a tagged player, for React keys and de-duping. */
export function taggedPlayerKey(p: TaggedPlayer): string {
  return p.kind === 'user' ? `user:${p.id}` : `ext:${p.name.toLowerCase()}`
}

/** Games won by each side in one set, as typed. */
export interface SetRow {
  a: string
  b: string
}

/** The sets that have a real score in them, as numbers. */
export function playedSets(rows: SetRow[]): { a: number; b: number }[] {
  return rows
    .map((r) => ({ a: Number(r.a), b: Number(r.b) }))
    .filter(
      (r) => Number.isFinite(r.a) && Number.isFinite(r.b) && r.a >= 0 && r.b >= 0 && (r.a > 0 || r.b > 0),
    )
}

const FORMAT_DEFAULT_HINT: Partial<Record<MatchType, string>> = {
  football: 'Skip it and the match uses 2 × 45 min with no extra time.',
  cricket: 'Skip it and the match uses T20 rules.',
  netball: 'Skip it and the match uses 4 × 15 min quarters.',
}

/** What applies when no match format is picked; `undefined` for sports
 *  with no format modelled. */
export function formatDefaultHint(sport: MatchType): string | undefined {
  return FORMAT_DEFAULT_HINT[sport]
}
