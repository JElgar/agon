import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { matchScoreQueryKey } from './useMatchScore'

type LiveEvent = components['schemas']['LiveEvent']
type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']
type CricketLiveEvent = components['schemas']['CricketLiveEvent']
type NetballLiveEvent = components['schemas']['NetballLiveEvent']

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

/** The log's real physical tip — the highest `seq` an event actually exists
 *  at, 0 if none yet — by draining the raw event log and taking the max.
 *  Shared queryFn logic for `useLiveSeq` and `useUndoTargetSeq`, which start
 *  out reading the same value but diverge from there (see
 *  `useUndoTargetSeq`'s doc comment). */
async function fetchLivePhysicalTip(matchId: string): Promise<number> {
  const events = await drainLiveEvents(matchId)
  return events.reduce((max, e) => Math.max(max, e.seq), 0)
}

export function liveSeqQueryKey(matchId: string | undefined) {
  return ['live-seq', matchId] as const
}

/**
 * The optimistic-concurrency token for the *next append* — `expected_last_seq`
 * — not a read of the score itself (see `useMatchScore` for that; there's no
 * separate "live state" endpoint anymore). Seeded once by draining the raw
 * event log (the real physical tip, same as `useUndoTargetSeq`); every
 * append or undo after that updates the cached value directly from its own
 * response's `last_seq` instead of refetching.
 *
 * That response `last_seq` is always the match's `live_seq` counter, not
 * necessarily a seq any event still physically exists at (see
 * `Dao::delete_live_event`'s doc comment on the backend: undoing bumps the
 * counter past the deleted event) — correct for gating the next append, but
 * NOT the value to hand `UndoLastEventButton` for undo; use
 * `useUndoTargetSeq` for that instead.
 */
export function useLiveSeq(matchId: string | undefined, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: liveSeqQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    staleTime: Infinity,
    queryFn: () => fetchLivePhysicalTip(matchId!),
  })
}

export function undoTargetSeqQueryKey(matchId: string | undefined) {
  return ['live-undo-target-seq', matchId] as const
}

/**
 * The seq to send `UndoLastEventButton`'s next `DELETE
 * .../live/events/:seq` at — the log's real physical tip, deliberately
 * tracked apart from `useLiveSeq`'s append token even though the two start
 * out equal and an append keeps them in lockstep (neither ever creates a
 * gap). They diverge the moment an undo happens: the append token jumps to
 * the bumped `live_seq` counter (see that hook's doc comment), which no
 * longer points at a real event — sending *that* to a second consecutive
 * undo 404s ("nothing there"), it isn't the next thing to delete.
 *
 * So unlike `useLiveSeq`, an undo's response is never trusted here directly
 * — `useUndoLastLiveEvent`'s `onSuccess` invalidates this instead, forcing a
 * real re-derivation from the event log rather than guessing. An append's
 * response *is* trusted directly here too, same as `useLiveSeq`, since it
 * never creates a gap.
 */
export function useUndoTargetSeq(matchId: string | undefined, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: undoTargetSeqQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    staleTime: Infinity,
    queryFn: () => fetchLivePhysicalTip(matchId!),
  })
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
      // An append never creates a gap — its own new event is always the
      // fresh physical tip too, same value as the append token above. Safe
      // to write directly, sparing `useUndoTargetSeq` an extra refetch on
      // the common (append) path.
      queryClient.setQueryData(undoTargetSeqQueryKey(matchId), data.last_seq)
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

export function useAppendNetballEvent(matchId: string) {
  return useAppendLiveEvent<NetballLiveEvent>(matchId, 'Netball')
}

/**
 * Undoes the most recently recorded live event — `DELETE
 * /matches/:match_id/live/events/:seq`, which the server restricts to the
 * current log tip (see `delete_live_event` on the backend): pass anything
 * else and it 400s rather than deleting mid-log. Callers pass the `seq`
 * they believe is the tip (from `useUndoTargetSeq`, NOT `useLiveSeq` — see
 * that hook's doc comment for why the two aren't interchangeable) rather
 * than this hook reading the cache itself, so a stale button — rendered
 * before an in-flight append's response lands — surfaces that mismatch as
 * an error instead of silently undoing the wrong thing.
 *
 * On success, updates the score cache the same way `useAppendLiveEvent`
 * does, from the response's already-recomputed snapshot rather than an
 * extra refetch. The two tip caches split here: the append token
 * (`liveSeqQueryKey`) trusts the response's `last_seq` directly, same as an
 * append; the undo target (`undoTargetSeqQueryKey`) does not, since an undo
 * always leaves that response value pointing past a real event — it's
 * invalidated instead, forcing a real re-derivation from the event log.
 */
export function useUndoLastLiveEvent(matchId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (seq: number) => {
      const { data, error, response } = await fetchClient.DELETE(
        '/matches/{match_id}/live/events/{seq}',
        { params: { path: { match_id: matchId, seq } } },
      )
      if (response.status === 400) {
        throw new Error('Only the most recently recorded event can be undone')
      }
      if (error || !data) throw new Error('Failed to undo that event')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(liveSeqQueryKey(matchId), data.last_seq)
      queryClient.invalidateQueries({ queryKey: undoTargetSeqQueryKey(matchId) })
      queryClient.setQueryData(matchScoreQueryKey(matchId), data.score)
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })
}
