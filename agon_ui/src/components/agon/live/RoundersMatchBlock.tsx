import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { LiveIndicator } from './LiveIndicator'
import {
  currentRoundersInnings,
  formatHalfRounders,
  playerNameFor,
  postLabel,
  roundersStateDescription,
  sideNameFor,
  type RoundersScore,
} from '@/lib/roundersScore'
import { roundersFormat } from '@/lib/matchFormat'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/**
 * The score block + "who's on base" row for a rounders match being scored
 * live (mirrors `CricketMatchBlock`'s role, adapted for rounders' concurrent
 * runners instead of a striker/non-striker pair). Presentational: the caller
 * fetches the live score (`useMatchScore`) and passes the derived rounders
 * score down.
 */
export function RoundersMatchBlock({
  match,
  state,
  showDescription = true,
}: {
  match: Match | FeedMatch | SearchMatch
  state: RoundersScore
  /** Whether to show the state-of-game line (leading/result) here. Callers
   *  that already surface it elsewhere (the feed card's header) pass
   *  `false` so it isn't said twice — the innings-break heading then falls
   *  back to a neutral "Innings break"/"Match complete" instead. */
  showDescription?: boolean
}) {
  const innings = currentRoundersInnings(state)
  const format = roundersFormat(match.format)
  const description = roundersStateDescription(
    match,
    { innings: state.innings, awaiting_next_innings: state.awaiting_next_innings ?? true },
    format,
  )

  if (!innings) {
    // Between innings, or nothing bowled yet. Every innings played so far is
    // still worth showing rather than discarding. `description` is only
    // ever non-null here once the match is complete, so its presence
    // doubles as that signal — same convention as `CricketMatchBlock`.
    const heading = showDescription
      ? description || 'Innings break'
      : description
        ? 'Match complete'
        : 'Innings break'
    return (
      <div className="rounded-lg bg-muted/50 px-3.5 py-3">
        <div className="flex items-center justify-between">
          <p
            className={cn(
              'text-sm font-medium',
              showDescription && description ? 'text-primary' : 'text-muted-foreground',
            )}
          >
            {heading}
          </p>
          <LiveIndicator />
        </div>
        {state.innings.length > 0 && (
          <div className="mt-1 space-y-0.5">
            {state.innings.map((inn, i) => (
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

  const battingName = sideNameFor(match, innings.batting_side_id)
  const fieldingName = sideNameFor(match, innings.fielding_side_id)
  const runners = Object.entries(state.next_ball_context?.runners_on_base ?? {})
  const goodBallsLabel = format.good_balls_per_innings
    ? `${innings.good_balls_bowled}/${format.good_balls_per_innings}`
    : `${innings.good_balls_bowled}`

  return (
    <div className="rounded-lg bg-muted/50 px-3.5 py-3">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-xs font-medium text-muted-foreground">{battingName} batting</p>
          <p className="text-2xl font-medium leading-tight tracking-tight">
            {formatHalfRounders(innings.half_rounders)}
            <span className="ml-1.5 text-sm font-normal text-muted-foreground">
              ({innings.outs} out · {goodBallsLabel} balls)
            </span>
          </p>
          {runners.length > 0 && (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {runners
                .map(([playerId, post]) => {
                  const name = playerNameFor(match, playerId, state.players)
                  return name ? `${name} (${postLabel(post)})` : null
                })
                .filter(Boolean)
                .join(' · ')}
            </p>
          )}
          {showDescription && description && (
            <p className="mt-0.5 text-xs font-medium text-primary">{description}</p>
          )}
        </div>
        <LiveIndicator />
      </div>

      <p className="mt-1.5 text-[11px] text-muted-foreground">vs {fieldingName}</p>
    </div>
  )
}
