import type { components } from '@/types/api'
import type { MatchType } from '@/lib/sports'

type TeamSummary = components['schemas']['TeamSummary']
type SportMatchStats = components['schemas']['SportMatchStats']

/**
 * A team's record against other teams: league, cup and friendly matches
 * (squad games have no W/D/L for the team, so they're left out), all time.
 */
export interface TeamRecord {
  matches: number
  wins: number
  /** `null` when `matches` is 0 — nothing to divide by. */
  winRatePct: number | null
}

export interface TeamSportRecord extends TeamRecord {
  sport: MatchType
}

function recordOf(played: number, won: number): TeamRecord {
  return {
    matches: played,
    wins: won,
    winRatePct: played === 0 ? null : (won / played) * 100,
  }
}

/**
 * One record per sport the team plays, most matches first, from each sport's
 * `GET /teams/{id}/summary`. These are full-history totals from the stats
 * store, not an approximation over recent matches.
 */
export function teamSportRecords(summaries: TeamSummary[]): TeamSportRecord[] {
  return summaries
    .map((s) => ({ sport: s.sport, ...recordOf(s.record.played, s.record.won) }))
    .filter((r) => r.matches > 0)
    .sort((a, b) => b.matches - a.matches)
}

/** The team's overall record: every sport's summed. */
export function teamRecord(summaries: TeamSummary[]): TeamRecord {
  const played = summaries.reduce((n, s) => n + s.record.played, 0)
  const won = summaries.reduce((n, s) => n + s.record.won, 0)
  return recordOf(played, won)
}

/**
 * The one counter a leaderboard row shows next to appearances, in its
 * sport's own terms: goals for football and netball, runs for cricket.
 * `null` for a sport with no counters yet (tennis, squash, …) or a player
 * with none.
 */
export function headlineStat(
  sport: MatchType,
  stats: SportMatchStats | undefined,
): { label: string; value: number } | null {
  if (!stats) return null
  switch (sport) {
    case 'football':
      return stats.football ? { label: 'Goals', value: stats.football.goals ?? 0 } : null
    case 'netball':
      return stats.netball ? { label: 'Goals', value: stats.netball.goals ?? 0 } : null
    case 'cricket':
      return stats.cricket ? { label: 'Runs', value: stats.cricket.runs ?? 0 } : null
    default:
      return null
  }
}
