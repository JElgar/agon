import type { components } from '@/types/api'
import type { MatchType } from '@/lib/sports'

export type UserStats = components['schemas']['UserStats']
export type GenericPlayerStats = components['schemas']['GenericPlayerStats']
export type CricketPlayerStats = components['schemas']['CricketPlayerStats']
export type FootballPlayerStats = components['schemas']['FootballPlayerStats']
export type BestFigure = components['schemas']['BestFigure']

/** Every stats field on `UserStats`, in the order sport rows should list. */
const SPORT_ORDER: MatchType[] = [
  'cricket',
  'football',
  'tennis',
  'badminton',
  'squash',
  'table_tennis',
  'other',
]

/** One populated sport slot off `UserStats` — the sport tag alongside its
 *  (always-present) common counters, for code that only needs those. */
export interface SportStatsEntry {
  sport: MatchType
  stats: GenericPlayerStats
}

/** Every sport the user has a stats entry for (i.e. has played a confirmed
 *  match in), in `SPORT_ORDER`. Each `CricketPlayerStats`/`FootballPlayerStats`
 *  carries `GenericPlayerStats`'s fields directly (the API flattens them), so
 *  they satisfy this shape as-is. */
export function sportEntries(stats: UserStats): SportStatsEntry[] {
  return SPORT_ORDER.filter((sport) => stats[sport] != null).map((sport) => ({
    sport,
    stats: stats[sport] as GenericPlayerStats,
  }))
}

/** Sports played, most-active first — for the stats table. */
export function sortedByActivity(stats: UserStats): SportStatsEntry[] {
  return [...sportEntries(stats)].sort(
    (a, b) => b.stats.matches_played - a.stats.matches_played,
  )
}

/** Total matches played across all sports. */
export function totalMatches(stats: UserStats): number {
  return sportEntries(stats).reduce((sum, s) => sum + s.stats.matches_played, 0)
}

/**
 * Overall win rate (0–100) across all sports, weighted by matches played — the
 * per-sport `win_percentage` values can't just be averaged. Returns `null`
 * when no confirmed matches have been played (nothing to divide by).
 */
export function overallWinRate(stats: UserStats): number | null {
  const played = totalMatches(stats)
  if (played === 0) return null
  const wins = sportEntries(stats).reduce(
    (sum, s) => sum + ((s.stats.win_percentage ?? 0) / 100) * s.stats.matches_played,
    0,
  )
  return (wins / played) * 100
}

/**
 * Format a 0–100 win percentage for display, e.g. 58.3 → "58%". `null`/
 * `undefined` (no confirmed matches yet) renders as "-", not "0%".
 */
export function formatWinRate(pct: number | null | undefined): string {
  if (pct == null) return '-'
  return `${Math.round(pct)}%`
}
