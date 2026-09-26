import { Check } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { AvatarStack } from './AvatarStack'

type FeedMatch = components['schemas']['FeedMatch']

/** "MON" / "29" — the compact date-box shown on an upcoming-match card. */
function dateParts(iso: string): { weekday: string; day: string } {
  const date = new Date(iso)
  return {
    weekday: date.toLocaleDateString(undefined, { weekday: 'short' }).toUpperCase(),
    day: String(date.getDate()),
  }
}

/** "19:30" — the scheduled kickoff time, local to the viewer. */
function timeLabel(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
}

/**
 * A compact upcoming-match card for the feed's "Coming up" list — date box,
 * title, time/location, who's playing, and the viewer's own RSVP state. A
 * fixed-width card in the horizontal snap-scroll strip below `xl`, full-width
 * in the desktop sidebar's vertical stack at `xl` and up. See the "Agon
 * redesign" canvas's `Main.dc.html`/`DesktopHome.dc.html`.
 */
export function UpcomingMatchCard({
  match,
  onOpen,
  className,
}: {
  match: FeedMatch
  onOpen?: () => void
  className?: string
}) {
  const { weekday, day } = dateParts(match.starts_at)
  const going = match.viewer_side_id != null
  const totalGoing = match.sides.reduce((sum, s) => sum + s.player_count, 0)
  const people = match.sides
    .flatMap((s) => s.roster_preview ?? [])
    .map((p) => ({ name: p.name, imageUrl: p.avatar_url }))

  return (
    <button
      type="button"
      onClick={onOpen}
      className={cn(
        'flex w-[300px] shrink-0 scroll-ml-4 snap-start items-center gap-3.5 rounded-2xl border bg-card p-3.5 text-left text-card-foreground xl:w-full xl:shrink xl:scroll-ml-0 xl:snap-align-none',
        className,
      )}
    >
      <div className="flex size-14 shrink-0 flex-col items-center justify-center rounded-2xl bg-primary/10">
        <span className="text-xs font-bold tracking-wide text-primary">{weekday}</span>
        <span className="font-display text-2xl font-extrabold leading-none">{day}</span>
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-base font-bold">{match.name}</span>
        <span className="truncate text-sm text-muted-foreground">
          {timeLabel(match.starts_at)}
          {match.location && ` · ${match.location.text}`}
        </span>
        {(people.length > 0 || totalGoing > 0) && (
          <div className="mt-1 flex items-center gap-2">
            {people.length > 0 && <AvatarStack people={people} size="sm" />}
            <span className="whitespace-nowrap text-xs text-muted-foreground">
              {totalGoing} going
            </span>
          </div>
        )}
      </div>
      {going ? (
        <span className="flex h-8 shrink-0 items-center gap-1 rounded-full bg-success/15 px-2.5 text-xs font-bold text-success">
          <Check className="size-3.5" /> Going
        </span>
      ) : (
        <span className="flex h-10 shrink-0 items-center rounded-full bg-primary px-3.5 text-sm font-bold text-primary-foreground">
          I'm in
        </span>
      )}
    </button>
  )
}
