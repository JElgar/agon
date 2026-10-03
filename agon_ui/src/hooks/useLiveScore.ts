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
 *  Used by `useUndoTargetSeq` only. NOT safe for seeding `useLiveSeq`'s
 *  append token — see that hook's doc comment for why the two are not
 *  interchangeable once any event has ever been undone. */
async function fetchLivePhysicalTip(matchId: string): Promise<number> {
  const events = await drainLiveEvents(matchId)
  return events.reduce((max, e) => Math.max(max, e.seq), 0)
}

/** The match's real `live_seq` counter, straight from the server —
 *  `GET /matches/:id/live/seq`. Shared queryFn for `useLiveSeq`'s seed. */
async function fetchLiveSeq(matchId: string): Promise<number> {
  const { data, response } = await fetchClient.GET('/matches/{match_id}/live/seq', {
    params: { path: { match_id: matchId } },
  })
  if (response.status === 404) return 0
  if (!data) throw new Error('Failed to load live seq')
  return data.last_seq
}

export function liveSeqQueryKey(matchId: string | undefined) {
  return ['live-seq', matchId] as const
}

/**
 * The optimistic-concurrency token for the *next append* — `expected_last_seq`
 * — not a read of the score itself (see `useMatchScore` for that; there's no
 * separate "live state" endpoint for scores). Seeded from `GET
 * /matches/:id/live/seq`, the server's actual `live_seq` counter; every
 * append or undo after that updates the cached value directly from its own
 * response's `last_seq` instead of refetching.
 *
 * This must NOT be seeded from the physical event log's max seq the way
 * `useUndoTargetSeq` is (regression: an earlier version of this hook did
 * exactly that via the same `fetchLivePhysicalTip` helper). The two start
 * out equal, but permanently diverge the moment an event is ever undone —
 * `live_seq` counts every mutation ever made (including deletes), while the
 * physical log's max seq only ever reflects what's still there (see
 * `Dao::delete_live_event`'s doc comment on the backend). A device that
 * seeds its append token from the log instead of the real counter sends a
 * stale `expected_last_seq` on its very next append and gets rejected with
 * `Conflict` forever after that — exactly what a page refresh right after an
 * undo used to do, since the refresh throws away the in-session cache
 * (populated correctly by the undo's own response) and reseeds from scratch.
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
    queryFn: () => fetchLiveSeq(matchId!),
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
 *
 * `useLiveSeq`'s `staleTime: Infinity` means nothing ever refetches this
 * tab's cached tip in the background — so if another client (another tab,
 * or the Garmin watch app's own scorer, which has this exact same
 * optimistic-concurrency handshake) appends while this tab is open, the
 * very next append here comes back `409 Conflict` with a now-stale cached
 * tip that nothing was fixing, needing a full page reload to reseed it —
 * reported directly as "have to refresh the page after an event comes in
 * from the watch app". Fixed the same way the watch app's own client
 * retries after a conflict: re-fetch the real tip and retry this exact
 * event with it once, rather than just failing and leaving the cache
 * stale for next time too.
 */
function useAppendLiveEvent<T extends { kind: string }>(
  matchId: string,
  sport: NewLiveEventInput['event']['sport'],
) {
  const queryClient = useQueryClient()

  const sendAppend = (event: T, expected: number) => {
    const input: NewLiveEventInput = {
      occurred_at: new Date().toISOString(),
      event: { sport, ...event } as NewLiveEventInput['event'],
    }
    return fetchClient.POST('/matches/{match_id}/live/events', {
      params: { path: { match_id: matchId } },
      body: {
        expected_last_seq: expected,
        events: [input],
      },
    })
  }

  return useMutation({
    mutationFn: async (event: T) => {
      const expected = queryClient.getQueryData<number>(liveSeqQueryKey(matchId)) ?? 0
      let { data, error, response } = await sendAppend(event, expected)
      if (response.status === 409) {
        // staleTime: 0 forces a real network refetch regardless of
        // useLiveSeq's own Infinity — fetchQuery takes the options given
        // here, not whatever a hook elsewhere registered this key with —
        // and its result also updates the shared cache, fixing it for
        // every other reader too, not just this retry.
        const realSeq = await queryClient.fetchQuery({
          queryKey: liveSeqQueryKey(matchId),
          queryFn: () => fetchLiveSeq(matchId),
          staleTime: 0,
        })
        ;({ data, error, response } = await sendAppend(event, realSeq))
      }
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
