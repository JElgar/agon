import type { components } from '@/types/api'

type UserSportStats = components['schemas']['UserSportStats']

/** Total matches played across all sports. */
export function totalMatches(stats: UserSportStats[]): number {
  return stats.reduce((sum, s) => sum + s.matches_played, 0)
}

/**
 * Overall win rate (0–100) across all sports, weighted by matches played — the
 * per-sport `win_percentage` values can't just be averaged. Returns 0 when no
 * matches have been played.
 */
export function overallWinRate(stats: UserSportStats[]): number {
  const played = totalMatches(stats)
  if (played === 0) return 0
  const wins = stats.reduce(
    (sum, s) => sum + (s.win_percentage / 100) * s.matches_played,
    0,
  )
  return (wins / played) * 100
}

/** Format a 0–100 win percentage for display, e.g. 58.3 → "58%". */
export function formatWinRate(pct: number): string {
  return `${Math.round(pct)}%`
}

/** Sports ordered by matches played (most active first), for the stats table. */
export function sortedByActivity(stats: UserSportStats[]): UserSportStats[] {
  return [...stats].sort((a, b) => b.matches_played - a.matches_played)
}
