import type { components } from '@/types/api'
import type { MatchType } from '@/lib/sports'

type SearchMatch = components['schemas']['SearchMatch']

export type TeamMatchOutcome = 'win' | 'loss' | 'draw'

export interface TeamRecord {
  matches: number
  wins: number
  /** `null` when `matches` is 0 — nothing to divide by. */
  winRatePct: number | null
}

export interface TeamSportRecord extends TeamRecord {
  sport: MatchType
}

/** The side of `match` that belongs to `teamId`, if any. */
function teamSide(match: SearchMatch, teamId: string) {
  return match.sides.find((side) => side.team_id === teamId)
}

/**
 * `teamId`'s result in one match, or `null` when it doesn't count toward a
 * completed record — no confirmed score yet (scheduled/in-progress, or a
 * completed match whose score hasn't been confirmed by both sides) or the
 * team isn't actually a side of this match.
 */
function outcomeFor(match: SearchMatch, teamId: string): TeamMatchOutcome | null {
  if (!match.confirmed_score) return null
  const side = teamSide(match, teamId)
  if (!side) return null
  const { winner_side_id } = match.confirmed_score
  if (winner_side_id == null) return 'draw'
  return winner_side_id === side.id ? 'win' : 'loss'
}

function recordFrom(outcomes: TeamMatchOutcome[]): TeamRecord {
  const matches = outcomes.length
  const wins = outcomes.filter((o) => o === 'win').length
  return {
    matches,
    wins,
    winRatePct: matches === 0 ? null : (wins / matches) * 100,
  }
}

/**
 * A team's overall win/loss record. There's no `GET /teams/{id}/stats`
 * endpoint (or stats fields on `Team`) yet, so this is derived client-side
 * from `matches` — the team's `GET /matches?team_id=` results — the same way
 * `ProfilePage`'s per-user stats already exist server-side but this team
 * equivalent doesn't. Only matches with a confirmed score count, and only
 * over whatever page of matches the caller fetched — see
 * `TEAM_STATS_MATCH_LIMIT` in `TeamPage` for the resulting known gap
 * (an approximation, not a full-history record).
 */
export function teamRecord(matches: SearchMatch[], teamId: string): TeamRecord {
  const outcomes = matches
    .map((m) => outcomeFor(m, teamId))
    .filter((o): o is TeamMatchOutcome => o !== null)
  return recordFrom(outcomes)
}

/**
 * The same record, split by `match_type` (the closest thing to "sport" on a
 * match) — most-played sport first. Same client-side-derived, same-page-of-
 * matches caveat as `teamRecord`.
 */
export function teamSportRecords(matches: SearchMatch[], teamId: string): TeamSportRecord[] {
  const bySport = new Map<MatchType, TeamMatchOutcome[]>()
  for (const match of matches) {
    const outcome = outcomeFor(match, teamId)
    if (outcome === null) continue
    const list = bySport.get(match.match_type) ?? []
    list.push(outcome)
    bySport.set(match.match_type, list)
  }
  return [...bySport.entries()]
    .map(([sport, outcomes]) => ({ sport, ...recordFrom(outcomes) }))
    .sort((a, b) => b.matches - a.matches)
}
