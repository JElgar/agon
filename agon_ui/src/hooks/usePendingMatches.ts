import { useQuery, type QueryClient } from '@tanstack/react-query'
import type { components } from '@/types/api'

type Match = components['schemas']['Match']

/**
 * Cache key for the "pending matches" overlay: matches the viewer just created
 * that haven't yet been fanned out into `GET /feed` by the async worker.
 *
 * The feed is eventually consistent — the worker writes feed entries off the
 * DynamoDB stream after creation — so a freshly created match isn't in `/feed`
 * for a beat. Rather than optimistically prepend into the feed cache (which the
 * refetch-on-mount would clobber), we keep created matches in this separate
 * store and merge them on top of the fetched feed until the server catches up.
 */
const PENDING_MATCHES_KEY = ['feed-pending-matches'] as const

/**
 * Record a just-created match so it shows in the feed immediately. Deduped by
 * id (re-adding the same match is a no-op) and prepended so the newest is first.
 */
export function addPendingMatch(queryClient: QueryClient, match: Match): void {
  queryClient.setQueryData<Match[]>(PENDING_MATCHES_KEY, (prev) => {
    const rest = (prev ?? []).filter((m) => m.id !== match.id)
    return [match, ...rest]
  })
}

/**
 * Read the pending-match overlay reactively. Backed by a never-fetching query
 * (data only ever arrives via [`addPendingMatch`] / [`prunePendingMatches`]), so
 * components re-render when the overlay changes. Empty when nothing is pending.
 */
export function usePendingMatches(): Match[] {
  const { data } = useQuery<Match[]>({
    queryKey: PENDING_MATCHES_KEY,
    // The overlay is only ever populated by writes; this seeds an empty list and
    // never overwrites a populated one (it can't run while data is fresh).
    queryFn: () => [],
    staleTime: Infinity,
    gcTime: Infinity,
  })
  return data ?? []
}

/**
 * Drop pending matches once the real feed contains them, so the overlay doesn't
 * double-render a match or grow without bound. Call from an effect with the ids
 * present in the fetched feed (not during render — it mutates the cache).
 */
export function prunePendingMatches(queryClient: QueryClient, serverIds: Set<string>): void {
  queryClient.setQueryData<Match[]>(PENDING_MATCHES_KEY, (prev) => {
    if (!prev || prev.length === 0) return prev
    const kept = prev.filter((m) => !serverIds.has(m.id))
    return kept.length === prev.length ? prev : kept
  })
}
