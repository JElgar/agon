import { useEffect, useState } from 'react'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { LiveIndicator } from './LiveIndicator'
import {
  describeEvent,
  eventEmoji,
  liveClockLabel,
  recentEvents,
  type FootballLiveState,
} from '@/lib/liveScore'

/** How often the clock label re-renders. Minute-granularity, so this just
 *  needs to be frequent enough to feel live, not per-second. */
const CLOCK_TICK_MS = 15_000

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/**
 * The score block + mini-ticker for a football match being scored live
 * (mockup "Mini-ticker — last events under the score"). Presentational: the
 * caller fetches the live snapshot (`useLiveScore`) and passes the derived
 * football state down, so `MatchCard` and `MatchDetailPage` can each decide
 * when to show this instead of their normal (confirmed/pending score) block.
 */
export function LiveMatchBlock({
  match,
  state,
  tickerLimit = 2,
}: {
  match: Match
  state: FootballLiveState
  /** How many recent events to show under the score. */
  tickerLimit?: number
}) {
  const [sideA, sideB] = match.sides
  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')
  const goalsFor = (sideId: string | undefined) =>
    state.score.find((s) => s.side_id === sideId)?.goals ?? 0

  const events = recentEvents(state, tickerLimit)

  // Ticks every viewer's clock in lockstep — the minute is computed from the
  // shared kickoff/half-time timestamps on `state`, not a per-device clock.
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), CLOCK_TICK_MS)
    return () => clearInterval(id)
  }, [])

  return (
    <div className="rounded-lg bg-muted/50 px-3.5 py-3">
      <div className="flex items-center justify-between">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <Avatar name={nameA} size="md" />
          <span className="truncate text-xs font-medium">{nameA}</span>
        </div>
        <div className="px-3 text-center">
          <div className="text-2xl font-medium leading-none tracking-tight">
            {goalsFor(sideA?.id)}
            <span className="text-muted-foreground">–</span>
            {goalsFor(sideB?.id)}
          </div>
          <div className="mt-1 flex items-center justify-center">
            {(() => {
              const label = liveClockLabel(state, now)
              return <LiveIndicator>{label === 'LIVE' ? undefined : label}</LiveIndicator>
            })()}
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-row-reverse items-center gap-2 text-right">
          <Avatar name={nameB} size="md" />
          <span className="truncate text-xs font-medium">{nameB}</span>
        </div>
      </div>

      {events.length > 0 && (
        <div className="mt-2.5 space-y-1 border-t pt-2">
          {events.map((event, i) => (
            <p key={i} className="truncate text-xs text-muted-foreground">
              <span aria-hidden>{eventEmoji(event.kind)}</span>{' '}
              {event.minute !== undefined && (
                <span className="font-medium text-foreground">{event.minute}' </span>
              )}
              {describeEvent(event, match)}
            </p>
          ))}
        </div>
      )}
    </div>
  )
}
