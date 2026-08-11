import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import {
  formatHalfRounders,
  roundersProgressFromScore,
  roundersStateDescription,
  sideNameFor,
  type RoundersScore,
} from '@/lib/roundersScore'
import { roundersFormat } from '@/lib/matchFormat'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/**
 * The rounders score tile for a match that's been fully scored and
 * confirmed — sourced entirely from the confirmed score's per-innings
 * totals, no live event log fetch involved. Mirrors `CricketScoreBlock`.
 */
export function RoundersScoreBlock({
  match,
  score,
  showDescription = true,
}: {
  match: Match | FeedMatch | SearchMatch
  score: RoundersScore
  /** Whether to show the result line here. Callers that already surface it
   *  elsewhere (the feed card's header) pass `false` so it isn't said twice. */
  showDescription?: boolean
}) {
  const format = roundersFormat(match.format)
  const description = roundersStateDescription(match, roundersProgressFromScore(score), format)

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
              {sideNameFor(match, inn.batting_side_id)} {formatHalfRounders(inn.half_rounders)}
              <span className="ml-1.5 text-sm font-normal text-muted-foreground">
                ({inn.outs} out)
              </span>
            </p>
          ))}
        </div>
      )}
    </div>
  )
}
