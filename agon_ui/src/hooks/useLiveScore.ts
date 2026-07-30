import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { matchDetailedScoreQueryKey } from './useMatchDetailedScore'

type LiveEvent = components['schemas']['LiveEvent']
type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']
type CricketLiveEvent = components['schemas']['CricketLiveEvent']

/** Drains every page of a match's raw live event log, oldest first. */
async function drainLiveEvents(matchId: string): Promise<LiveEvent[]> {
  const events: LiveEvent[] = []
  let cursor: string | undefined
  for (;;) {
    const { data, response } = await fetchClient.GET('/matches/{match_id}/live/events', {
      params: { path: { match_id: matchId }, query: { cursor, limit: 50 } },
    })
    if (response.status === 404) return []
    if (!data) throw new Error('Failed to load live events')
    events.push(...data.items)
    if (!data.next_cursor) break
    cursor = data.next_cursor
  }
  return events
}

/**
 * A match's raw live-scoring event log, in append order. Unlike the derived
 * scorecard (`useMatchDetailedScore`), this has no size ceiling (one
 * DynamoDB item per event) and stays fully readable regardless of match
 * length or status, so a completed match's full run-progression graph reads
 * deliveries from here (see `inningsDeliveriesFromEvents`) rather than the
 * scorecard, which only ever carries the current innings' totals plus a
 * bounded recent-deliveries window. The endpoint itself is paginated (a long
 * match's log can run to thousands of events), so this drains every page —
 * fine for a one-shot fetch on a match detail page, not meant for polling.
 */
export function useLiveEvents(matchId: string | undefined, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['live-events', matchId],
    enabled: !!matchId && (options?.enabled ?? true),
    queryFn: () => drainLiveEvents(matchId!),
  })
}

export function liveSeqQueryKey(matchId: string | undefined) {
  return ['live-seq', matchId] as const
}

/**
 * The event log's current tip (`seq` of the most recently recorded event, 0
 * if none yet) — needed only for the optimistic-concurrency
 * `expected_last_seq` on the next append, not a read of the scorecard itself
 * (see `useMatchDetailedScore` for that; there's no separate "live state"
 * endpoint anymore). Seeded once by draining the raw event log, the same
 * one-shot-fetch pattern as `useLiveEvents`; every append after that updates
 * the cached value directly from its own response instead of refetching.
 */
export function useLiveSeq(matchId: string | undefined, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: liveSeqQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    staleTime: Infinity,
    queryFn: async (): Promise<number> => {
      const events = await drainLiveEvents(matchId!)
      return events.reduce((max, e) => Math.max(max, e.seq), 0)
    },
  })
}

/**
 * Appends one sport-tagged live event to a match's log. Reads
 * `expected_last_seq` off the cached tip (0 if scoring hasn't started yet, or
 * this device hasn't loaded it) so the server can detect a lost update; on
 * success the returned snapshot's `last_seq` and `detail` replace the tip and
 * scorecard caches directly (cheaper and more current than invalidating +
 * refetching). Shared by the per-sport wrappers below — each just tags its
 * event with the right `sport` discriminator.
 *
 * The server flips a still-`scheduled` match to `in_progress` the first time
 * any live event is recorded, so every append also invalidates the match and
 * feed queries — that's the only signal the scorer's own client has that the
 * status (and therefore other viewers' "Live" gate) may have just changed.
 */
function useAppendLiveEvent<T extends { kind: string }>(
  matchId: string,
  sport: NewLiveEventInput['event']['sport'],
) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (event: T) => {
      const expected = queryClient.getQueryData<number>(liveSeqQueryKey(matchId)) ?? 0
      const input: NewLiveEventInput = {
        occurred_at: new Date().toISOString(),
        event: { sport, ...event } as NewLiveEventInput['event'],
      }
      const { data, error } = await fetchClient.POST('/matches/{match_id}/live/events', {
        params: { path: { match_id: matchId } },
        body: {
          expected_last_seq: expected,
          events: [input],
        },
      })
      if (error || !data) throw new Error('Failed to record event')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(liveSeqQueryKey(matchId), data.last_seq)
      queryClient.setQueryData(matchDetailedScoreQueryKey(matchId), data.detail)
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })
}

export function useAppendFootballEvent(matchId: string) {
  return useAppendLiveEvent<FootballLiveEvent>(matchId, 'Football')
}

export function useAppendCricketEvent(matchId: string) {
  return useAppendLiveEvent<CricketLiveEvent>(matchId, 'Cricket')
}
