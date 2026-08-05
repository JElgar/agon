import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type Score = components['schemas']['Score']

export function matchScoreQueryKey(matchId: string | undefined) {
  return ['match-score', matchId] as const
}

/**
 * A match's resolved sport-specific score (`Score`) — the per-innings/
 * per-player breakdown alongside the headline totals, distinct from the
 * live-scoring event log. Populated whether the match was scored live
 * (ball-by-ball) or entered as a finished result directly — it's the same
 * `Score` served here, embedded on `Match.confirmed_score`/`pending_score`,
 * and persisted as the match's live-scoring record. `null` (not `undefined`)
 * means the match has no score recorded, a normal state distinct from
 * "still loading".
 */
export function useMatchScore(
  matchId: string | undefined,
  options?: { enabled?: boolean; refetchInterval?: number | false },
) {
  return useQuery({
    queryKey: matchScoreQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    refetchInterval: options?.refetchInterval,
    queryFn: async (): Promise<Score | null> => {
      const { data, response } = await fetchClient.GET('/matches/{match_id}/score', {
        params: { path: { match_id: matchId! } },
      })
      if (response.status === 404) return null
      if (!data) throw new Error('Failed to load score')
      return data
    },
  })
}
