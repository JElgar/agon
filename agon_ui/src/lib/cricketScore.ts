import type { components } from '@/types/api'
import { memberName } from './members'
import type { CricketFormat } from './matchFormat'

export type CricketDelivery = components['schemas']['CricketDelivery']
export type CricketInnings = components['schemas']['CricketInnings']
export type CricketDetail = components['schemas']['CricketDetail']
export type NextBallContext = components['schemas']['NextBallContext']
export type CricketExtraKind = components['schemas']['CricketExtraKind']
export type CricketDismissalKind = components['schemas']['CricketDismissalKind']
export type CricketBattingEntry = components['schemas']['CricketBattingEntry']
export type CricketBowlingEntry = components['schemas']['CricketBowlingEntry']
export type CricketScore = components['schemas']['CricketScore']
export type CricketScoreInnings = components['schemas']['CricketScoreInnings']
export type Overs = components['schemas']['Overs']
export type CricketLiveEvent = components['schemas']['CricketLiveEvent']
type LiveEvent = components['schemas']['LiveEvent']
type DetailedScore = components['schemas']['DetailedScore']
type Score = components['schemas']['Score']
type Match = components['schemas']['Match']

/** Narrows a match's detailed score to its cricket detail — innings plus,
 *  while a match is actually being live-scored, the bounded recent-ball
 *  window and next-ball context (see `CricketDetail`'s doc comment on the
 *  backend for why it's one merged shape, live or finished). `null` when
 *  there's none yet or it's for a different sport. */
export function cricketDetailFrom(detail: DetailedScore | null | undefined): CricketDetail | null {
  if (!detail || detail.type !== 'Cricket') return null
  return detail
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
  return cricketDetailFrom(detail)?.innings ?? null
}

/** A `detailed_score` innings (batting/bowling cards, totals) with its
 *  ball-by-ball log folded back in from the live event log — what the match
 *  detail page builds for the scorecard/run-rate graph, since
 *  `detailed_score` itself no longer carries deliveries (see
 *  `inningsDeliveriesFromEvents`). */
export type CricketInningsWithDeliveries = CricketInnings & { deliveries: CricketDelivery[] }

/** One innings' ball-by-ball log, recovered by segmenting the raw live event
 *  log on its `InningsStart`/`InningsEnd` markers. This is the durable
 *  source for a completed match's full run-progression graph — the detail
 *  (`CricketDetail.recent_deliveries`) only ever keeps a bounded window for
 *  the innings currently open (or just-finished), since that's all the live
 *  view itself needs; the event log has no such bound (one item per ball,
 *  not one item per match) and is safe to read in full regardless of match
 *  length. */
export interface InningsDeliveries {
  batting_side_id: string
  bowling_side_id: string
  deliveries: CricketDelivery[]
}

export function inningsDeliveriesFromEvents(events: LiveEvent[]): InningsDeliveries[] {
  const innings: InningsDeliveries[] = []
  let current: InningsDeliveries | null = null

  for (const record of events) {
    if (record.event.sport !== 'Cricket') continue
    // `event.sport`'s narrowing erases `kind` on the nested union (the same
    // openapi-typescript quirk as `Invitation.kind` in `lib/members.ts`) —
    // cast back to the real union to read it.
    const event = record.event as unknown as CricketLiveEvent
    if (event.kind === 'InningsStart') {
      // Close out anything left open without an explicit End, rather than
      // silently dropping it — mirrors `derive_state`'s fallback on the
      // backend (shouldn't normally happen).
      if (current) innings.push(current)
      current = {
        batting_side_id: event.batting_side_id,
        bowling_side_id: event.bowling_side_id,
        deliveries: [],
      }
    } else if (event.kind === 'Delivery' && current) {
      current.deliveries.push(event)
    } else if (event.kind === 'InningsEnd' && current) {
      innings.push(current)
      current = null
    }
  }
  if (current) innings.push(current)
  return innings
}

/** The minimal per-innings shape match-aggregate math and the state-of-game
 *  sentence need — both `CricketDetail.innings` (rich, ball-by-ball derived,
 *  live or finished) and a confirmed `CricketScore.innings` (the persisted
 *  summary) satisfy this structurally, so the same functions work on either. */
interface CricketInningsTotal {
  batting_side_id: string
  bowling_side_id: string
  runs: number
  wickets: number
  overs: Overs
}

/** Ditto, for the match-level progress shape: a detail's own state, or a
 *  finished match's confirmed score reframed as one (see
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
 *  `CricketDetail.awaiting_next_innings`). */
export function currentInnings(detail: CricketDetail): CricketInnings | null {
  if (detail.awaiting_next_innings) return null
  return detail.innings[detail.innings.length - 1] ?? null
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

/** This over's deliveries out of a detail's bounded recent-deliveries window
 *  (`CricketDetail.recent_deliveries`) — everything recorded
 *  against the latest over index (we assign `over`/`ball` ourselves on
 *  submit, so this is a simple filter rather than a rolling window). */
export function currentOverDeliveries(recentDeliveries: CricketDelivery[]): CricketDelivery[] {
  const latestOver = recentDeliveries.reduce((max, d) => Math.max(max, d.over), 0)
  return recentDeliveries.filter((d) => d.over === latestOver)
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

/** "3.2-0-24-1" bowling figures (overs-maidens-runs-wickets), or an em dash
 *  before the bowler has sent down a delivery. */
export function bowlingFigures(entry: CricketBowlingEntry | null): string {
  if (!entry) return '—'
  return `${formatOvers(entry.overs)}-${entry.maidens}-${entry.runs_conceded}-${entry.wickets}`
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
 *
 * A win is only reported by wickets when the chasing side's innings ended
 * *before* using its full over allocation — i.e. they got there with
 * balls to spare. If both sides batted out their complete overs quota, the
 * "wickets in hand" framing doesn't mean anything (nobody was chasing down
 * a target mid-over); that's reported as a runs margin instead, same as the
 * side batting first finishing ahead.
 */
export function cricketStateDescription(
  match: Pick<Match, 'sides' | 'players'>,
  state: CricketMatchProgress,
  format: Pick<CricketFormat, 'innings_per_side' | 'overs_per_innings'>,
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

  const oversAllUsed =
    format.overs_per_innings != null &&
    last.overs.overs >= format.overs_per_innings &&
    last.overs.balls === 0

  if (battingTotal > bowlingTotal && !oversAllUsed) {
    const rosterSize = match.players.filter((p) => p.side_id === last.batting_side_id).length
    const maxWickets = rosterSize > 1 ? rosterSize - 1 : 10
    const remaining = Math.max(maxWickets - last.wickets, 0)
    return `${sideNameFor(match, last.batting_side_id)} won by ${remaining} wicket${remaining === 1 ? '' : 's'}`
  }
  const winnerSideId = battingTotal > bowlingTotal ? last.batting_side_id : last.bowling_side_id
  const margin = Math.abs(battingTotal - bowlingTotal)
  return `${sideNameFor(match, winnerSideId)} won by ${margin} run${margin === 1 ? '' : 's'}`
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

// `NextBallContext` (who's on strike/bowling for the next delivery) is now
// folded server-side, incrementally, as part of `CricketDetail` (see
// `CricketDetail.next_ball_context` and the backend's `apply_delivery`) — no
// client-side replay needed for the online case. An offline-scoring client
// will need its own local copy of that same incremental fold (applied to its
// not-yet-synced queue) when that's built; it isn't yet, so there's nothing
// here today.
