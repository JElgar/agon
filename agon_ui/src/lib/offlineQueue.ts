/**
 * Local, per-match durable queue for offline live-scoring: the last state
 * actually confirmed by the server (`baseline`) plus the ordered backlog of
 * events recorded locally since then (`pending_events`). Backs
 * `useOfflineLiveScoring.ts`. IndexedDB (via `idb`, a small promise wrapper)
 * rather than `localStorage`: the sync/drain path needs to remove a
 * *prefix* of the pending list and advance the baseline together, and
 * `localStorage` would mean deserializing/re-serializing one flat JSON blob
 * per match on every write — synchronously, on the main thread, possibly
 * mid-tap. IndexedDB gives real keyed, per-match records instead.
 *
 * Survives a reload — a PWA can be closed while offline and reopened later,
 * still offline, without losing what was recorded (see `vite.config.ts`'s
 * `VitePWA` setup, which already makes the app shell itself reloadable
 * offline; this is the data-side counterpart).
 */
import { openDB, type DBSchema, type IDBPDatabase } from 'idb'
import type { components } from '@/types/api'

type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type Score = components['schemas']['Score']

/** The last state actually confirmed by the server for a match — what the
 *  next sync batch's `expected_last_seq` is built from, and what a
 *  reconciliation replay (`foldCricketEvents`/`foldFootballEvents`) starts
 *  from. `syncedScore: null` means the match has no server-recorded score
 *  yet (scoring started offline, from scratch). */
export interface OfflineBaseline {
  matchId: string
  syncedSeq: number
  syncedScore: Score | null
}

/** One event recorded locally but not yet confirmed by the server. `seq` is
 *  the seq it *would* get if synced right now (`baseline.syncedSeq` plus its
 *  position in the backlog) — assigning it up front, the same way the
 *  server would, means the queue's own ordering is just "sort by seq," and
 *  undo is just "remove the highest seq for this match." */
export interface PendingLiveEvent {
  matchId: string
  seq: number
  input: NewLiveEventInput
}

interface OfflineQueueSchema extends DBSchema {
  baseline: {
    key: string
    value: OfflineBaseline
  }
  pending_events: {
    key: [string, number]
    value: PendingLiveEvent
    indexes: { by_match: string }
  }
}

const DB_NAME = 'agon-offline-live-scoring'
const DB_VERSION = 1

let dbPromise: Promise<IDBPDatabase<OfflineQueueSchema>> | undefined

function db(): Promise<IDBPDatabase<OfflineQueueSchema>> {
  dbPromise ??= openDB<OfflineQueueSchema>(DB_NAME, DB_VERSION, {
    upgrade(database) {
      database.createObjectStore('baseline', { keyPath: 'matchId' })
      const pending = database.createObjectStore('pending_events', { keyPath: ['matchId', 'seq'] })
      pending.createIndex('by_match', 'matchId')
    },
  })
  return dbPromise
}

export async function getBaseline(matchId: string): Promise<OfflineBaseline | undefined> {
  return (await db()).get('baseline', matchId)
}

export async function setBaseline(baseline: OfflineBaseline): Promise<void> {
  await (await db()).put('baseline', baseline)
}

/** Every pending event for a match, oldest first. Small enough (a match's
 *  offline backlog realistically tops out in the hundreds before a sync
 *  window comes around) to just read and sort in memory rather than lean on
 *  cursor ordering. */
export async function getPendingEvents(matchId: string): Promise<PendingLiveEvent[]> {
  const events = await (await db()).getAllFromIndex('pending_events', 'by_match', matchId)
  return events.sort((a, b) => a.seq - b.seq)
}

/** Appends one event to the match's local backlog, assigning it the next
 *  seq after whatever's already queued (or the baseline tip if the queue is
 *  empty). Returns the stored record, so a caller can fold it into the
 *  optimistic UI state immediately without a second read. */
export async function enqueuePendingEvent(
  matchId: string,
  input: NewLiveEventInput,
): Promise<PendingLiveEvent> {
  const database = await db()
  const tx = database.transaction(['baseline', 'pending_events'], 'readwrite')
  const baseline = await tx.objectStore('baseline').get(matchId)
  const existing = await tx.objectStore('pending_events').index('by_match').getAll(matchId)
  const lastSeq = existing.reduce((max, e) => Math.max(max, e.seq), baseline?.syncedSeq ?? 0)
  const record: PendingLiveEvent = { matchId, seq: lastSeq + 1, input }
  await tx.objectStore('pending_events').put(record)
  await tx.done
  return record
}

/** Removes and returns the highest-seq pending event for a match — the
 *  local half of undo: while anything's unsynced, undoing just pops this,
 *  no network call. `undefined` if the queue is already empty (nothing
 *  local to undo; the caller falls back to the real `DELETE` endpoint for
 *  an already-synced event). */
export async function popLastPendingEvent(matchId: string): Promise<PendingLiveEvent | undefined> {
  const events = await getPendingEvents(matchId)
  const last = events[events.length - 1]
  if (!last) return undefined
  await (await db()).delete('pending_events', [matchId, last.seq])
  return last
}

/** Removes every pending event up to and including `seq` — called after a
 *  sync batch lands successfully for exactly the events it drained. */
export async function removePendingThrough(matchId: string, seq: number): Promise<void> {
  const database = await db()
  const tx = database.transaction('pending_events', 'readwrite')
  const toRemove = await tx.store.index('by_match').getAll(matchId)
  await Promise.all(
    toRemove.filter((e) => e.seq <= seq).map((e) => tx.store.delete([matchId, e.seq])),
  )
  await tx.done
}

/** Clears everything local for a match — baseline and backlog alike. Used
 *  when a sync conflict is resolved by discarding the local backlog and
 *  adopting the server's version outright (see `useOfflineLiveScoring.ts`),
 *  and once a match is done with live scoring entirely. */
export async function clearMatch(matchId: string): Promise<void> {
  const database = await db()
  const tx = database.transaction(['baseline', 'pending_events'], 'readwrite')
  await tx.objectStore('baseline').delete(matchId)
  const toRemove = await tx.objectStore('pending_events').index('by_match').getAll(matchId)
  await Promise.all(toRemove.map((e) => tx.objectStore('pending_events').delete([matchId, e.seq])))
  await tx.done
}
