import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type Match = components['schemas']['Match']

/**
 * Fetch (and cache under `['match', matchId]`) a single match by id —
 * `GET /matches/{match_id}`. Shared by every page that loads a match
 * standalone (`MatchDetailPage`, `EditMatchPage`) so they read/write the
 * same cache entry rather than each running their own copy of this query.
 */
export function useMatchQuery(matchId: string | undefined) {
  return useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })
}
