import { useId } from 'react'
import { cn } from '@/lib/utils'
import type { RatingHistoryPoint } from '@/hooks/useRatingHistory'

/** How many of the most recent matches the line draws. */
const WINDOW = 30

/** Drawing height in CSS pixels. The SVG's own coordinate space is 0–100 in
 *  both axes; see `preserveAspectRatio` below. */
const HEIGHT = 150

/**
 * The most of the y axis the confidence band is allowed to claim, as a
 * multiple of the span the line itself needs. See `domainFor`.
 */
const BAND_CAP = 2.2

/** Narrowest y span we will scale to, in rating points. Stops a player whose
 *  rating has barely moved from having single-point wobbles blown up into a
 *  mountain range. */
const MIN_SPAN = 60

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
  const fadeId = useId()
  const maskId = useId()

  const window = points.slice(-WINDOW)
  if (window.length === 0) return null

  const { lo, span, clipped } = domainFor(window)
  const top = lo + span

  const x = (i: number) => (window.length === 1 ? 50 : (i / (window.length - 1)) * 100)
  const y = (v: number) => ((top - v) / span) * 100

  const last = window[window.length - 1]

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
        aria-label={describe(window, reordered, clipped)}
        className="relative w-full"
        style={{ height: HEIGHT }}
      >
        {/* `preserveAspectRatio="none"` lets the 0–100 grid stretch to
            whatever width the card is, with no measuring. Every stroke is
            `non-scaling-stroke` so the stretch can't leave the line thick on
            a wide screen and thin on a narrow one, and there is no text and
            no circle inside the SVG — both would distort, so the end marker
            and all labels are HTML on top. */}
        <svg
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          className="size-full overflow-hidden"
          aria-hidden
        >
          {clipped && (
            <defs>
              {/* Only when the band runs past the edges: fading it out there
                  is how the chart says "this keeps going" without a sentence
                  of copy. A band that fits is drawn flat, so the fade always
                  means something. */}
              <linearGradient id={fadeId} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="black" />
                <stop offset="22%" stopColor="white" />
                <stop offset="78%" stopColor="white" />
                <stop offset="100%" stopColor="black" />
              </linearGradient>
              <mask id={maskId}>
                <rect x="0" y="0" width="100" height="100" fill={`url(#${fadeId})`} />
              </mask>
            </defs>
          )}
          <path
            d={bandPath}
            className="fill-primary/15"
            mask={clipped ? `url(#${maskId})` : undefined}
          />
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

        {/* The current rating, pinned to the end of the line. Shifted by half
            its own size so it sits on the point rather than beside it. */}
        <span
          className="absolute size-2.5 rounded-full border-2 border-background bg-primary"
          style={{
            left: `${x(window.length - 1)}%`,
            top: `${y(last.rating)}%`,
            transform: 'translate(-50%, -50%)',
          }}
        />
      </div>

      <div className="flex items-baseline justify-between gap-2 text-[11px] font-medium text-muted-foreground">
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
 * The y range to draw, and whether the band had to be cut to get it.
 *
 * Scaling to the whole `±` band is the obvious choice and it produces a
 * useless chart. The band is three sigmas wide, which after a player's
 * placements is still something like ±650 — several times the distance their
 * rating has actually travelled. Fit that on the axis and every real
 * movement collapses into a flat line inside a huge stripe, which reads as
 * "nothing happened" to someone whose rating climbed 200 points.
 *
 * So the line gets the axis, and the band is allowed `BAND_CAP` times that
 * much before it is cut off at the edges. When it is cut, the drawing fades
 * it out there rather than ending it in a hard line, so the chart doesn't
 * quietly claim the uncertainty stops where the box does. The exact `±` is
 * never left to the chart anyway — it is printed next to the rating above
 * it, and spelled out in the screen-reader description.
 */
function domainFor(points: RatingHistoryPoint[]): { lo: number; span: number; clipped: boolean } {
  const ratings = points.map((p) => p.rating)
  const lineLo = Math.min(...ratings)
  const lineHi = Math.max(...ratings)
  const mid = (lineLo + lineHi) / 2

  // What the line needs: its own range, plus room to breathe so it never
  // runs along the top or bottom edge.
  const lineSpan = Math.max((lineHi - lineLo) * 1.7, MIN_SPAN)

  const bandLo = Math.min(...points.map((p) => p.floor))
  const bandHi = Math.max(...points.map((p) => p.rating + p.confidence))
  const bandSpan = bandHi - bandLo

  if (bandSpan <= lineSpan * BAND_CAP) {
    // It fits. Show the whole thing — on a settled rating this is the honest
    // picture and no cutting is needed.
    return { lo: bandLo, span: Math.max(bandSpan, MIN_SPAN), clipped: false }
  }

  const span = lineSpan * BAND_CAP
  return { lo: mid - span / 2, span, clipped: true }
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
function describe(points: RatingHistoryPoint[], reordered: number, clipped: boolean): string {
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
  if (clipped) {
    parts.push('The shaded range is wider than the chart shows.')
  }
  if (reordered > 0) {
    parts.push(
      `${reordered} ${reordered === 1 ? 'step is' : 'steps are'} marked as out of order, confirmed after a later match had already counted.`,
    )
  }
  return parts.join(' ')
}
