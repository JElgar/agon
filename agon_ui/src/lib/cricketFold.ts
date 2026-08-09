/**
 * The cricket live-scoring fold, run client-side against events an offline
 * device has recorded locally but not yet synced (see
 * `useOfflineLiveScoring.ts`). A literal, hand-ported TS mirror of
 * `apply_delivery`/`CricketScore::apply_event`/`CricketScore::from_events`
 * in `agon_service/src/live_score/cricket.rs` — kept in sync by hand, the
 * same promise `NextBallContext`'s doc comment (on the backend, in
 * `detailed_score::cricket`) already makes. When that Rust function
 * changes, mirror the change here too; the two are meant to be read
 * side-by-side, not independently re-derived, so this file follows the same
 * order of operations and variable names as the Rust source throughout.
 *
 * `applyCricketEvent` is the primary per-tap path — an O(1) step applied
 * once to the last-known local state, exactly like the backend's own fast
 * path (`apply_live_events_incrementally` in `agon_service/src/main.rs`).
 * `foldCricketEvents` (full replay) exists only for reconciliation —
 * rebuilding local state from a synced baseline plus the pending queue on
 * resume, or after a sync conflict — mirroring the backend's slow
 * full-derive fallback. It's built from the exact same per-event step, just
 * called in a loop, so the two paths can't disagree.
 */
import type { components } from '@/types/api'
import {
  creditsBowler,
  isLegalDelivery,
  runsChargedToBowler,
  ballsToOvers,
  type CricketDelivery,
  type CricketLiveEvent,
  type CricketScore,
  type CricketScoreInnings,
  type NextBallContext,
} from './cricketScore'
import type { CricketFormat } from './matchFormat'

type CricketBattingEntry = components['schemas']['CricketBattingEntry']
type CricketBowlingEntry = components['schemas']['CricketBowlingEntry']

/** The subset of `CricketFormat` the fold actually needs — same shape as
 *  `runProgression`'s format param in `cricketScore.ts`. */
export type CricketFoldFormat = Pick<
  CricketFormat,
  'wide_is_extra_ball' | 'no_ball_is_extra_ball' | 'balls_per_over'
>

/** How many balls back `CricketScore.recent_deliveries` keeps — mirrors the
 *  backend's `RECENT_DELIVERIES_LIMIT` (`detailed_score::cricket`). Must
 *  match: this is the window the live-scoring UI reads for "this over," and
 *  a synced score's window is server-authoritative once sync lands, so a
 *  mismatched limit would only show up as a visible reshuffle at that
 *  moment. */
const RECENT_DELIVERIES_LIMIT = 18

/** The state before any delivery has been recorded in an innings — mirrors
 *  the backend's `NextBallContext::opening`. */
export function openingContext(): NextBallContext {
  return {
    striker_player_id: undefined,
    non_striker_player_id: undefined,
    bowler_player_id: undefined,
    over: 0,
    ball: 1,
    previous_over_bowler_player_id: undefined,
    runs_conceded_this_over: 0,
  }
}

/** A freshly-started innings, with its optional card fields populated
 *  empty rather than left `undefined` — mirrors the backend's
 *  `CricketScoreInnings::opening`: there's a live innings behind this from
 *  the moment it exists, even before its first ball. */
function openingInnings(battingSideId: string, bowlingSideId: string): CricketScoreInnings {
  return {
    batting_side_id: battingSideId,
    bowling_side_id: bowlingSideId,
    runs: 0,
    wickets: 0,
    overs: { overs: 0, balls: 0 },
    declared: false,
    batting: [],
    bowling: [],
    fall_of_wickets: [],
    extras: { byes: 0, leg_byes: 0, wides: 0, no_balls: 0, penalty: 0 },
  }
}

/** Get-or-insert a batting-card entry by player id — mirrors the backend's
 *  `batter()` helper. */
function batterEntry(batting: CricketBattingEntry[], playerId: string): CricketBattingEntry {
  const existing = batting.find((b) => b.player_id === playerId)
  if (existing) return existing
  const entry: CricketBattingEntry = {
    player_id: playerId,
    runs: 0,
    balls_faced: 0,
    fours: 0,
    sixes: 0,
    batting_position: batting.length + 1,
  }
  batting.push(entry)
  return entry
}

/** Get-or-insert a bowling-card entry by player id — mirrors the backend's
 *  `bowler()` helper. */
function bowlerEntry(bowling: CricketBowlingEntry[], playerId: string): CricketBowlingEntry {
  const existing = bowling.find((b) => b.player_id === playerId)
  if (existing) return existing
  const entry: CricketBowlingEntry = {
    player_id: playerId,
    overs: { overs: 0, balls: 0 },
    maidens: 0,
    runs_conceded: 0,
    wickets: 0,
    wides: 0,
    no_balls: 0,
  }
  bowling.push(entry)
  return entry
}

/**
 * Folds one delivery into an innings already in progress — updates totals,
 * the batting/bowling cards, extras, and fall-of-wickets in place — and
 * returns the next-ball context that follows it. Mutates `innings` in
 * place, exactly like the Rust `&mut innings`; callers own cloning first
 * (see `applyCricketEvent`).
 *
 * Strike rotates on an odd number of the ball's rotating runs (off the bat,
 * or byes/leg-byes — wides/no-balls don't rotate strike) and always at the
 * end of an over — including when one slot is already vacant from a wicket
 * on that same ball (e.g. dismissed on the over's final ball): the swap
 * still applies to whichever slot the survivor occupies, so the vacancy
 * lands in the correct slot for the next ball rather than always defaulting
 * to "striker." That's why this does a literal two-variable swap at each
 * point rather than special-casing "was there a wicket this ball" — same
 * as the Rust source.
 */
export function applyDelivery(
  innings: CricketScoreInnings,
  context: NextBallContext,
  d: CricketDelivery,
  format: CricketFoldFormat,
): NextBallContext {
  const legal = isLegalDelivery(d, format)
  const charged = runsChargedToBowler(d)
  const extraRuns = d.extra?.runs ?? 0

  innings.runs += d.runs_off_bat + extraRuns
  if (legal) {
    const legalBalls = innings.overs.overs * format.balls_per_over + innings.overs.balls + 1
    innings.overs = ballsToOvers(legalBalls, format.balls_per_over)
  }

  // Batting: striker is credited runs off the bat and faces legal balls and
  // no-balls (but not wides).
  {
    const batting = (innings.batting ??= [])
    const b = batterEntry(batting, d.striker_player_id)
    b.runs += d.runs_off_bat
    const facedBall = d.extra?.kind !== 'wide'
    if (facedBall) b.balls_faced += 1
    if (d.runs_off_bat === 4) b.fours += 1
    else if (d.runs_off_bat === 6) b.sixes += 1
  }

  // Bowling figures.
  {
    const bowling = (innings.bowling ??= [])
    const bw = bowlerEntry(bowling, d.bowler_player_id)
    bw.runs_conceded += charged
    if (d.extra?.kind === 'wide') bw.wides += 1
    else if (d.extra?.kind === 'no_ball') bw.no_balls += 1
    if (legal) {
      const legalBalls = bw.overs.overs * format.balls_per_over + bw.overs.balls + 1
      bw.overs = ballsToOvers(legalBalls, format.balls_per_over)
    }
  }

  // Extras breakdown.
  if (d.extra) {
    const extras = (innings.extras ??= { byes: 0, leg_byes: 0, wides: 0, no_balls: 0, penalty: 0 })
    switch (d.extra.kind) {
      case 'bye':
        extras.byes += d.extra.runs
        break
      case 'leg_bye':
        extras.leg_byes += d.extra.runs
        break
      case 'wide':
        extras.wides += d.extra.runs
        break
      case 'no_ball':
        extras.no_balls += d.extra.runs
        break
      case 'penalty':
        extras.penalty += d.extra.runs
        break
    }
  }

  // Wicket.
  if (d.wicket) {
    innings.wickets += 1
    const fallOfWickets = (innings.fall_of_wickets ??= [])
    fallOfWickets.push({
      wicket: innings.wickets,
      runs: innings.runs,
      player_id: d.wicket.dismissed_player_id,
      overs: innings.overs,
    })
    if (creditsBowler(d.wicket.kind)) {
      bowlerEntry(innings.bowling ??= [], d.bowler_player_id).wickets += 1
    }
    const b = batterEntry(innings.batting ??= [], d.wicket.dismissed_player_id)
    b.dismissal = {
      kind: d.wicket.kind,
      bowler_player_id: d.wicket.bowler_player_id,
      fielder_player_id: d.wicket.fielder_player_id,
    }
  }

  // Next-ball context, folded from the previous one.
  let striker: string | undefined = d.striker_player_id
  let nonStriker: string | undefined = d.non_striker_player_id
  let bowlerId: string | undefined = d.bowler_player_id
  let previousOverBowler: string | undefined = undefined
  let legalInOver = Math.max(context.ball - 1, 0)
  let over = context.over
  let runsConcededThisOver = context.runs_conceded_this_over + charged

  if (d.wicket) {
    if (striker === d.wicket.dismissed_player_id) {
      striker = undefined
    } else if (nonStriker === d.wicket.dismissed_player_id) {
      nonStriker = undefined
    }
  }

  if (legal) {
    legalInOver += 1
    const rotatingRuns =
      d.runs_off_bat + (d.extra?.kind === 'bye' || d.extra?.kind === 'leg_bye' ? extraRuns : 0)
    if (rotatingRuns % 2 === 1) {
      ;[striker, nonStriker] = [nonStriker, striker]
    }
    if (legalInOver === format.balls_per_over) {
      ;[striker, nonStriker] = [nonStriker, striker]
      if (runsConcededThisOver === 0 && bowlerId) {
        bowlerEntry(innings.bowling ??= [], bowlerId).maidens += 1
      }
      previousOverBowler = bowlerId
      bowlerId = undefined
      over += 1
      legalInOver = 0
      runsConcededThisOver = 0
    }
  }

  return {
    striker_player_id: striker,
    non_striker_player_id: nonStriker,
    bowler_player_id: bowlerId,
    over,
    ball: legalInOver + 1,
    previous_over_bowler_player_id: previousOverBowler,
    runs_conceded_this_over: runsConcededThisOver,
  }
}

/** Deep-clones a score before mutating — the Rust original mutates `&mut
 *  self` in place; this returns a fresh top-level object so a React Query
 *  cache write (`setQueryData`) sees a new reference. Every field on
 *  `CricketScore` is plain JSON (no `Date`/`Map`/`Set`), so a JSON
 *  round-trip is a safe, dependency-free clone. */
function cloneScore(score: CricketScore): CricketScore {
  return JSON.parse(JSON.stringify(score)) as CricketScore
}

/** Folds one new event into a cricket score, returning the updated score —
 *  mirrors the backend's `CricketScore::apply_event`. The primary per-tap
 *  path: apply one step to the last-known score, don't replay history. */
export function applyCricketEvent(
  score: CricketScore,
  event: CricketLiveEvent,
  format: CricketFoldFormat,
): CricketScore {
  const next = cloneScore(score)

  switch (event.kind) {
    case 'InningsStart': {
      next.innings.push(openingInnings(event.batting_side_id, event.bowling_side_id))
      next.recent_deliveries = []
      next.next_ball_context = openingContext()
      next.awaiting_next_innings = false
      break
    }
    case 'Delivery': {
      const current = next.innings[next.innings.length - 1]
      if (!current) break // A delivery with no open innings is malformed input.
      const context = next.next_ball_context ?? openingContext()
      const delivery: CricketDelivery = event
      next.next_ball_context = applyDelivery(current, context, delivery, format)

      const deliveries = (next.recent_deliveries ??= [])
      deliveries.push(delivery)
      if (deliveries.length > RECENT_DELIVERIES_LIMIT) deliveries.shift()
      break
    }
    case 'Retire': {
      const current = next.innings[next.innings.length - 1]
      if (!current) break
      const entry = (current.batting ?? []).find((b) => b.player_id === event.batter_player_id)
      // Retired without ever facing a ball — no batting-card row exists yet
      // to annotate. Rare enough not to synthesize one.
      if (!entry) break
      // A later real dismissal (from a delivery) always takes precedence
      // over an earlier retirement note — and by the time events are
      // processed in order, that's already reflected here.
      if (entry.dismissal) break
      entry.dismissal = {
        kind: event.retired_out ? 'retired_out' : 'retired_hurt',
      }
      if (event.retired_out) {
        current.wickets += 1
        const fallOfWickets = (current.fall_of_wickets ??= [])
        fallOfWickets.push({
          wicket: current.wickets,
          runs: current.runs,
          player_id: event.batter_player_id,
          overs: current.overs,
        })
      }
      if (next.next_ball_context) {
        if (next.next_ball_context.striker_player_id === event.batter_player_id) {
          next.next_ball_context.striker_player_id = undefined
        } else if (next.next_ball_context.non_striker_player_id === event.batter_player_id) {
          next.next_ball_context.non_striker_player_id = undefined
        }
      }
      break
    }
    case 'InningsEnd': {
      const current = next.innings[next.innings.length - 1]
      if (current) current.declared = event.reason === 'declared'
      next.recent_deliveries = undefined
      next.next_ball_context = undefined
      next.awaiting_next_innings = true
      break
    }
  }

  return next
}

/** Folds a whole ordered event list into a `CricketScore` from scratch —
 *  mirrors the backend's `CricketScore::from_events`. The reconciliation
 *  path only: rebuilding local state from a synced baseline plus the
 *  pending queue on resume, or after a sync conflict. Just
 *  `applyCricketEvent` run once per event in order — the same fold as the
 *  per-tap path, so the two can't disagree. */
export function foldCricketEvents(events: CricketLiveEvent[], format: CricketFoldFormat): CricketScore {
  let score: CricketScore = { innings: [], players: {} }
  for (const event of events) {
    score = applyCricketEvent(score, event, format)
  }
  return score
}
