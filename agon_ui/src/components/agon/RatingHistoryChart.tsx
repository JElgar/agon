import { cn } from '@/lib/utils'
import type { RatingHistoryPoint } from '@/hooks/useRatingHistory'

/** How many of the most recent matches the line draws. */
const WINDOW = 30

/** Drawing height in CSS pixels. The SVG's own coordinate space is 0–100 in
 *  both axes; see `preserveAspectRatio` below.
 *
 *  A little under `MatchActivityChart`'s 170, and deliberately not taller:
 *  height buys the line a few more pixels of travel inside a band several
 *  times its size, which is a poor trade for a card the length of the sport
 *  page. The y axis is what makes a short line readable. See `domainFor`. */
const HEIGHT = 150

/** Width reserved on the left for the y axis labels, in CSS pixels. A gutter
 *  rather than labels laid over the plot, so a label can never land on the
 *  line it is there to explain. */
const AXIS_GUTTER = 34

/** Narrowest y span we will scale to, in rating points. Stops a player whose
 *  rating has barely moved from having single-point wobbles blown up into a
 *  mountain range. */
const MIN_SPAN = 60

/**
 * Where this scale's zero is: `agon_core::rating::scale::RATING_ORIGIN`, an
 * average, unproven player. The only value on the axis that means something
 * on its own, so it gets a gridline of its own whenever it is in range.
 */
const RATING_ORIGIN = 1500

export interface RatingHistoryChartProps {
  /** Oldest first, as the endpoint returns them. */
  points: RatingHistoryPoint[]
  className?: string
}

/**
 * Rating over time: the line with its confidence band, from the EloSport
 * board on the "Agon redesign" canvas
 * (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 *
 * ## The x axis is matches, not time
 *
 * One step per rated match, evenly spaced, rather than a date scale. A
 * rating moves when you play, not while you don't — a date scale would draw
 * a long flat run through a quiet winter and bunch a busy fortnight into a
 * spike, which says something about the fixture list rather than about the
 * player. The dates are printed under the ends so the window stays readable.
 *
 * ## Why some segments are dashed
 *
 * Ratings are **applied in confirmation order but plotted in played order**,
 * so a match confirmed late lands in the middle of a line already drawn past
 * it. Where that has happened, this point's `rating_before` is not the
 * previous point's `rating` — the two disagree about where the rating stood
 * between them — and the step drawn is not the step the player was told
 * about. Those segments are dashed, because a solid line would assert a
 * continuous history the numbers don't support.
 *
 * `delta` is deliberately never printed against a step for the same reason:
 * it is what the match card said on the day, and next to a visibly different
 * step it would make one of the two look wrong when both are correct.
 *
 * `RepairTrigger::OutOfOrder` in `agon_worker` is the server side of this,
 * and the planned replay is what eventually removes the dashes — not
 * anything here.
 */
export function RatingHistoryChart({ points, className }: RatingHistoryChartProps) {
  const window = points.slice(-WINDOW)
  if (window.length === 0) return null

  const { lo, span } = domainFor(window)
  const top = lo + span

  const x = (i: number) => (window.length === 1 ? 50 : (i / (window.length - 1)) * 100)
  const y = (v: number) => ((top - v) / span) * 100

  const last = window[window.length - 1]
  const ticks = ticksFor(lo, top)

  const bandPath = [
    ...window.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(i)},${y(p.rating + p.confidence)}`),
    ...window.map((p, i) => `L${x(i)},${y(p.floor)}`).reverse(),
    'Z',
  ].join(' ')

  const segments = window.slice(1).map((_, idx) => {
    const i = idx + 1
    return {
      i,
      d: `M${x(i - 1)},${y(window[i - 1].rating)} L${x(i)},${y(window[i].rating)}`,
      // Whether the stored numbers agree about where the rating stood
      // between these two matches.
      continuous: window[i].rating_before === window[i - 1].rating,
    }
  })

  const reordered = segments.filter((s) => !s.continuous).length

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <div
        role="img"
        aria-label={describe(window, reordered)}
        className="relative w-full"
        style={{ height: HEIGHT }}
      >
        {/* Gridlines run the full width with their labels in the gutter. They
            are what makes the band affordable: without a scale to read
            against, a line crossing a sixth of the height is just a flat
            line, and with one it is a 300-point climb. */}
        {ticks.map((t) => (
          <div
            key={t}
            aria-hidden
            className="pointer-events-none absolute inset-x-0 flex items-center"
            style={{ top: `${y(t)}%` }}
          >
            <span
              className={cn(
                'shrink-0 pr-1.5 text-right text-[10px] font-medium tabular-nums',
                t === RATING_ORIGIN ? 'text-muted-foreground' : 'text-muted-foreground/60',
              )}
              style={{ width: AXIS_GUTTER }}
            >
              {t}
            </span>
            <div className={cn('h-px flex-1', t === RATING_ORIGIN ? 'bg-border' : 'bg-border/50')} />
            {t === RATING_ORIGIN && (
              <span className="pl-1 text-[10px] font-medium text-muted-foreground">avg</span>
            )}
          </div>
        ))}

        {/* `preserveAspectRatio="none"` lets the 0–100 grid stretch to
            whatever width the card is, with no measuring. Every stroke is
            `non-scaling-stroke` so the stretch can't leave the line thick on
            a wide screen and thin on a narrow one, and there is no text and
            no circle inside the SVG — both would distort, so the end marker
            and every label are HTML on top. */}
        <div className="absolute inset-y-0 right-0" style={{ left: AXIS_GUTTER }}>
          <svg
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            className="size-full overflow-hidden"
            aria-hidden
          >
            <path d={bandPath} className="fill-primary/15" />
            {segments.map((s) => (
              <path
                key={s.i}
                d={s.d}
                fill="none"
                strokeWidth={2}
                strokeLinecap="round"
                strokeDasharray={s.continuous ? undefined : '3 3'}
                vectorEffect="non-scaling-stroke"
                className={s.continuous ? 'stroke-primary' : 'stroke-primary/50'}
              />
            ))}
          </svg>

          {/* The current rating, pinned to the end of the line. Shifted by
              half its own size so it sits on the point rather than beside
              it. */}
          <span
            aria-hidden
            className="absolute size-2.5 rounded-full border-2 border-background bg-primary"
            style={{
              left: `${x(window.length - 1)}%`,
              top: `${y(last.rating)}%`,
              transform: 'translate(-50%, -50%)',
            }}
          />
        </div>
      </div>

      <div
        className="flex items-baseline justify-between gap-2 text-[11px] font-medium text-muted-foreground"
        style={{ paddingLeft: AXIS_GUTTER }}
      >
        <span>{axisLabel(window[0].played_at, last.played_at)}</span>
        <span>{axisLabel(last.played_at, window[0].played_at)}</span>
      </div>

      {reordered > 0 && (
        <span className="text-[13px] leading-snug text-muted-foreground">
          {reordered === 1 ? 'One match was' : `${reordered} matches were`} confirmed after a later
          one had already counted, so{' '}
          {reordered === 1 ? "the dashed step doesn't match" : "the dashed steps don't match"} what{' '}
          {reordered === 1 ? 'it' : 'they'} showed at the time.
        </span>
      )}
    </div>
  )
}

/**
 * The y range to draw: the whole `±` band, padded.
 *
 * The band is three sigmas wide, which after a player's placements is still
 * something like ±550 — several times the distance their rating has actually
 * travelled. Drawn without a scale to read against, that flattens a 300-point
 * climb into a wiggle, and the chart says "nothing happened" to someone whose
 * rating has climbed 300 points.
 *
 * The fix is the y axis, not a smaller band. Gridlines make a short line
 * legible as a number, and the band stays exactly what the `±` above it says
 * it is. Scaling to one sigma instead would read better still, and would be
 * a lie in the worst place to tell one: its lower edge would look like
 * `floor` (`rating - confidence`, the value this app actually ranks and gates
 * on) while sitting hundreds of points above it.
 *
 * `lineSpan` is the one floor on that: for a settled player whose band has
 * closed right up, it keeps the line off the top and bottom edges.
 */
function domainFor(points: RatingHistoryPoint[]): { lo: number; span: number } {
  const ratings = points.map((p) => p.rating)
  const lineSpan = Math.max((Math.max(...ratings) - Math.min(...ratings)) * 1.7, MIN_SPAN)

  const bandLo = Math.min(...points.map((p) => p.floor))
  const bandHi = Math.max(...points.map((p) => p.rating + p.confidence))
  const span = Math.max((bandHi - bandLo) * 1.12, lineSpan, MIN_SPAN)
  return { lo: (bandLo + bandHi) / 2 - span / 2, span }
}

/**
 * Round values to rule a gridline at — at most five, plus `RATING_ORIGIN`
 * whenever the drawn range contains it.
 *
 * Ticks within a sliver of either edge are dropped: their labels would be
 * clipped by the plot's own bounds, and a half-visible number on an axis is
 * worse than one fewer gridline.
 */
function ticksFor(lo: number, top: number): number[] {
  const span = top - lo
  const step = [50, 100, 200, 250, 500, 1000].find((s) => span / s <= 5) ?? 1000
  const out: number[] = []
  for (let v = Math.ceil(lo / step) * step; v <= top; v += step) {
    if ((v - lo) / span > 0.06 && (top - v) / span > 0.04) out.push(v)
  }
  if (lo < RATING_ORIGIN && RATING_ORIGIN < top && !out.includes(RATING_ORIGIN)) {
    out.push(RATING_ORIGIN)
  }
  return out.sort((a, b) => a - b)
}

/**
 * An end of the x axis. Months when the window spans more than one, days
 * when it doesn't — two identical "Apr 2026"s under a chart look like a bug,
 * and a fortnight of matches is exactly when the day is the useful part.
 */
function axisLabel(iso: string, other: string): string {
  const date = new Date(iso)
  const otherDate = new Date(other)
  if (Number.isNaN(date.getTime())) return ''
  const sameMonth =
    !Number.isNaN(otherDate.getTime()) &&
    date.getFullYear() === otherDate.getFullYear() &&
    date.getMonth() === otherDate.getMonth()
  return date.toLocaleDateString(
    undefined,
    sameMonth ? { day: 'numeric', month: 'short' } : { month: 'short', year: 'numeric' },
  )
}

/**
 * What a screen reader gets. Deliberately a summary rather than one reading
 * per match: thirty "1642 give or take 180" in a row is technically the
 * whole chart and practically unusable, and the shape — where it started,
 * where it is, how far it ranged, whether the band has closed — is what a
 * sighted reader takes from it too.
 */
function describe(points: RatingHistoryPoint[], reordered: number): string {
  const first = points[0]
  const last = points[points.length - 1]
  const ratings = points.map((p) => p.rating)
  const parts = [
    `Rating over ${points.length} ranked ${points.length === 1 ? 'match' : 'matches'}.`,
    `Started at ${first.rating} give or take ${first.confidence}, now ${last.rating} give or take ${last.confidence}.`,
    `Ranged from ${Math.min(...ratings)} to ${Math.max(...ratings)}.`,
  ]
  if (last.confidence < first.confidence) {
    parts.push('The confidence range has narrowed over this period.')
  }
  if (reordered > 0) {
    parts.push(
      `${reordered} ${reordered === 1 ? 'step is' : 'steps are'} marked as out of order, confirmed after a later match had already counted.`,
    )
  }
  return parts.join(' ')
}
