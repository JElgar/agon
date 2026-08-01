import type { components } from '@/types/api'
import { describeEvent, eventEmoji, eventsFromDetail, minuteLabel, type FootballEventSource } from '@/lib/liveScore'

type Match = components['schemas']['Match']

/**
 * A football match's full event timeline — every goal, card, and
 * substitution recorded (live or entered after the fact), each attributed to
 * a player and minute. Unlike `LiveMatchBlock`'s mini-ticker this isn't
 * gated on the match still being in progress, so it's what keeps goals
 * visible on the match detail page once the match is completed — reading
 * straight off the confirmed/pending score once there is one (see
 * `footballEventSourceFromScore`), no separate fetch needed. Renders nothing
 * if there's no event detail recorded (e.g. only a headline score was ever
 * entered).
 */
export function FootballScorecard({ match, detail }: { match: Match; detail: FootballEventSource }) {
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
              <span>{describeEvent(event, match)}</span>
            </p>
          )
        })}
      </div>
    </div>
  )
}
