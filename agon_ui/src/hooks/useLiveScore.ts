import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { matchScoreQueryKey } from './useMatchScore'

type LiveEvent = components['schemas']['LiveEvent']
type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']
type CricketLiveEvent = components['schemas']['CricketLiveEvent']
type AppendLiveEventsInput = components['schemas']['AppendLiveEventsInput']
type LiveScoreSnapshot = components['schemas']['LiveScoreSnapshot']

/** Drains every page of a match's raw live event log, oldest first. Also
 *  used by `useOfflineLiveScoring`'s conflict reconciliation, which needs
 *  to read "the log since my old tip" to diff against a rejected batch —
 *  there's no `since_seq` query param, so that's a client-side filter over
 *  the full drain, same as everything else that reads the raw log. */
export async function drainLiveEvents(matchId: string): Promise<LiveEvent[]> {
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
 * score (`useMatchScore`), this has no size ceiling (one DynamoDB item per
 * event) and stays fully readable regardless of match length or status, so a
 * completed match's full run-progression graph reads deliveries from here
 * (see `inningsDeliveriesFromEvents`) rather than the score, which only ever
 * carries the current innings' totals plus a bounded recent-deliveries
 * window. The endpoint itself is paginated (a long match's log can run to
 * thousands of events), so this drains every page — fine for a one-shot
 * fetch on a match detail page, not meant for polling.
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
 * `expected_last_seq` on the next append, not a read of the score itself
 * (see `useMatchScore` for that; there's no separate "live state" endpoint
 * anymore). Seeded once by draining the raw event log, the same
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
 * Raw batch-append call — POSTs directly, no cache reads/writes, no
 * `expected_last_seq` default-guessing. Shared by `useAppendLiveEvent`
 * below (the simple online-only per-tap path) and
 * `useOfflineLiveScoring.ts`'s sync engine, which needs to send a whole
 * queued backlog in ≤99-event chunks against an explicitly tracked
 * baseline, not one event against the live-seq cache's tip. Resolves
 * normally — checking `status`/`error` is the caller's job — for an
 * HTTP-level failure like a `409` conflict; a genuine network failure (no
 * response at all) rejects instead, which is what lets
 * `useOfflineLiveScoring` tell "the server said no" apart from "there's no
 * server to ask" (see `hooks/useOnlineStatus.ts`'s `isNetworkError`).
 */
export async function postLiveEventBatch(
  matchId: string,
  body: AppendLiveEventsInput,
): Promise<{ data: LiveScoreSnapshot | undefined; status: number }> {
  const { data, response } = await fetchClient.POST('/matches/{match_id}/live/events', {
    params: { path: { match_id: matchId } },
    body,
  })
  return { data, status: response.status }
}

/**
 * Appends one sport-tagged live event to a match's log. Reads
 * `expected_last_seq` off the cached tip (0 if scoring hasn't started yet, or
 * this device hasn't loaded it) so the server can detect a lost update; on
 * success the returned snapshot's `last_seq` and `score` replace the tip and
 * score caches directly (cheaper and more current than invalidating +
 * refetching). Shared by the per-sport wrappers below — each just tags its
 * event with the right `sport` discriminator.
 *
 * The server flips a still-`scheduled` match to `in_progress` the first time
 * any live event is recorded, so every append also invalidates the match and
 * feed queries — that's the only signal the scorer's own client has that the
 * status (and therefore other viewers' "Live" gate) may have just changed.
 *
 * This is the plain online-only path — offline-aware recording goes through
 * `useOfflineLiveScoring` instead, which queues locally on a network
 * failure rather than surfacing one.
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
      const { data, status } = await postLiveEventBatch(matchId, {
        expected_last_seq: expected,
        events: [input],
      })
      if (status >= 400 || !data) throw new Error('Failed to record event')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(liveSeqQueryKey(matchId), data.last_seq)
      queryClient.setQueryData(matchScoreQueryKey(matchId), data.score)
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

/**
 * Undo the most recently recorded live event — thin wrapper over
 * `DELETE /matches/:id/live/events/:seq`. Only reached for the *synced*
 * case (an event the server already has): `useOfflineLiveScoring` only
 * calls this when its own local pending queue is empty — while anything's
 * still queued locally, undo pops the queue instead, no network call (see
 * `lib/offlineQueue.ts`'s `popLastPendingEvent`). The backend only accepts
 * `seq` if it's still the current tip (`400`, "only the most recently
 * recorded event can be undone," otherwise) — not a 404/409, so that's
 * checked for explicitly rather than folded into a generic failure. Same
 * cache-write pattern as `useAppendLiveEvent`'s `onSuccess`.
 */
export function useUndoLiveEvent(matchId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (seq: number) => {
      const { data, error, response } = await fetchClient.DELETE(
        '/matches/{match_id}/live/events/{seq}',
        { params: { path: { match_id: matchId, seq } } },
      )
      if (response.status === 400) {
        throw new Error(
          typeof error === 'string' ? error : 'Only the most recently recorded event can be undone',
        )
      }
      if (error || !data) throw new Error('Failed to undo that event')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(liveSeqQueryKey(matchId), data.last_seq)
      queryClient.setQueryData(matchScoreQueryKey(matchId), data.score)
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })
}
