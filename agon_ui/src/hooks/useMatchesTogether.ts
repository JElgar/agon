import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type SearchMatch = components['schemas']['SearchMatch']
type MatchOutcome = components['schemas']['MatchOutcome']

export interface TogetherMatch {
  match: SearchMatch
  /** "with" (same side, i.e. teammates) or "against" (opposing sides). A
   *  genuine draw puts both parties in the same `outcome` bucket regardless
   *  of which side they were actually on (see the backend's
   *  `drawing_participant_ids` doc comment) — there's no way to tell
   *  teammates-who-drew from opponents-who-drew from `outcome` alone, so
   *  a draw is classified "against" (the head-to-head lens is the more
   *  common of the two). */
  kind: 'with' | 'against'
  /** The viewer's own result in this match. */
  viewerOutcome: MatchOutcome | undefined
}

/**
 * Matches two users have both played in, classified as played-together or
 * played-against — powers the profile's "Head to head"/"Playing together"
 * banners and "Matches together" list when viewing someone else's profile
 * (see the Profile board on the "Agon redesign" canvas). No dedicated
 * head-to-head endpoint exists, so this composes two `GET /matches?participant=`
 * calls (one scoped to each user) and intersects them by match id — each
 * call's `outcome` is resolved relative to *its own* `participant`, which is
 * what lets the merge tell "both won" (teammates) apart from "one won, one
 * lost" (opponents) without any new API surface.
 */
export function useMatchesTogether(viewerId: string | undefined, otherUserId: string) {
  const viewerQuery = useQuery({
    queryKey: ['matches-together', 'subject', viewerId, otherUserId],
    enabled: !!viewerId,
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { participant: viewerId, limit: 50 } },
      })
      if (error || !data) throw new Error('Failed to load matches together')
      return data.items
    },
  })
  const otherQuery = useQuery({
    queryKey: ['matches-together', 'subject', otherUserId, viewerId],
    enabled: !!viewerId,
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { participant: otherUserId, limit: 50 } },
      })
      if (error || !data) throw new Error('Failed to load matches together')
      return data.items
    },
  })

  const isLoading = viewerQuery.isLoading || otherQuery.isLoading
  const isError = viewerQuery.isError || otherQuery.isError

  const otherById = new Map((otherQuery.data ?? []).map((m) => [m.id, m]))
  const together: TogetherMatch[] = (viewerQuery.data ?? [])
    .filter((m) => otherById.has(m.id))
    .map((match) => {
      const otherOutcome = otherById.get(match.id)?.outcome
      const kind: TogetherMatch['kind'] =
        match.outcome && otherOutcome && match.outcome === otherOutcome && match.outcome !== 'draw'
          ? 'with'
          : 'against'
      return { match, kind, viewerOutcome: match.outcome }
    })

  const against = together.filter((t) => t.kind === 'against')
  const withTeam = together.filter((t) => t.kind === 'with')

  return {
    isLoading,
    isError,
    together,
    headToHead: {
      youWon: against.filter((t) => t.viewerOutcome === 'won').length,
      draws: against.filter((t) => t.viewerOutcome === 'draw').length,
      theyWon: against.filter((t) => t.viewerOutcome === 'lost').length,
      total: against.length,
    },
    playingTogether: {
      won: withTeam.filter((t) => t.viewerOutcome === 'won').length,
      draws: withTeam.filter((t) => t.viewerOutcome === 'draw').length,
      lost: withTeam.filter((t) => t.viewerOutcome === 'lost').length,
      total: withTeam.length,
    },
    refetch: () => {
      viewerQuery.refetch()
      otherQuery.refetch()
    },
  }
}
