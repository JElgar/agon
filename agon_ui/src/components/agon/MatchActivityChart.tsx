import { useId, useState } from 'react'
import { cn } from '@/lib/utils'

export interface MonthlyActivityPoint {
  /** Short month label, e.g. "Mar". */
  label: string
  /** Full label for the tooltip, e.g. "March 2026". */
  fullLabel: string
  count: number
}

const WIDTH = 320
const HEIGHT = 120
const BAR_GAP = 8
const TOP_PAD = 16
const BOTTOM_PAD = 20
const BAR_RADIUS = 4

/** A rounded-top bar path: flat baseline, radius-capped top corners. Falls
 *  back to a flat-topped rect when the bar is shorter than the radius. */
function barPath(x: number, y: number, w: number, h: number, r: number): string {
  const rad = Math.min(r, h, w / 2)
  if (rad <= 0) return `M${x},${y + h} h${w} v${-h} h${-w} Z`
  return [
    `M${x},${y + h}`,
    `V${y + rad}`,
    `Q${x},${y} ${x + rad},${y}`,
    `H${x + w - rad}`,
    `Q${x + w},${y} ${x + w},${y + rad}`,
    `V${y + h}`,
    'Z',
  ].join(' ')
}

export interface MatchActivityChartProps {
  data: MonthlyActivityPoint[]
  className?: string
}

/**
 * Matches-per-month bar chart: one thin, rounded-top bar per month, a hover
 * tooltip with the exact count, and month labels below. Single series (bar
 * count, one sport, one player) so no legend — the section heading above this
 * names it. Uses the app's `--chart-1` token, not a new color.
 */
export function MatchActivityChart({ data, className }: MatchActivityChartProps) {
  const [hovered, setHovered] = useState<number | null>(null)
  const gradientId = useId()

  const max = Math.max(1, ...data.map((d) => d.count))
  const plotHeight = HEIGHT - TOP_PAD - BOTTOM_PAD
  const barWidth = data.length > 0 ? (WIDTH - BAR_GAP * (data.length - 1)) / data.length : 0

  const active = hovered != null ? data[hovered] : null
  const activeX = hovered != null ? hovered * (barWidth + BAR_GAP) + barWidth / 2 : 0

  return (
    <div className={cn('relative', className)}>
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="w-full"
        role="img"
        aria-label={`Matches per month: ${data.map((d) => `${d.fullLabel}, ${d.count} match${d.count === 1 ? '' : 'es'}`).join('; ')}`}
      >
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--chart-1)" stopOpacity="1" />
            <stop offset="100%" stopColor="var(--chart-1)" stopOpacity="0.75" />
          </linearGradient>
        </defs>

        {/* Baseline */}
        <line
          x1={0}
          y1={HEIGHT - BOTTOM_PAD}
          x2={WIDTH}
          y2={HEIGHT - BOTTOM_PAD}
          stroke="var(--border)"
          strokeWidth={1}
        />

        {data.map((d, i) => {
          const h = d.count === 0 ? 0 : Math.max(3, (d.count / max) * plotHeight)
          const x = i * (barWidth + BAR_GAP)
          const y = HEIGHT - BOTTOM_PAD - h
          const isHovered = hovered === i
          return (
            <g key={d.fullLabel}>
              {/* Bigger-than-the-mark hit target, per the whole column height. */}
              <rect
                x={x}
                y={TOP_PAD}
                width={barWidth}
                height={HEIGHT - TOP_PAD - BOTTOM_PAD}
                fill="transparent"
                onMouseEnter={() => setHovered(i)}
                onMouseLeave={() => setHovered(null)}
              />
              {d.count > 0 && (
                <path
                  d={barPath(x, y, barWidth, h, BAR_RADIUS)}
                  fill={`url(#${gradientId})`}
                  opacity={isHovered ? 1 : 0.9}
                  className="pointer-events-none transition-opacity"
                />
              )}
              <text
                x={x + barWidth / 2}
                y={HEIGHT - 6}
                textAnchor="middle"
                fontSize={9}
                fill="var(--muted-foreground)"
              >
                {d.label}
              </text>
            </g>
          )
        })}
      </svg>

      {active && (
        <div
          className="pointer-events-none absolute top-0 -translate-x-1/2 -translate-y-full rounded-md border bg-popover px-2 py-1 text-xs whitespace-nowrap text-popover-foreground shadow-sm"
          style={{ left: `${(activeX / WIDTH) * 100}%` }}
        >
          <span className="font-medium">{active.count}</span>{' '}
          {active.count === 1 ? 'match' : 'matches'} · {active.fullLabel}
        </div>
      )}
    </div>
  )
}
