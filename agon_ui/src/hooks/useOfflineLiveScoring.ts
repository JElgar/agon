/**
 * Offline-aware live-event recording: every call folds locally first (see
 * `lib/cricketFold.ts`/`lib/footballFold.ts`) and writes straight to the
 * `matchScoreQueryKey`/`liveSeqQueryKey` caches the live-scoring pages
 * already read, so the screen updates immediately regardless of
 * connectivity — then syncs to the server in the background, catching up a
 * whole local backlog in one call per ≤99-event chunk
 * (`postLiveEventBatch`, `useLiveScore.ts`) once connectivity is available.
 * A network-classified failure just leaves the event queued locally
 * (`lib/offlineQueue.ts`, IndexedDB) rather than surfacing an error; the
 * queue survives a reload, so a scorer can close the app mid-innings while
 * offline and pick back up later.
 *
 * Two sport-specific hooks (`useOfflineCricketScoring`,
 * `useOfflineFootballScoring`) wrap one shared, sport-agnostic engine that
 * only ever touches `Score` opaquely — each hook supplies the small set of
 * fold/wrap/unwrap functions the engine needs (see `SportAdapter` below).
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { components } from '@/types/api'
import { drainLiveEvents, liveSeqQueryKey, postLiveEventBatch, useUndoLiveEvent } from './useLiveScore'
import { matchScoreQueryKey } from './useMatchScore'
import { isNetworkError, useOnlineStatus } from './useOnlineStatus'
import {
  clearMatch,
  enqueuePendingEvent,
  getBaseline,
  getPendingEvents,
  popLastPendingEvent,
  removePendingThrough,
  setBaseline,
  type OfflineBaseline,
  type PendingLiveEvent,
} from '@/lib/offlineQueue'
import { applyCricketEvent, type CricketFoldFormat } from '@/lib/cricketFold'
import { applyFootballEvent, openingFootballScore } from '@/lib/footballFold'
import { cricketScoreFrom, type CricketLiveEvent, type CricketScore } from '@/lib/cricketScore'
import { footballScoreFrom, type FootballScore } from '@/lib/liveScore'

type Score = components['schemas']['Score']
type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type LiveEventInput = components['schemas']['LiveEventInput']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']

/** DynamoDB's `TransactWriteItems` cap, minus the seq-counter reservation —
 *  mirrors `agon_core::dao::live_score_ops::MAX_LIVE_EVENTS_PER_BATCH`. A
 *  local backlog bigger than this (a very long match scored entirely
 *  offline) is sent in this many chunks per sync pass, not one giant call. */
const MAX_LIVE_EVENTS_PER_BATCH = 99

/** The small set of sport-specific operations the shared engine needs,
 *  everything else about queueing/sync/undo/conflict-handling is identical
 *  either way. `TScore` is each sport's own plain score shape
 *  (`CricketScore`/`FootballScore`); the engine only ever holds `Score`
 *  (the wire union written to `matchScoreQueryKey`) at its boundary. */
interface SportAdapter<TEvent extends { kind: string }, TScore> {
  sport: 'Cricket' | 'Football'
  emptyScore: () => TScore
  applyOne: (score: TScore, occurredAt: string, event: TEvent) => TScore
  wrapScore: (score: TScore) => Score
  unwrapScore: (score: Score | null) => TScore | null
  wrapEvent: (event: TEvent) => LiveEventInput
  unwrapEvent: (input: LiveEventInput) => TEvent | null
}

/** Structural (not reference) equality over plain JSON values — every field
 *  on `LiveEventInput`/`NewLiveEventInput` is plain JSON, so this is enough
 *  to compare a fetched `LiveEvent` against a locally-queued one for the
 *  409-conflict reconciliation below. */
function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false
  const aKeys = Object.keys(a as Record<string, unknown>)
  const bKeys = Object.keys(b as Record<string, unknown>)
  if (aKeys.length !== bKeys.length) return false
  return aKeys.every((k) =>
    Object.prototype.hasOwnProperty.call(b, k) &&
    deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  )
}

/**
 * Implements the conflict-recovery recipe `AppendLiveEventsInput`'s doc
 * comment prescribes (`agon_service/src/live_score/mod.rs`): fetch the log
 * since the old tip and diff it against what was about to be sent.
 * Comparing on full `{occurred_at, event}` equality (not `event` content
 * alone) matters here — two back-to-back identical dot balls are common in
 * cricket, and `occurred_at` (a real per-tap timestamp) is what tells "my
 * own resubmitted event" apart from "a second, genuinely distinct ball that
 * happens to look the same." Returns how many leading events of `batch` are
 * already-applied (safe to drop and retry with the rest), or `null` for a
 * real, unresolved conflict.
 */
async function reconcileConflict(
  matchId: string,
  baseline: OfflineBaseline,
  batch: PendingLiveEvent[],
): Promise<{ dropped: number } | null> {
  const events = await drainLiveEvents(matchId)
  const serverNew = events
    .filter((e) => e.seq > baseline.syncedSeq)
    .sort((a, b) => a.seq - b.seq)

  let matched = 0
  while (
    matched < serverNew.length &&
    matched < batch.length &&
    serverNew[matched].occurred_at === batch[matched].input.occurred_at &&
    deepEqual(serverNew[matched].event, batch[matched].input.event)
  ) {
    matched++
  }

  if (matched === serverNew.length && matched > 0) {
    return { dropped: matched }
  }
  return null
}

function useOfflineLiveScoringEngine<TEvent extends { kind: string }, TScore>(
  matchId: string,
  sport: SportAdapter<TEvent, TScore>,
) {
  const queryClient = useQueryClient()
  const { isOnline, reportNetworkFailure, reportNetworkSuccess } = useOnlineStatus()
  const undoMutation = useUndoLiveEvent(matchId)

  const [pending, setPending] = useState<PendingLiveEvent[]>([])
  const [syncing, setSyncing] = useState(false)
  const [conflict, setConflict] = useState(false)
  const baselineRef = useRef<OfflineBaseline | null>(null)
  const syncingRef = useRef(false)
  const resyncRequestedRef = useRef(false)

  /** Rebuilds a score from a baseline plus an ordered list of not-yet-synced
   *  events — the reconciliation path (resume on mount, undo): the same
   *  per-event step as the per-tap path, just run in a loop instead of
   *  applied once. */
  const replay = useCallback(
    (baselineScore: Score | null, events: PendingLiveEvent[]): TScore => {
      let score = sport.unwrapScore(baselineScore) ?? sport.emptyScore()
      for (const p of events) {
        const event = sport.unwrapEvent(p.input.event)
        if (event) score = sport.applyOne(score, p.input.occurred_at, event)
      }
      return score
    },
    [sport],
  )

  const runSync = useCallback(async (): Promise<void> => {
    if (syncingRef.current) {
      resyncRequestedRef.current = true
      return
    }
    syncingRef.current = true
    setSyncing(true)
    try {
      for (;;) {
        const baseline = baselineRef.current
        const queue = await getPendingEvents(matchId)
        setPending(queue)
        if (!baseline || queue.length === 0) break

        const batch = queue.slice(0, MAX_LIVE_EVENTS_PER_BATCH)
        let result: Awaited<ReturnType<typeof postLiveEventBatch>>
        try {
          result = await postLiveEventBatch(matchId, {
            expected_last_seq: baseline.syncedSeq,
            events: batch.map((p) => p.input),
          })
        } catch (err) {
          if (isNetworkError(err)) {
            reportNetworkFailure()
            break
          }
          throw err
        }

        if (result.status === 409) {
          const resolved = await reconcileConflict(matchId, baseline, batch)
          if (!resolved) {
            setConflict(true)
            break
          }
          // Our own already-applied prefix lost its response — advance the
          // baseline past it (no fresh `Score` to adopt here since there
          // was no successful POST response; the next successful batch in
          // this same loop self-heals it) and retry with what's left.
          const newBaseline: OfflineBaseline = {
            matchId,
            syncedSeq: baseline.syncedSeq + resolved.dropped,
            syncedScore: baseline.syncedScore,
          }
          await removePendingThrough(matchId, newBaseline.syncedSeq)
          await setBaseline(newBaseline)
          baselineRef.current = newBaseline
          continue
        }

        if (result.status >= 400 || !result.data) {
          // A non-conflict failure on an already-validated batch shouldn't
          // normally happen — stop rather than loop forever; the backlog
          // stays queued for a later manual retry rather than being lost.
          break
        }

        const data = result.data
        const newBaseline: OfflineBaseline = { matchId, syncedSeq: data.last_seq, syncedScore: data.score }
        await removePendingThrough(matchId, data.last_seq)
        await setBaseline(newBaseline)
        baselineRef.current = newBaseline
        setConflict(false)
        reportNetworkSuccess()

        // The server's authoritative response replaces the local fold —
        // self-heals any drift between the two.
        queryClient.setQueryData(liveSeqQueryKey(matchId), data.last_seq)
        queryClient.setQueryData(matchScoreQueryKey(matchId), data.score)
        queryClient.invalidateQueries({ queryKey: ['match', matchId] })
        queryClient.invalidateQueries({ queryKey: ['feed'] })
      }
    } finally {
      setPending(await getPendingEvents(matchId))
      syncingRef.current = false
      setSyncing(false)
      if (resyncRequestedRef.current) {
        resyncRequestedRef.current = false
        void runSync()
      }
    }
  }, [matchId, queryClient, reportNetworkFailure, reportNetworkSuccess])

  // Resume: pick up any backlog left over from a previous session (the app
  // was closed while offline) and reflect it in the UI immediately.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      const baseline = (await getBaseline(matchId)) ?? null
      const queue = await getPendingEvents(matchId)
      if (cancelled) return
      baselineRef.current = baseline
      setPending(queue)
      if (queue.length > 0) {
        const score = replay(baseline?.syncedScore ?? null, queue)
        queryClient.setQueryData(matchScoreQueryKey(matchId), sport.wrapScore(score))
        queryClient.setQueryData(liveSeqQueryKey(matchId), queue[queue.length - 1].seq)
        if (isOnline) void runSync()
      }
    })()
    return () => {
      cancelled = true
    }
    // Only re-run on match change — `isOnline`/`runSync` reacting here too
    // would re-resume on every connectivity flicker, not just on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matchId])

  // Auto-sync when connectivity returns, plus a periodic backstop in case
  // the browser's `online` event doesn't fire (flaky wifi, not a real
  // online/offline transition).
  useEffect(() => {
    if (!isOnline) return
    void runSync()
    const id = setInterval(() => void runSync(), 20_000)
    return () => clearInterval(id)
  }, [isOnline, runSync])

  const recordEvent = useCallback(
    (event: TEvent) => {
      void (async () => {
        let baseline = baselineRef.current
        if (!baseline) {
          const syncedSeq = queryClient.getQueryData<number>(liveSeqQueryKey(matchId)) ?? 0
          const syncedScore = queryClient.getQueryData<Score | null>(matchScoreQueryKey(matchId)) ?? null
          baseline = { matchId, syncedSeq, syncedScore }
          await setBaseline(baseline)
          baselineRef.current = baseline
        }

        const input: NewLiveEventInput = {
          occurred_at: new Date().toISOString(),
          event: sport.wrapEvent(event),
        }
        const record = await enqueuePendingEvent(matchId, input)
        setPending((prev) => [...prev, record])

        const current = sport.unwrapScore(
          queryClient.getQueryData<Score | null>(matchScoreQueryKey(matchId)) ?? null,
        )
        const folded = sport.applyOne(current ?? sport.emptyScore(), input.occurred_at, event)
        queryClient.setQueryData(matchScoreQueryKey(matchId), sport.wrapScore(folded))
        queryClient.setQueryData(liveSeqQueryKey(matchId), record.seq)

        if (isOnline) void runSync()
      })()
    },
    [matchId, queryClient, sport, isOnline, runSync],
  )

  const undo = useCallback(() => {
    void (async () => {
      const popped = await popLastPendingEvent(matchId)
      if (popped) {
        const remaining = await getPendingEvents(matchId)
        setPending(remaining)
        const baseline = baselineRef.current
        const score = replay(baseline?.syncedScore ?? null, remaining)
        queryClient.setQueryData(matchScoreQueryKey(matchId), sport.wrapScore(score))
        queryClient.setQueryData(
          liveSeqQueryKey(matchId),
          remaining.length > 0 ? remaining[remaining.length - 1].seq : (baseline?.syncedSeq ?? 0),
        )
        return
      }

      // Nothing local to undo — fall back to the real DELETE. Only reached
      // while `canUndo` allows it (queue empty, online, not mid-sync).
      const baseline = baselineRef.current
      if (!baseline) return
      const result = await undoMutation.mutateAsync(baseline.syncedSeq)
      const newBaseline: OfflineBaseline = { matchId, syncedSeq: result.last_seq, syncedScore: result.score }
      await setBaseline(newBaseline)
      baselineRef.current = newBaseline
    })()
  }, [matchId, queryClient, sport, undoMutation, replay])

  /** Only offered once the app has a *definite* synced-and-online state,
   *  not just "queue empty" — right after reconnecting there's a real
   *  window where the queue reads empty but the first sync pass hasn't run
   *  yet, and the real `DELETE` would 400 (it only accepts the true
   *  server tip). */
  const canUndo = pending.length > 0 || (isOnline && !syncing && !conflict)

  const discardAndReloadAfterConflict = useCallback(() => {
    void (async () => {
      await clearMatch(matchId)
      baselineRef.current = null
      setPending([])
      setConflict(false)
      await queryClient.refetchQueries({ queryKey: liveSeqQueryKey(matchId) })
      await queryClient.refetchQueries({ queryKey: matchScoreQueryKey(matchId) })
    })()
  }, [matchId, queryClient])

  return {
    isOffline: !isOnline,
    pendingCount: pending.length,
    syncing,
    conflict,
    canUndo,
    recordEvent,
    undo,
    syncNow: () => void runSync(),
    discardAndReloadAfterConflict,
  }
}

const cricketAdapter = (format: CricketFoldFormat): SportAdapter<CricketLiveEvent, CricketScore> => ({
  sport: 'Cricket',
  emptyScore: () => ({ innings: [], players: {} }),
  applyOne: (score, _occurredAt, event) => applyCricketEvent(score, event, format),
  wrapScore: (score): Score => ({ type: 'Cricket', ...score }),
  unwrapScore: (score) => cricketScoreFrom(score),
  wrapEvent: (event): LiveEventInput => ({ sport: 'Cricket', ...event }) as LiveEventInput,
  unwrapEvent: (input) =>
    input.sport === 'Cricket' ? (input as unknown as CricketLiveEvent) : null,
})

const footballAdapter: SportAdapter<FootballLiveEvent, FootballScore> = {
  sport: 'Football',
  emptyScore: () => openingFootballScore(),
  applyOne: (score, occurredAt, event) => applyFootballEvent(score, occurredAt, event),
  wrapScore: (score) => score,
  unwrapScore: (score) => footballScoreFrom(score),
  wrapEvent: (event): LiveEventInput => ({ sport: 'Football', ...event }) as LiveEventInput,
  unwrapEvent: (input) =>
    input.sport === 'Football' ? (input as unknown as FootballLiveEvent) : null,
}

/** Offline-aware cricket live-scoring: replaces `useAppendCricketEvent` in
 *  `CricketLiveScoringPage.tsx`. `format` only needs the fields
 *  `applyCricketEvent` actually reads (see `CricketFoldFormat`). */
export function useOfflineCricketScoring(matchId: string, format: CricketFoldFormat) {
  return useOfflineLiveScoringEngine(matchId, cricketAdapter(format))
}

/** Offline-aware football live-scoring: replaces `useAppendFootballEvent`
 *  in the football half of `LiveScoringPage.tsx`. */
export function useOfflineFootballScoring(matchId: string) {
  return useOfflineLiveScoringEngine(matchId, footballAdapter)
}
