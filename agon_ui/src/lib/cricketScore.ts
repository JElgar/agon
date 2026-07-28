import type { components } from '@/types/api'
import { memberName } from './members'

export type CricketDelivery = components['schemas']['CricketDelivery']
export type CricketInnings = components['schemas']['CricketInnings']
export type CricketLiveState = components['schemas']['CricketLiveState']
export type CricketExtraKind = components['schemas']['CricketExtraKind']
export type CricketDismissalKind = components['schemas']['CricketDismissalKind']
export type CricketBattingEntry = components['schemas']['CricketBattingEntry']
export type CricketBowlingEntry = components['schemas']['CricketBowlingEntry']
type LiveScoreSnapshot = components['schemas']['LiveScoreSnapshot']
type Match = components['schemas']['Match']

/** Narrows a live-score snapshot to its cricket state, or `null` when there's
 *  no snapshot yet or it's for a different sport. */
export function cricketLiveState(
  snapshot: LiveScoreSnapshot | null | undefined,
): CricketLiveState | null {
  if (!snapshot || snapshot.state.sport !== 'Cricket') return null
  return snapshot.state
}

/** The innings currently being played, or `null` when the match hasn't
 *  started its first innings yet, or is between innings (see
 *  `CricketLiveState.awaiting_next_innings`). */
export function currentInnings(state: CricketLiveState): CricketInnings | null {
  if (state.awaiting_next_innings) return null
  return state.innings[state.innings.length - 1] ?? null
}

/** Whether a delivery counts toward the over (wides/no-balls don't). */
export function isLegalDelivery(d: CricketDelivery): boolean {
  return !(d.extra && (d.extra.kind === 'wide' || d.extra.kind === 'no_ball'))
}

/** Run rate so far, from the display-format `overs` (e.g. 18.2 = 18 overs +
 *  2 balls, not 18.2 decimal overs). */
export function runRate(runs: number, oversDisplay: number): number {
  const wholeOvers = Math.floor(oversDisplay)
  const balls = Math.round((oversDisplay - wholeOvers) * 10)
  const totalBalls = wholeOvers * 6 + balls
  return totalBalls > 0 ? (runs / totalBalls) * 6 : 0
}

/** This over's deliveries — everything recorded against the innings' latest
 *  over index (we assign `over`/`ball` ourselves on submit, so this is a
 *  simple filter rather than a rolling window). */
export function currentOverDeliveries(innings: CricketInnings): CricketDelivery[] {
  const latestOver = innings.deliveries.reduce((max, d) => Math.max(max, d.over), 0)
  return innings.deliveries.filter((d) => d.over === latestOver)
}

/** Short chip label for a delivery in the "this over" row, e.g. "4", "W",
 *  "·", "wd", "1lb". */
export function deliveryChipLabel(d: CricketDelivery): string {
  if (d.wicket) return 'W'
  if (d.extra) {
    const suffix: Record<CricketExtraKind, string> = {
      wide: 'wd',
      no_ball: 'nb',
      bye: 'b',
      leg_bye: 'lb',
      penalty: 'pen',
    }
    return d.extra.runs > 1 ? `${d.extra.runs}${suffix[d.extra.kind]}` : suffix[d.extra.kind]
  }
  return d.runs_off_bat === 0 ? '·' : String(d.runs_off_bat)
}

/** Whether a chip should read as a "big" event (boundary/wicket) for styling. */
export function isChipHighlighted(d: CricketDelivery): 'boundary' | 'wicket' | null {
  if (d.wicket) return 'wicket'
  if (!d.extra && (d.runs_off_bat === 4 || d.runs_off_bat === 6)) return 'boundary'
  return null
}

const DISMISSAL_LABEL: Record<CricketDismissalKind, string> = {
  bowled: 'Bowled',
  caught: 'Caught',
  leg_before_wicket: 'LBW',
  run_out: 'Run out',
  stumped: 'Stumped',
  hit_wicket: 'Hit wicket',
  retired_out: 'Retired out',
  retired_hurt: 'Retired hurt',
}

export function dismissalLabel(kind: CricketDismissalKind): string {
  return DISMISSAL_LABEL[kind]
}

/** Dismissal kinds a bowler is credited for (matches the backend's
 *  `dismissal_credited_to_bowler`) — used to auto-fill/hide the bowler field
 *  in the wicket dialog. */
export function creditsBowler(kind: CricketDismissalKind): boolean {
  return (
    kind === 'bowled' ||
    kind === 'caught' ||
    kind === 'leg_before_wicket' ||
    kind === 'stumped' ||
    kind === 'hit_wicket'
  )
}

/** A batter's card entry, or `null` if they haven't faced a ball yet (e.g. a
 *  non-striker who's yet to be on strike) — not the same as not being on the
 *  crease at all. */
export function battingEntryFor(
  innings: CricketInnings,
  playerId: string | null,
): CricketBattingEntry | null {
  if (!playerId) return null
  return innings.batting.find((b) => b.player_id === playerId) ?? null
}

export function bowlingEntryFor(
  innings: CricketInnings,
  playerId: string | null,
): CricketBowlingEntry | null {
  if (!playerId) return null
  return innings.bowling.find((b) => b.player_id === playerId) ?? null
}

/** "3.2-0-24-1" bowling figures (overs-maidens-runs-wickets), or an em dash
 *  before the bowler has sent down a delivery. */
export function bowlingFigures(entry: CricketBowlingEntry | null): string {
  if (!entry) return '—'
  return `${entry.overs.toFixed(1)}-${entry.maidens}-${entry.runs_conceded}-${entry.wickets}`
}

/** "46 (32)" — runs and balls faced, or "0 (0)" before a batter's first ball. */
export function battingLine(entry: CricketBattingEntry | null): string {
  return `${entry?.runs ?? 0} (${entry?.balls_faced ?? 0})`
}

/** Batters who can't be picked as a new arrival — already dismissed for a
 *  reason other than "retired hurt" (which lets the same batter resume). */
export function outBattersFor(innings: CricketInnings): string[] {
  return innings.batting
    .filter((b) => b.dismissal && b.dismissal.kind !== 'retired_hurt')
    .map((b) => b.player_id)
}

/** Player display name for a member id, if it's on the roster. */
export function playerNameFor(
  match: Pick<Match, 'players'>,
  playerId: string | undefined | null,
): string {
  if (!playerId) return '—'
  const player = match.players.find((p) => p.member.id === playerId)
  return player ? memberName(player.member) : '—'
}

/**
 * What's known about who's at the crease/bowling for the *next* delivery,
 * folded from the current innings' delivery log. A `null` field means the
 * scorer needs to pick someone before the next ball can be recorded — either
 * a fresh innings (nothing bowled yet), a wicket just fell (the dismissed
 * batter's slot is open), or an over just completed (a new bowler is
 * required; the same bowler can't bowl consecutive overs).
 *
 * Strike rotates automatically on an odd number of the ball's rotating runs
 * (off the bat, or byes/leg-byes — wides/no-balls don't rotate strike here)
 * and always at the end of an over — including when one slot is already
 * vacant from a wicket on that same ball (e.g. dismissed on the over's final
 * ball): the swap still applies to whichever slot the survivor occupies, so
 * the vacancy lands in the correct slot for the next ball rather than always
 * defaulting to "striker". One remaining simplification: a mid-run run-out's
 * rotation is inferred from the parity of runs completed before the
 * dismissal (as entered in the wicket dialog), not an explicit "had they
 * crossed?" flag — the two agree in the vast majority of real dismissals.
 */
export interface NextBallContext {
  strikerPlayerId: string | null
  nonStrikerPlayerId: string | null
  bowlerPlayerId: string | null
  /** 0-based over index the next delivery belongs to. */
  over: number
  /** 1-based ball number within that over. */
  ball: number
  /** The bowler who just finished an over — excluded from the next-bowler
   *  picker. `null` unless a bowler pick is actually needed. */
  previousOverBowlerPlayerId: string | null
}

export function nextBallContext(innings: CricketInnings): NextBallContext {
  let striker: string | null = null
  let nonStriker: string | null = null
  let bowler: string | null = null
  let legalInOver = 0
  let over = 0
  let previousOverBowler: string | null = null

  for (const d of innings.deliveries) {
    bowler = d.bowler_player_id
    striker = d.striker_player_id
    nonStriker = d.non_striker_player_id
    previousOverBowler = null

    if (d.wicket) {
      if (d.wicket.dismissed_player_id === striker) striker = null
      else if (d.wicket.dismissed_player_id === nonStriker) nonStriker = null
    }

    if (isLegalDelivery(d)) {
      legalInOver += 1
      const rotatingRuns =
        d.runs_off_bat + (d.extra && (d.extra.kind === 'bye' || d.extra.kind === 'leg_bye') ? d.extra.runs : 0)
      // Not guarded on both being non-null: swapping a real id with a `null`
      // vacancy still means something — it moves *which slot* is vacant, so
      // a wicket that falls alongside a rotation (odd runs, or an over
      // boundary) leaves the opening in the correct slot for the next ball.
      if (rotatingRuns % 2 === 1) {
        ;[striker, nonStriker] = [nonStriker, striker]
      }
      if (legalInOver === 6) {
        ;[striker, nonStriker] = [nonStriker, striker]
        previousOverBowler = bowler
        bowler = null
        over += 1
        legalInOver = 0
      }
    }
  }

  return {
    strikerPlayerId: striker,
    nonStrikerPlayerId: nonStriker,
    bowlerPlayerId: bowler,
    over,
    ball: legalInOver + 1,
    previousOverBowlerPlayerId: previousOverBowler,
  }
}
