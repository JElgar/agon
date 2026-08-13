import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type Score = components['schemas']['Score']
type MatchSide = components['schemas']['MatchSide']
type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type NetballGoalEvent = components['schemas']['NetballGoalEvent']

/** A finished football match's goals, straight off its confirmed/pending
 *  score — `null` for any other score type, or a football score with no
 *  goal-by-goal detail attached (a manual entry with just the final tally).
 *  Lets a feed card show who scored without a separate live-poll fetch once
 *  the match is over. */
export function footballGoalsFromScore(score: Score): FootballGoalEvent[] | null {
  return score.type === 'Football' ? (score.goals ?? null) : null
}

/** A finished netball match's goals, straight off its confirmed/pending
 *  score — `null` for any other score type, or a netball score with no
 *  goal-by-goal detail attached (a quarter-only-scored or manual entry with
 *  just the final tally). Same role as `footballGoalsFromScore`. */
export function netballGoalsFromScore(score: Score): NetballGoalEvent[] | null {
  return score.type === 'Netball' ? (score.goals ?? null) : null
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
 * Football score it's the goal tally (`score.score`, keyed by side id —
 * already correct including own goals, which credit the benefiting side);
 * for a Netball score it's the same kind of running goal tally, whichever of
 * netball's two live-scoring methods produced it (see
 * `NetballScore.score`'s backend doc comment). Returns a map of side id →
 * headline value. A `Cricket` score has no single headline number to show
 * here — `CricketMatchBlock`/`CricketScoreBlock` render it their own way —
 * so it's an empty map, same as "no score yet".
 */
export function headlineBySide(score: Score): Record<string, number> {
  if (score.type === 'Simple') return { ...score.entries }
  if (score.type === 'Football') return { ...score.score }
  if (score.type === 'Netball') return { ...score.score }
  if (score.type === 'Cricket') return {}
  // Sets: a side wins a set at index i if its games exceed every other side's.
  const out: Record<string, number> = {}
  const entries = Object.entries(score.entries)
  const setCount = Math.max(0, ...entries.map(([, sets]) => sets.length))
  for (const [sideId] of entries) out[sideId] = 0
  for (let i = 0; i < setCount; i++) {
    let bestSide: string | null = null
    let bestGames = -1
    let tie = false
    for (const [sideId, sets] of entries) {
      const games = sets[i] ?? 0
      if (games > bestGames) {
        bestGames = games
        bestSide = sideId
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
  const ordered = sides
    .map((s) => score.entries[s.id])
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
