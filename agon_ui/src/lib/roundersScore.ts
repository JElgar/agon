import type { components } from '@/types/api'
import { memberName, type ScorePlayers } from './members'
import type { RoundersFormat } from './matchFormat'

export type RoundersPost = components['schemas']['RoundersPost']
export type RoundersOutKind = components['schemas']['RoundersOutKind']
export type RoundersOut = components['schemas']['RoundersOut']
export type RoundersRunnerMovement = components['schemas']['RoundersRunnerMovement']
export type RoundersDelivery = components['schemas']['RoundersDelivery']
export type RoundersNextBallContext = components['schemas']['RoundersNextBallContext']
export type RoundersBattingEntry = components['schemas']['RoundersBattingEntry']
export type RoundersBowlingEntry = components['schemas']['RoundersBowlingEntry']
export type RoundersFallOfOut = components['schemas']['RoundersFallOfOut']
export type RoundersScore = components['schemas']['RoundersScore']
export type RoundersScoreInnings = components['schemas']['RoundersScoreInnings']
export type RoundersLiveEvent = components['schemas']['RoundersLiveEvent']
type Score = components['schemas']['Score']
type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
/** Anything with an optional `players` list — see `members.ts`'s `MatchLike`
 *  for why `FeedMatch`/`SearchMatch` need their own explicit branches. */
type MatchLike = { players?: MatchPlayer[] } | FeedMatch | SearchMatch

/** Narrows a match's score to its rounders variant — same role as
 *  `cricketScoreFrom` in `lib/cricketScore.ts`. `null` when there's no score
 *  yet or it's the plain totals-only shape a manually-logged (not
 *  live-scored) rounders result degrades to. */
export function roundersScoreFrom(score: Score | null | undefined): RoundersScore | null {
  if (!score || score.type !== 'Rounders') return null
  return score
}

/** A match's per-innings totals (plus whatever per-player detail they carry)
 *  straight off its score — `null` if there's no score yet or it's not
 *  `Rounders`. Mirrors `cricketInningsFor` in `lib/cricketScore.ts`. */
export function roundersInningsFor(score: Score | null | undefined): RoundersScoreInnings[] | null {
  return roundersScoreFrom(score)?.innings ?? null
}

/** The minimal per-innings shape match-aggregate math needs — a live
 *  `RoundersScore.innings` and a confirmed one satisfy this structurally
 *  either way. */
interface RoundersInningsTotal {
  batting_side_id: string
  fielding_side_id: string
  half_rounders: number
  outs: number
}

/** Ditto, for the match-level progress shape — mirrors
 *  `CricketMatchProgress`. */
export interface RoundersMatchProgress {
  innings: RoundersInningsTotal[]
  awaiting_next_innings: boolean
}

/** Reframes a completed match's `RoundersScore` as a `RoundersMatchProgress`
 *  — there's no currently-open innings once the score is confirmed. */
export function roundersProgressFromScore(score: RoundersScore): RoundersMatchProgress {
  return { innings: score.innings, awaiting_next_innings: true }
}

/** The innings currently being played, or `null` when the match hasn't
 *  started its first innings yet, or is between innings. */
export function currentRoundersInnings(score: RoundersScore): RoundersScoreInnings | null {
  if (score.awaiting_next_innings ?? true) return null
  return score.innings[score.innings.length - 1] ?? null
}

/** "4½" / "4" — half-rounder units rendered as the conventional fractional
 *  score (a full rounder is 2 units). */
export function formatHalfRounders(units: number): string {
  const whole = Math.floor(units / 2)
  return units % 2 === 1 ? `${whole}½` : `${whole}`
}

const POST_LABEL: Record<RoundersPost, string> = {
  first: '1st post',
  second: '2nd post',
  third: '3rd post',
  fourth: 'Home',
}

export function postLabel(post: RoundersPost): string {
  return POST_LABEL[post]
}

const OUT_KIND_LABEL: Record<RoundersOutKind, string> = {
  caught: 'Caught',
  stumped_at_post: 'Stumped at post',
  overtook_runner: 'Overtook runner',
  obstruction: 'Obstruction',
  failed_to_run: 'Failed to run',
}

export function outKindLabel(kind: RoundersOutKind): string {
  return OUT_KIND_LABEL[kind]
}

/** Total half-rounders scored by each side across every completed innings so
 *  far — mirrors `matchTotalsBySide` in `lib/cricketScore.ts`. */
export function roundersMatchTotalsBySide(state: RoundersMatchProgress): Record<string, number> {
  const totals: Record<string, number> = {}
  for (const innings of state.innings) {
    totals[innings.batting_side_id] = (totals[innings.batting_side_id] ?? 0) + innings.half_rounders
  }
  return totals
}

/** The batting order's size for a side — the roster registered on it, or the
 *  standard 9 if the roster looks too small to make sense of (e.g. not fully
 *  set up) — same fallback reasoning as cricket's `wicketsInHand`. */
export function battingOrderSize(
  match: Pick<Match, 'sides'> & Partial<Pick<Match, 'players'>>,
  sideId: string,
): number {
  const rosterSize = (match.players ?? []).filter((p) => p.side_id === sideId).length
  return rosterSize > 0 ? rosterSize : 9
}

/** Whether every batting-order slot is out — the "Batters out" grid full,
 *  same signal as cricket's `wicketsInHand` reaching zero. */
export function allBattersOut(
  match: Pick<Match, 'sides'> & Partial<Pick<Match, 'players'>>,
  innings: Pick<RoundersScoreInnings, 'batting_side_id' | 'outs'>,
): boolean {
  return innings.outs >= battingOrderSize(match, innings.batting_side_id)
}

/** Whether the innings' good-ball cap (if the format sets one) has been
 *  reached — the "Good balls" grid full. */
export function goodBallsComplete(
  innings: Pick<RoundersScoreInnings, 'good_balls_bowled'>,
  format: Pick<RoundersFormat, 'good_balls_per_innings'>,
): boolean {
  return (
    format.good_balls_per_innings != null &&
    innings.good_balls_bowled >= format.good_balls_per_innings
  )
}

/**
 * A one-line summary of where the match stands — mirrors
 * `cricketStateDescription`'s three phases (leading, chasing, result), using
 * half-rounders as the unit instead of runs. `null` when none apply yet
 * (the match's very first innings, with nothing yet to lead or chase).
 */
export function roundersStateDescription(
  match: Pick<Match, 'sides'> & Partial<Pick<Match, 'players'>>,
  state: RoundersMatchProgress,
  format: Pick<RoundersFormat, 'innings_per_side'>,
): string | null {
  if (state.innings.length === 0) return null
  const quota = match.sides.length * format.innings_per_side
  const totals = roundersMatchTotalsBySide(state)

  const open = state.awaiting_next_innings ? null : state.innings[state.innings.length - 1]
  if (open) {
    const battingTotal = totals[open.batting_side_id] ?? 0
    const fieldingTotal = totals[open.fielding_side_id] ?? 0

    if (state.innings.length === quota) {
      const needed = fieldingTotal + 1 - battingTotal
      if (needed <= 0) return null
      return `${sideNameFor(match, open.batting_side_id)} need ${formatHalfRounders(needed)} to win`
    }

    const fieldingHasBatted = state.innings
      .slice(0, -1)
      .some((i) => i.batting_side_id === open.fielding_side_id)
    if (!fieldingHasBatted) return null
    const diff = battingTotal - fieldingTotal
    if (diff === 0) return 'Scores level'
    const leadingSideId = diff > 0 ? open.batting_side_id : open.fielding_side_id
    return `${sideNameFor(match, leadingSideId)} lead by ${formatHalfRounders(Math.abs(diff))}`
  }

  if (!state.awaiting_next_innings || state.innings.length < quota) return null
  const last = state.innings[state.innings.length - 1]
  const battingTotal = totals[last.batting_side_id] ?? 0
  const fieldingTotal = totals[last.fielding_side_id] ?? 0
  if (battingTotal === fieldingTotal) return 'Match tied'

  const winnerSideId = battingTotal > fieldingTotal ? last.batting_side_id : last.fielding_side_id
  const margin = Math.abs(battingTotal - fieldingTotal)
  return `${sideNameFor(match, winnerSideId)} won by ${formatHalfRounders(margin)}`
}

/** Display name for a side id: its name, or a neutral fallback. */
export function sideNameFor(match: Pick<Match, 'sides'>, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
}

/** Player display name for a match-scoped player id — same contract as
 *  cricket's `playerNameFor` in `lib/cricketScore.ts`: checks the score's
 *  own resolved-name map first (the only source a feed/search card's
 *  trimmed match type carries), falling back to the match's own roster. */
export function playerNameFor(
  match: MatchLike,
  playerId: string | undefined | null,
  scorePlayers?: ScorePlayers,
): string | null {
  if (!playerId) return null
  const resolved = scorePlayers?.[playerId]
  if (resolved) return resolved.name
  const players = ('players' in match && match.players) || []
  const player = players.find((p) => p.member.id === playerId)
  return player ? memberName(player.member) : null
}
