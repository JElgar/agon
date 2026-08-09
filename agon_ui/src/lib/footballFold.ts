/**
 * The football live-scoring fold, run client-side against events an offline
 * device has recorded locally but not yet synced (see
 * `useOfflineLiveScoring.ts`). A literal TS mirror of
 * `FootballScore::apply_event`/`FootballScore::from_events` in
 * `agon_service/src/live_score/football.rs` — much simpler than cricket's,
 * since a football event never depends on cross-event state (no
 * next-ball-context equivalent): each event just appends to a list and bumps
 * a tally. Same primary-path/reconciliation-path split as `cricketFold.ts`.
 */
import type { components } from '@/types/api'
import type { FootballScore } from './liveScore'

export type FootballLiveEvent = components['schemas']['FootballLiveEvent']

/** A score with every optional collection populated empty rather than left
 *  `undefined` — mirrors the backend's `FootballScore::from_events` opening
 *  state ("every optional field is populated... by the time this is called
 *  from a live-scoring path"). */
export function openingFootballScore(): FootballScore {
  return {
    type: 'Football',
    score: {},
    goals: [],
    cards: [],
    substitutions: [],
    period: undefined,
    period_times: {},
    penalty_shootout: [],
    penalty_shootout_score: {},
    players: {},
  }
}

/** Deep-clones a score before mutating, same reasoning as `cricketFold.ts`'s
 *  `cloneScore` — every field here is plain JSON. */
function cloneScore(score: FootballScore): FootballScore {
  return JSON.parse(JSON.stringify(score)) as FootballScore
}

/** Folds one new event into a football score, returning the updated score —
 *  mirrors the backend's `FootballScore::apply_event`. `occurredAt` (not a
 *  "recorded at" server timestamp) is threaded through separately from
 *  `event` so a period marker's timestamp reflects when the half actually
 *  started/ended, not when it was synced — same reasoning as the backend. */
export function applyFootballEvent(
  score: FootballScore,
  occurredAt: string,
  event: FootballLiveEvent,
): FootballScore {
  const next = cloneScore(score)

  switch (event.kind) {
    case 'Goal': {
      next.score[event.side_id] = (next.score[event.side_id] ?? 0) + 1
      const goals = (next.goals ??= [])
      goals.push(event)
      break
    }
    case 'Card': {
      const cards = (next.cards ??= [])
      cards.push(event)
      break
    }
    case 'Substitution': {
      const substitutions = (next.substitutions ??= [])
      substitutions.push(event)
      break
    }
    case 'Period': {
      const periodTimes = (next.period_times ??= {})
      periodTimes[event.period] = occurredAt
      next.period = event.period
      break
    }
    case 'PenaltyShootoutKick': {
      if (event.scored) {
        const shootoutScore = (next.penalty_shootout_score ??= {})
        shootoutScore[event.side_id] = (shootoutScore[event.side_id] ?? 0) + 1
      }
      const shootout = (next.penalty_shootout ??= [])
      shootout.push(event)
      break
    }
  }

  return next
}

/** Folds a whole ordered event list into a `FootballScore` from scratch —
 *  mirrors the backend's `FootballScore::from_events`. The reconciliation
 *  path only (resume, or after a sync conflict) — the same per-event step
 *  as `applyFootballEvent`, just run in a loop. */
export function foldFootballEvents(
  events: { occurred_at: string; event: FootballLiveEvent }[],
): FootballScore {
  let score = openingFootballScore()
  for (const { occurred_at, event } of events) {
    score = applyFootballEvent(score, occurred_at, event)
  }
  return score
}


// Re-exported only so callers of this module don't also need a separate
// import from `liveScore.ts` just for the type name.
export type { FootballScore }
