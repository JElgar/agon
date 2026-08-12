import type { components } from '@/types/api'
import { describeEvent, eventEmoji, eventsFromDetail, minuteLabel, type NetballEventSource } from '@/lib/netballScore'

type Match = components['schemas']['Match']

/**
 * A netball match's full event timeline — every goal and foul recorded
 * (event-by-event live scoring only; quarter-only scoring has no
 * goal-by-goal detail to show — see `netballEventSourceFromScore`), each
 * attributed to a player and minute. Same role as `FootballScorecard`:
 * unlike `LiveMatchBlock`'s mini-ticker this isn't gated on the match still
 * being in progress, so it's what keeps goals visible on the match detail
 * page once the match is completed. Renders nothing if there's no event
 * detail recorded.
 */
export function NetballScorecard({ match, detail }: { match: Match; detail: NetballEventSource }) {
  const events = eventsFromDetail(detail)
  if (events.length === 0) return null

  const [, sideB] = match.sides

  return (
    <div className="rounded-xl border bg-card p-4">
      <p className="mb-3 text-sm font-medium">Match events</p>
      <div className="flex flex-col gap-1.5">
        {events.map((event, i) => {
          const isSideB = event.side_id === sideB?.id
          return (
            <p
              key={i}
              className={`flex items-baseline gap-1.5 text-sm ${isSideB ? 'flex-row-reverse text-right' : ''}`}
            >
              <span aria-hidden>{eventEmoji(event.kind)}</span>
              <span className="font-medium tabular-nums text-muted-foreground">
                {minuteLabel(event.minute)}
              </span>
              <span>{describeEvent(event, match, detail.players)}</span>
            </p>
          )
        })}
      </div>
    </div>
  )
}
