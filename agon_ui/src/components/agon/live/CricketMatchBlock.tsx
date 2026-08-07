import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { LiveIndicator } from './LiveIndicator'
import {
  cricketStateDescription,
  currentInnings,
  currentOverDeliveries,
  deliveryChipLabel,
  formatOvers,
  isChipHighlighted,
  playerNameFor,
  runRate,
  sideNameFor,
  type CricketScore,
} from '@/lib/cricketScore'
import { cricketFormat } from '@/lib/matchFormat'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

/** One ball's chip in the "this over" row (mockup: "1  4  W  ·  2"). */
function BallChip({ label, kind }: { label: string; kind: 'boundary' | 'wicket' | null }) {
  return (
    <span
      className={cn(
        'flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold',
        kind === 'boundary' && 'bg-primary/15 text-primary',
        kind === 'wicket' && 'bg-destructive/15 text-destructive',
        kind === null && 'bg-muted text-foreground',
      )}
    >
      {label}
    </span>
  )
}

/**
 * The score block + "this over" ball row for a cricket match being scored
 * live (mirrors `LiveMatchBlock`'s role for football). Presentational: the
 * caller fetches the live score (`useMatchScore`) and passes the derived
 * cricket score down.
 */
export function CricketMatchBlock({
  match,
  state,
  showDescription = true,
}: {
  match: Match | FeedMatch | SearchMatch
  state: CricketScore
  /** Whether to show the state-of-game line (target/leading/result) here.
   *  Callers that already surface it elsewhere (the feed card's header)
   *  pass `false` so it isn't said twice — the innings-break heading then
   *  falls back to a neutral "Innings break"/"Match complete" instead. */
  showDescription?: boolean
}) {
  const innings = currentInnings(state)
  const format = cricketFormat(match.format)
  const description = cricketStateDescription(
    match,
    { innings: state.innings, awaiting_next_innings: state.awaiting_next_innings ?? true },
    format,
  )

  if (!innings) {
    // Between innings, or nothing bowled yet. Every innings played so far
    // is still worth showing rather than discarding — just without the
    // ball-by-ball detail that needs an open innings. `description` is only
    // ever non-null here once the match is complete (a target only applies
    // to an *open* final innings), so its presence doubles as that signal.
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
                {sideNameFor(match, inn.batting_side_id)} {inn.runs}/{inn.wickets}
                <span className="ml-1.5 text-sm font-normal text-muted-foreground">
                  ({formatOvers(inn.overs)} ov)
                </span>
              </p>
            ))}
          </div>
        )}
      </div>
    )
  }

  const battingName = sideNameFor(match, innings.batting_side_id)
  const bowlingName = sideNameFor(match, innings.bowling_side_id)
  const next = state.next_ball_context
  const overBalls = currentOverDeliveries(state.recent_deliveries ?? [])
  const crr = runRate(innings.runs, innings.overs, format.balls_per_over)

  return (
    <div className="rounded-lg bg-muted/50 px-3.5 py-3">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-xs font-medium text-muted-foreground">{battingName} batting</p>
          <p className="text-2xl font-medium leading-tight tracking-tight">
            {innings.runs}/{innings.wickets}
            <span className="ml-1.5 text-sm font-normal text-muted-foreground">
              ({formatOvers(innings.overs)}
              {format.overs_per_innings ? `/${format.overs_per_innings}` : ''} ov · CRR{' '}
              {crr.toFixed(2)})
            </span>
          </p>
          {next && (next.striker_player_id || next.non_striker_player_id) && (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {next.striker_player_id && <>{playerNameFor(match, next.striker_player_id)}*</>}
              {next.striker_player_id && next.non_striker_player_id && ' · '}
              {next.non_striker_player_id && <>{playerNameFor(match, next.non_striker_player_id)}</>}
            </p>
          )}
          {showDescription && description && (
            <p className="mt-0.5 text-xs font-medium text-primary">{description}</p>
          )}
        </div>
        <LiveIndicator />
      </div>

      {overBalls.length > 0 && (
        <div className="mt-2.5 border-t pt-2.5">
          <p className="mb-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            This over
          </p>
          <div className="flex gap-1.5">
            {overBalls.map((d, i) => (
              <BallChip key={i} label={deliveryChipLabel(d)} kind={isChipHighlighted(d)} />
            ))}
          </div>
        </div>
      )}

      <p className="mt-1.5 text-[11px] text-muted-foreground">vs {bowlingName}</p>
    </div>
  )
}
