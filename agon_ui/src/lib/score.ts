import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type Score = components['schemas']['Score']
type MatchSide = components['schemas']['MatchSide']
type FootballGoalEvent = components['schemas']['FootballGoalEvent']

/** A finished football match's goals, straight off its confirmed/pending
 *  score — `null` for any other score type (including a football match
 *  logged manually rather than live-scored, which degrades to `Simple`; see
 *  `Score.Simple`'s doc comment). Lets a feed card show who scored without a
 *  separate `/detailed-score` fetch once the match is over. */
export function footballGoalsFromScore(score: Score): FootballGoalEvent[] | null {
  return score.type === 'Football' ? score.goals : null
}

/** The score to display for a match: the confirmed result if present, else the
 *  pending (awaiting-confirmation) submission. `null` when no score exists yet. */
export function displayScore(
  match: Pick<Match, 'confirmed_score' | 'pending_score'>,
): { score: Score; winnerSideId?: string; confirmed: boolean } | null {
  if (match.confirmed_score) {
    return {
      score: match.confirmed_score.score,
      winnerSideId: match.confirmed_score.winner_side_id,
      confirmed: true,
    }
  }
  if (match.pending_score) {
    return {
      score: match.pending_score.score,
      winnerSideId: match.pending_score.winner_side_id,
      confirmed: false,
    }
  }
  return null
}

/**
 * The headline number a side shows: for a Sets score it's the count of sets won
 * (across index-aligned entries); for a Simple score it's the points; for a
 * Football score it's the goal count (each goal's `side_id` is already the
 * side it counts for — including an own goal crediting the opponent — so a
 * plain per-side count is the full tally). Returns a map of side id →
 * headline value. A `Cricket` score has no single headline number to show
 * here — `CricketMatchBlock`/`CricketScoreBlock` render it their own way —
 * so it's an empty map, same as "no score yet".
 */
export function headlineBySide(score: Score): Record<string, number> {
  const out: Record<string, number> = {}
  if (score.type === 'Simple') {
    for (const e of score.entries) out[e.side_id] = e.points
    return out
  }
  if (score.type === 'Football') {
    for (const g of score.goals) out[g.side_id] = (out[g.side_id] ?? 0) + 1
    return out
  }
  if (score.type === 'Cricket') return out
  // Sets: a side wins a set at index i if its games exceed every other side's.
  const setCount = Math.max(0, ...score.entries.map((e) => e.sets.length))
  for (const e of score.entries) out[e.side_id] = 0
  for (let i = 0; i < setCount; i++) {
    let bestSide: string | null = null
    let bestGames = -1
    let tie = false
    for (const e of score.entries) {
      const games = e.sets[i] ?? 0
      if (games > bestGames) {
        bestGames = games
        bestSide = e.side_id
        tie = false
      } else if (games === bestGames) {
        tie = true
      }
    }
    if (bestSide && !tie) out[bestSide] += 1
  }
  return out
}

/** Per-set game scores in side order, e.g. "6–3 · 6–2". Empty for Simple scores. */
export function setLine(score: Score, sides: MatchSide[]): string[] {
  if (score.type !== 'Sets') return []
  const bySide = new Map(score.entries.map((e) => [e.side_id, e.sets]))
  const ordered = sides
    .map((s) => bySide.get(s.id))
    .filter((s): s is number[] => Array.isArray(s))
  if (ordered.length < 2) return []
  const setCount = Math.max(...ordered.map((s) => s.length))
  const lines: string[] = []
  for (let i = 0; i < setCount; i++) {
    lines.push(ordered.map((s) => s[i] ?? 0).join('–'))
  }
  return lines
}

/** Short label for the headline unit, e.g. "sets" for racket sports, "full time" otherwise.
 *  Unused for a `Cricket` score — see `headlineBySide`. */
export function headlineLabel(score: Score): string {
  return score.type === 'Sets' ? 'sets' : 'full time'
}
