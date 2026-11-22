import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

export type Tournament = components['schemas']['Tournament']
export type TournamentEntry = components['schemas']['TournamentEntry']
export type TournamentMatchSummary = components['schemas']['TournamentMatchSummary']

export function tournamentFormatLabel(format: Tournament['format']): string {
  return format === 'knockout' ? 'Knockout' : 'League'
}

export function tournamentStatusLabel(status: string): string {
  switch (status) {
    case 'open':
      return 'Scheduled'
    case 'in_progress':
      return 'In progress'
    case 'completed':
      return 'Completed'
    case 'cancelled':
      return 'Cancelled'
    default:
      return status
  }
}

export function tournamentEntryAudienceLabel(audience: Tournament['entry_audience']): string {
  return audience === 'teams' ? 'Teams' : 'Players'
}

/** Every entry in a tournament, oldest first — walks
 *  `GET /tournaments/{id}/entries` to the end. */
export async function listAllTournamentEntries(tournamentId: string): Promise<TournamentEntry[]> {
  const items: TournamentEntry[] = []
  let cursor: string | undefined
  do {
    const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}/entries', {
      params: { path: { tournament_id: tournamentId }, query: { cursor, limit: 50 } },
    })
    if (error || !data) throw new Error('Failed to load entries')
    items.push(...data.items)
    cursor = data.next_cursor
  } while (cursor)
  return items
}

/** Every match generated for a tournament, oldest first — walks
 *  `GET /tournaments/{id}/matches` to the end. */
export async function listAllTournamentMatches(
  tournamentId: string,
): Promise<TournamentMatchSummary[]> {
  const items: TournamentMatchSummary[] = []
  let cursor: string | undefined
  do {
    const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}/matches', {
      params: { path: { tournament_id: tournamentId }, query: { cursor, limit: 50 } },
    })
    if (error || !data) throw new Error("Failed to load this tournament's matches")
    items.push(...data.items)
    cursor = data.next_cursor
  } while (cursor)
  return items
}

/** Group a tournament's matches by round, in round order — the shared shape
 *  for both a knockout bracket's columns and a league's matchday sections. */
export function groupMatchesByRound(
  matches: TournamentMatchSummary[],
): [number, TournamentMatchSummary[]][] {
  const byRound = new Map<number, TournamentMatchSummary[]>()
  for (const m of matches) {
    const list = byRound.get(m.round) ?? []
    list.push(m)
    byRound.set(m.round, list)
  }
  return Array.from(byRound.entries()).sort(([a], [b]) => a - b)
}
