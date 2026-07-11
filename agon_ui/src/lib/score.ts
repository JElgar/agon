import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type Score = components['schemas']['Score']
type MatchSide = components['schemas']['MatchSide']

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
 * (across index-aligned entries); for a Simple score it's the points. Returns a
 * map of side id → headline value.
 */
export function headlineBySide(score: Score): Record<string, number> {
  const out: Record<string, number> = {}
  if (score.type === 'Simple') {
    for (const e of score.entries) out[e.side_id] = e.points
    return out
  }
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

/** Short label for the headline unit, e.g. "sets" for racket sports, "FT" otherwise. */
export function headlineLabel(score: Score): string {
  return score.type === 'Sets' ? 'sets' : 'FT'
}
