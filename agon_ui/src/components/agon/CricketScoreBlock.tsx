import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import {
  cricketProgressFromScore,
  cricketStateDescription,
  sideNameFor,
  type CricketScore,
} from '@/lib/cricketScore'
import { cricketFormat } from '@/lib/matchFormat'

type Match = components['schemas']['Match']

/**
 * The cricket score tile for a match that's been fully scored and confirmed
 * — sourced entirely from the confirmed score's per-innings totals
 * (`finishMatch` in `CricketLiveScoringPage` is what puts them there), no
 * live event log fetch involved. Mirrors `CricketMatchBlock`'s "between
 * innings" rendering, minus anything that needs an innings still open
 * (there isn't one on a finished match).
 */
export function CricketScoreBlock({
  match,
  score,
  showDescription = true,
}: {
  match: Match
  score: CricketScore
  /** Whether to show the result line here. Callers that already surface it
   *  elsewhere (the feed card's header) pass `false` so it isn't said twice. */
  showDescription?: boolean
}) {
  const format = cricketFormat(match.format)
  const description = cricketStateDescription(match, cricketProgressFromScore(score), format)

  return (
    <div className="rounded-lg bg-muted/50 px-3.5 py-3">
      <p
        className={cn(
          'text-sm font-medium',
          showDescription && description ? 'text-primary' : 'text-muted-foreground',
        )}
      >
        {showDescription ? description || 'Result' : 'Result'}
      </p>
      {score.innings.length > 0 && (
        <div className="mt-1 space-y-0.5">
          {score.innings.map((inn, i) => (
            <p key={i} className="text-lg font-medium leading-tight tracking-tight">
              {sideNameFor(match, inn.batting_side_id)} {inn.runs}/{inn.wickets}
              <span className="ml-1.5 text-sm font-normal text-muted-foreground">
                ({inn.overs.toFixed(1)} ov)
              </span>
            </p>
          ))}
        </div>
      )}
    </div>
  )
}
