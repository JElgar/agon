import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { LiveIndicator } from './LiveIndicator'
import {
  battingEntryFor,
  battingLine,
  currentInnings,
  currentOverDeliveries,
  deliveryChipLabel,
  isChipHighlighted,
  nextBallContext,
  playerNameFor,
  runRate,
  type CricketLiveState,
} from '@/lib/cricketScore'

type Match = components['schemas']['Match']

function sideNameFor(match: Match, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
}

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
 * caller fetches the live snapshot (`useLiveScore`) and passes the derived
 * cricket state down.
 */
export function CricketMatchBlock({ match, state }: { match: Match; state: CricketLiveState }) {
  const innings = currentInnings(state)

  if (!innings) {
    // Between innings, or nothing bowled yet — still worth surfacing that
    // the match is live, just without a score to show.
    return (
      <div className="flex items-center justify-between rounded-lg bg-muted/50 px-3.5 py-3">
        <p className="text-sm text-muted-foreground">Innings break</p>
        <LiveIndicator />
      </div>
    )
  }

  const battingName = sideNameFor(match, innings.batting_side_id)
  const bowlingName = sideNameFor(match, innings.bowling_side_id)
  const next = nextBallContext(innings)
  const striker = battingEntryFor(innings, next.strikerPlayerId)
  const nonStriker = battingEntryFor(innings, next.nonStrikerPlayerId)
  const overBalls = currentOverDeliveries(innings)
  const crr = runRate(innings.runs, innings.overs)

  return (
    <div className="rounded-lg bg-muted/50 px-3.5 py-3">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-xs font-medium text-muted-foreground">{battingName} batting</p>
          <p className="text-2xl font-medium leading-tight tracking-tight">
            {innings.runs}/{innings.wickets}
            <span className="ml-1.5 text-sm font-normal text-muted-foreground">
              ({innings.overs.toFixed(1)} ov · CRR {crr.toFixed(2)})
            </span>
          </p>
          {(striker || nonStriker) && (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {next.strikerPlayerId && (
                <>
                  {playerNameFor(match, next.strikerPlayerId)}* {battingLine(striker)}
                </>
              )}
              {next.strikerPlayerId && next.nonStrikerPlayerId && ' · '}
              {next.nonStrikerPlayerId && (
                <>
                  {playerNameFor(match, next.nonStrikerPlayerId)} {battingLine(nonStriker)}
                </>
              )}
            </p>
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
