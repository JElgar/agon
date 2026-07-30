import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type DetailedScore = components['schemas']['DetailedScore']

export function matchDetailedScoreQueryKey(matchId: string | undefined) {
  return ['match-detailed-score', matchId] as const
}

/**
 * A match's resolved sport-specific scorecard (`DetailedScore`) — the
 * per-innings/per-player breakdown, distinct from the live-scoring event log.
 * Populated whether the match was scored live (ball-by-ball) or entered as a
 * finished scorecard directly. `null` (not `undefined`) means the match has
 * no detailed score recorded, a normal state distinct from "still loading".
 */
export function useMatchDetailedScore(
  matchId: string | undefined,
  options?: { enabled?: boolean; refetchInterval?: number | false },
) {
  return useQuery({
    queryKey: matchDetailedScoreQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    refetchInterval: options?.refetchInterval,
    queryFn: async (): Promise<DetailedScore | null> => {
      const { data, response } = await fetchClient.GET('/matches/{match_id}/detailed-score', {
        params: { path: { match_id: matchId! } },
      })
      if (response.status === 404) return null
      if (!data) throw new Error('Failed to load detailed score')
      return data
    },
  })
}
