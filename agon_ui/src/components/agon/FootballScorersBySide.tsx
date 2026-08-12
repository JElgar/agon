import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { scorersBySide, type FootballScorerLine } from '@/lib/liveScore'
import type { ScorePlayers } from '@/lib/members'

type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type MatchSide = components['schemas']['MatchSide']
type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/**
 * Every football scorer for a finished match, grouped by side and merged
 * onto one line per player with each minute they scored — e.g. "James Elgar
 * 5', 53'" — laid out side-by-side under the score, aligned under `sideA`/
 * `sideB` the same way the score header itself is. Shared between the feed/
 * profile card (`MatchCard`) and the match detail page header; both show
 * this in place of goals for a finished/confirmed-or-pending score.
 * `FootballScorecard`'s full event timeline (goals *and* cards/subs) further
 * down the detail page is unaffected — this is just the score header's
 * summary. Renders nothing if neither side has a goal to show.
 */
export function FootballScorersBySide({
  goals,
  match,
  players,
  sideA,
  sideB,
  className,
}: {
  goals: FootballGoalEvent[]
  match: Match | FeedMatch | SearchMatch
  players?: ScorePlayers
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  className?: string
}) {
  const bySide = scorersBySide(goals, match, players)
  const scorersA = bySide[sideA?.id ?? ''] ?? []
  const scorersB = bySide[sideB?.id ?? ''] ?? []

  if (scorersA.length === 0 && scorersB.length === 0) return null

  return (
    <div className={cn('flex justify-between gap-3 border-t pt-2 text-muted-foreground', className)}>
      <ScorerColumn scorers={scorersA} />
      <ScorerColumn scorers={scorersB} align="right" />
    </div>
  )
}

function ScorerColumn({
  scorers,
  align,
}: {
  scorers: FootballScorerLine[]
  align?: 'right'
}) {
  return (
    <div className={cn('min-w-0 flex-1 space-y-1', align === 'right' && 'text-right')}>
      {scorers.map((s) => (
        <p key={s.key} className="truncate">
          {s.name}
          {s.minutes.length > 0 && (
            <>
              {' '}
              <span className="font-medium text-foreground">
                {s.minutes.map((m) => `${m}'`).join(', ')}
              </span>
            </>
          )}
        </p>
      ))}
    </div>
  )
}
