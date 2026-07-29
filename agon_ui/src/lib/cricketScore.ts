import type { components } from '@/types/api'
import { memberName } from './members'
import type { CricketFormat } from './matchFormat'

export type CricketDelivery = components['schemas']['CricketDelivery']
export type CricketInnings = components['schemas']['CricketInnings']
export type CricketLiveState = components['schemas']['CricketLiveState']
export type CricketExtraKind = components['schemas']['CricketExtraKind']
export type CricketDismissalKind = components['schemas']['CricketDismissalKind']
export type CricketBattingEntry = components['schemas']['CricketBattingEntry']
export type CricketBowlingEntry = components['schemas']['CricketBowlingEntry']
export type CricketScore = components['schemas']['CricketScore']
export type CricketScoreInnings = components['schemas']['CricketScoreInnings']
export type Overs = components['schemas']['Overs']
type LiveScoreSnapshot = components['schemas']['LiveScoreSnapshot']
type DetailedScore = components['schemas']['DetailedScore']
type Score = components['schemas']['Score']
type Match = components['schemas']['Match']

/** Narrows a live-score snapshot to its cricket state, or `null` when there's
 *  no snapshot yet or it's for a different sport. */
export function cricketLiveState(
  snapshot: LiveScoreSnapshot | null | undefined,
): CricketLiveState | null {
  if (!snapshot || snapshot.state.sport !== 'Cricket') return null
  return snapshot.state
}

/** Narrows a match score to its cricket variant, or `null` when there's no
 *  score yet or it's the plain totals-only shape a manually-logged (not
 *  live-scored) cricket result degrades to (see `Score::Simple`'s doc
 *  comment on the backend). */
export function cricketScoreFrom(score: Score | null | undefined): CricketScore | null {
  if (!score || score.type !== 'Cricket') return null
  return score
}

/** Narrows a match's detailed score to its cricket innings, or `null` when
 *  there's none yet or it's for a different sport. */
export function cricketDetail(
  detail: DetailedScore | null | undefined,
): CricketInnings[] | null {
  if (!detail || detail.type !== 'Cricket') return null
  return detail.innings
}

/** The minimal per-innings shape match-aggregate math and the state-of-game
 *  sentence need — both `CricketLiveState.innings` (rich, ball-by-ball
 *  derived, while a match is actually being scored) and a confirmed
 *  `CricketScore.innings` (the persisted summary, once it's over) satisfy
 *  this structurally, so the same functions work on either. */
interface CricketInningsTotal {
  batting_side_id: string
  bowling_side_id: string
  runs: number
  wickets: number
}

/** Ditto, for the match-level progress shape: a live snapshot's own state, or
 *  a finished match's confirmed score reframed as one (see
 *  `cricketProgressFromScore`) — it has no notion of "in progress", so it's
 *  always `awaiting_next_innings: true`. */
export interface CricketMatchProgress {
  innings: CricketInningsTotal[]
  awaiting_next_innings: boolean
}

/** Reframes a completed match's `CricketScore` as a `CricketMatchProgress` —
 *  there's no currently-open innings once the score is confirmed, so this is
 *  always "awaiting the next innings" (there won't be one). */
export function cricketProgressFromScore(score: CricketScore): CricketMatchProgress {
  return { innings: score.innings, awaiting_next_innings: true }
}

/** The innings currently being played, or `null` when the match hasn't
 *  started its first innings yet, or is between innings (see
 *  `CricketLiveState.awaiting_next_innings`). */
export function currentInnings(state: CricketLiveState): CricketInnings | null {
  if (state.awaiting_next_innings) return null
  return state.innings[state.innings.length - 1] ?? null
}

/** Whether a delivery counts toward the over. Wides/no-balls don't under the
 *  standard rules, but a match format can configure either one to just count
 *  as a legal ball instead (see `CricketFormat.wide_is_extra_ball` /
 *  `no_ball_is_extra_ball`). */
export function isLegalDelivery(
  d: CricketDelivery,
  format: Pick<CricketFormat, 'wide_is_extra_ball' | 'no_ball_is_extra_ball'>,
): boolean {
  if (d.extra?.kind === 'wide') return !format.wide_is_extra_ball
  if (d.extra?.kind === 'no_ball') return !format.no_ball_is_extra_ball
  return true
}

/** "19.4" — the conventional overs-and-balls display, e.g. 4 balls into the
 *  20th over. */
export function formatOvers(overs: Overs): string {
  return `${overs.overs}.${overs.balls}`
}

/** Run rate so far, from an overs-and-balls count and the match's configured
 *  over length (6 for almost everything, 5 for The Hundred). */
export function runRate(runs: number, overs: Overs, ballsPerOver: number): number {
  const totalBalls = overs.overs * ballsPerOver + overs.balls
  return totalBalls > 0 ? (runs / totalBalls) * ballsPerOver : 0
}

/** One point on a run-progression graph: the cumulative state immediately
 *  after a delivery. `overDecimal` is a continuous x-axis value (whole overs
 *  plus a fraction of the current one, scaled by the format's
 *  `balls_per_over`) — a graph needs a single number to plot against, unlike
 *  `overs`'s exact whole-overs-plus-balls count. */
export interface RunProgressionPoint {
  overs: Overs
  overDecimal: number
  runs: number
  wickets: number
  /** True if this delivery was a wicket — for marking the point distinctly. */
  isWicket: boolean
}

/** Cumulative runs/wickets after each delivery, in order — the data behind a
 *  run-rate/worm graph. Only legal deliveries (per the match's format —
 *  wides/no-balls may or may not count, see `isLegalDelivery`) advance the
 *  overs count; extras still add to `runs` at the same over position as the
 *  previous legal ball. */
export function runProgression(
  deliveries: CricketDelivery[],
  format: Pick<CricketFormat, 'wide_is_extra_ball' | 'no_ball_is_extra_ball' | 'balls_per_over'>,
): RunProgressionPoint[] {
  let runs = 0
  let wickets = 0
  let legalBalls = 0
  return deliveries.map((d) => {
    const extraRuns = d.extra?.runs ?? 0
    runs += d.runs_off_bat + extraRuns
    if (isLegalDelivery(d, format)) legalBalls += 1
    if (d.wicket) wickets += 1
    const overs = Math.floor(legalBalls / format.balls_per_over)
    const balls = legalBalls % format.balls_per_over
    return {
      overs: { overs, balls },
      overDecimal: overs + balls / format.balls_per_over,
      runs,
      wickets,
      isWicket: !!d.wicket,
    }
  })
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
  return `${formatOvers(entry.overs)}-${entry.maidens}-${entry.runs_conceded}-${entry.wickets}`
}

/** "46 (32)" — runs and balls faced, or "0 (0)" before a batter's first ball. */
export function battingLine(entry: CricketBattingEntry | null): string {
  return `${entry?.runs ?? 0} (${entry?.balls_faced ?? 0})`
}

/** Total runs scored by each side across every completed innings so far —
 *  the input to a final result once the match is done (sums across both
 *  innings for a two-innings-per-side format, not just the latest one). */
export function matchTotalsBySide(state: CricketMatchProgress): Record<string, number> {
  const totals: Record<string, number> = {}
  for (const innings of state.innings) {
    totals[innings.batting_side_id] = (totals[innings.batting_side_id] ?? 0) + innings.runs
  }
  return totals
}

/**
 * A one-line summary of where the match stands:
 *   - Mid-match, once the bowling side has a score on the board to compare
 *     against: who's ahead on the match aggregate ("England lead by 30 runs").
 *   - In the match's final innings: the run target for the side chasing
 *     ("England need 200 to win").
 *   - Once every innings the format allows has been played: the result
 *     ("England won by 4 wickets" / "Australia won by 100 runs" / "Match
 *     tied").
 * `null` when none of these apply yet — e.g. the match's very first innings,
 * with nothing yet to lead or chase.
 *
 * The wickets margin is "wickets not yet lost" against the batting side's
 * own roster size (players registered on that side, minus one — the last
 * batter has no partner left to bat with), falling back to the standard 10
 * if the roster looks too small to make sense of (e.g. not fully set up).
 */
export function cricketStateDescription(
  match: Pick<Match, 'sides' | 'players'>,
  state: CricketMatchProgress,
  format: Pick<CricketFormat, 'innings_per_side'>,
): string | null {
  if (state.innings.length === 0) return null
  const quota = match.sides.length * format.innings_per_side
  const totals = matchTotalsBySide(state)

  const open = state.awaiting_next_innings ? null : state.innings[state.innings.length - 1]
  if (open) {
    const battingTotal = totals[open.batting_side_id] ?? 0
    const bowlingTotal = totals[open.bowling_side_id] ?? 0

    // Only the match's final innings has a fixed target — every earlier
    // innings (including any the batting side has already had, in a
    // multi-innings format) is done, so what's left to chase is known.
    if (state.innings.length === quota) {
      const runsNeeded = bowlingTotal + 1 - battingTotal
      if (runsNeeded <= 0) return null
      return `${sideNameFor(match, open.batting_side_id)} need ${runsNeeded} to win`
    }

    // Not the decider yet — show who's ahead on the match aggregate, once
    // the bowling side actually has a score on the board to compare against
    // (there's nothing to lead in the match's very first innings).
    const bowlingHasBatted = state.innings
      .slice(0, -1)
      .some((i) => i.batting_side_id === open.bowling_side_id)
    if (!bowlingHasBatted) return null
    const diff = battingTotal - bowlingTotal
    if (diff === 0) return 'Scores level'
    const leadingSideId = diff > 0 ? open.batting_side_id : open.bowling_side_id
    const margin = Math.abs(diff)
    return `${sideNameFor(match, leadingSideId)} lead by ${margin} run${margin === 1 ? '' : 's'}`
  }

  if (!state.awaiting_next_innings || state.innings.length < quota) return null
  const last = state.innings[state.innings.length - 1]
  const battingTotal = totals[last.batting_side_id] ?? 0
  const bowlingTotal = totals[last.bowling_side_id] ?? 0
  if (battingTotal === bowlingTotal) return 'Match tied'
  if (battingTotal > bowlingTotal) {
    const rosterSize = match.players.filter((p) => p.side_id === last.batting_side_id).length
    const maxWickets = rosterSize > 1 ? rosterSize - 1 : 10
    const remaining = Math.max(maxWickets - last.wickets, 0)
    return `${sideNameFor(match, last.batting_side_id)} won by ${remaining} wicket${remaining === 1 ? '' : 's'}`
  }
  const margin = bowlingTotal - battingTotal
  return `${sideNameFor(match, last.bowling_side_id)} won by ${margin} run${margin === 1 ? '' : 's'}`
}

/** Batters who can't be picked as a new arrival — already dismissed for a
 *  reason other than "retired hurt" (which lets the same batter resume). */
export function outBattersFor(innings: CricketInnings): string[] {
  return innings.batting
    .filter((b) => b.dismissal && b.dismissal.kind !== 'retired_hurt')
    .map((b) => b.player_id)
}

/** Display name for a side id: its name, or a neutral fallback. */
export function sideNameFor(match: Pick<Match, 'sides'>, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
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

export function nextBallContext(
  innings: CricketInnings,
  format: Pick<CricketFormat, 'balls_per_over' | 'wide_is_extra_ball' | 'no_ball_is_extra_ball'>,
): NextBallContext {
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

    if (isLegalDelivery(d, format)) {
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
      if (legalInOver === format.balls_per_over) {
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
