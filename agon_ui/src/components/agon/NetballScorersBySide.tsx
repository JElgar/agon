import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { scorersBySide, type NetballPeriodTimes, type NetballScorerLine } from '@/lib/netballScore'
import type { ScorePlayers } from '@/lib/members'

type NetballGoalEvent = components['schemas']['NetballGoalEvent']
type MatchSide = components['schemas']['MatchSide']
type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/**
 * Every netball scorer for a finished match, grouped by side and merged onto
 * one line per player with each minute they scored — same role and layout as
 * `FootballScorersBySide`. Empty (renders nothing) for a quarter-only-scored
 * or manually-entered result, which has no goal-by-goal detail to draw from.
 */
export function NetballScorersBySide({
  goals,
  match,
  players,
  periodTimes,
  sideA,
  sideB,
  className,
}: {
  goals: NetballGoalEvent[]
  match: Match | FeedMatch | SearchMatch
  players?: ScorePlayers
  /** Lets a live-scored goal's time show as mm:ss-within-its-quarter — see
   *  `scorersBySide`. */
  periodTimes?: NetballPeriodTimes
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  className?: string
}) {
  const bySide = scorersBySide(goals, match, players, periodTimes)
  const scorersA = bySide[sideA?.id ?? ''] ?? []
  const scorersB = bySide[sideB?.id ?? ''] ?? []

  if (scorersA.length === 0 && scorersB.length === 0) return null

  return (
    <div className={cn('grid grid-cols-2 gap-3 border-t pt-2 text-muted-foreground', className)}>
      <ScorerColumn scorers={scorersA} />
      <ScorerColumn scorers={scorersB} align="right" />
    </div>
  )
}

function ScorerColumn({
  scorers,
  align,
}: {
  scorers: NetballScorerLine[]
  align?: 'right'
}) {
  return (
    <div className={cn('min-w-0 space-y-1', align === 'right' && 'text-right')}>
      {scorers.map((s) => (
        <p key={s.key} className="truncate">
          {s.name}
          {s.times.length > 0 && (
            <>
              {' '}
              <span className="font-medium text-foreground">{s.times.join(', ')}</span>
            </>
          )}
        </p>
      ))}
    </div>
  )
}
