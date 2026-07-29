import { useState } from 'react'
import { cn } from '@/lib/utils'
import type { RunProgressionPoint } from '@/lib/cricketScore'

export interface RunRateSeries {
  key: string
  label: string
  /** A `--color-chart-N` CSS variable reference, e.g. `var(--color-chart-1)`. */
  color: string
  points: RunProgressionPoint[]
  finalRuns: number
  finalWickets: number
}

const VIEW_W = 600
const VIEW_H = 220
const PAD = { top: 10, right: 12, bottom: 24, left: 32 }
const PLOT_W = VIEW_W - PAD.left - PAD.right
const PLOT_H = VIEW_H - PAD.top - PAD.bottom

/** Rounds up to a "nice" step for axis ticks (1/2/5 × a power of ten). */
function niceCeil(value: number): number {
  if (value <= 0) return 10
  const magnitude = 10 ** Math.floor(Math.log10(value))
  const steps = [1, 2, 2.5, 5, 10]
  const step = steps.find((s) => value <= s * magnitude) ?? 10
  return step * magnitude
}

function xForOvers(overs: number, maxOvers: number): number {
  return PAD.left + (overs / maxOvers) * PLOT_W
}

function yForRuns(runs: number, maxRuns: number): number {
  return PAD.top + PLOT_H - (runs / maxRuns) * PLOT_H
}

/** Nearest point (and its index) in a series to a given overs value. */
function nearestIndex(points: RunProgressionPoint[], overs: number): number {
  let best = 0
  let bestDist = Infinity
  points.forEach((p, i) => {
    const dist = Math.abs(p.overs - overs)
    if (dist < bestDist) {
      bestDist = dist
      best = i
    }
  })
  return best
}

/**
 * A run-progression ("worm") graph: cumulative runs against overs, one line
 * per innings, with a hover crosshair reading off every series at once.
 * Every value shown here is also in the batting/bowling tables alongside it —
 * this chart is a read, not the only way to get the numbers.
 */
export function CricketRunRateChart({ series }: { series: RunRateSeries[] }) {
  const [hoverOvers, setHoverOvers] = useState<number | null>(null)

  const nonEmpty = series.filter((s) => s.points.length > 0)
  if (nonEmpty.length === 0) return null

  const maxOvers = niceCeil(Math.max(1, ...nonEmpty.map((s) => s.points.at(-1)?.overs ?? 0)))
  const maxRuns = niceCeil(Math.max(1, ...nonEmpty.map((s) => s.finalRuns)))

  const runTicks = 4
  const overTickStep = maxOvers <= 10 ? 2 : maxOvers <= 25 ? 5 : 10

  function handleMove(e: React.MouseEvent<SVGSVGElement>) {
    const svg = e.currentTarget
    const rect = svg.getBoundingClientRect()
    const relX = ((e.clientX - rect.left) / rect.width) * VIEW_W
    const overs = Math.max(0, Math.min(maxOvers, ((relX - PAD.left) / PLOT_W) * maxOvers))
    setHoverOvers(overs)
  }

  const hoverX = hoverOvers === null ? null : xForOvers(hoverOvers, maxOvers)

  return (
    <div>
      {nonEmpty.length > 1 && (
        <div className="mb-2 flex flex-wrap items-center gap-x-4 gap-y-1">
          {nonEmpty.map((s) => (
            <div key={s.key} className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                className="h-0.5 w-3 shrink-0 rounded-full"
                style={{ backgroundColor: s.color }}
              />
              {s.label}
              <span className="font-medium text-foreground">
                {s.finalRuns}/{s.finalWickets}
              </span>
            </div>
          ))}
        </div>
      )}

      <div className="relative">
      <svg
        viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
        className="w-full touch-none"
        onMouseMove={handleMove}
        onMouseLeave={() => setHoverOvers(null)}
        role="img"
        aria-label="Run progression by over"
      >
        {/* Horizontal gridlines + y-axis run labels. */}
        {Array.from({ length: runTicks + 1 }, (_, i) => {
          const runs = (maxRuns / runTicks) * i
          const y = yForRuns(runs, maxRuns)
          return (
            <g key={i}>
              <line
                x1={PAD.left}
                x2={VIEW_W - PAD.right}
                y1={y}
                y2={y}
                stroke="var(--color-border)"
                strokeWidth={1}
              />
              <text x={PAD.left - 6} y={y} textAnchor="end" dominantBaseline="middle" className="fill-muted-foreground text-[9px]">
                {Math.round(runs)}
              </text>
            </g>
          )
        })}

        {/* X-axis over labels. */}
        {Array.from({ length: Math.floor(maxOvers / overTickStep) + 1 }, (_, i) => {
          const overs = i * overTickStep
          const x = xForOvers(overs, maxOvers)
          return (
            <text
              key={i}
              x={x}
              y={VIEW_H - PAD.bottom + 14}
              textAnchor="middle"
              className="fill-muted-foreground text-[9px]"
            >
              {overs}
            </text>
          )
        })}

        {/* Hover crosshair. */}
        {hoverX !== null && (
          <line
            x1={hoverX}
            x2={hoverX}
            y1={PAD.top}
            y2={VIEW_H - PAD.bottom}
            stroke="var(--color-muted-foreground)"
            strokeWidth={1}
            strokeDasharray="3 3"
          />
        )}

        {nonEmpty.map((s) => {
          const pathPoints = [{ overs: 0, runs: 0 }, ...s.points]
          const d = pathPoints
            .map((p, i) => `${i === 0 ? 'M' : 'L'} ${xForOvers(p.overs, maxOvers)} ${yForRuns(p.runs, maxRuns)}`)
            .join(' ')
          const last = pathPoints[pathPoints.length - 1]

          return (
            <g key={s.key}>
              <path d={d} fill="none" stroke={s.color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
              {/* Wicket markers: distinct ring shape, not a new hue. */}
              {s.points
                .filter((p) => p.isWicket)
                .map((p, i) => (
                  <circle
                    key={i}
                    cx={xForOvers(p.overs, maxOvers)}
                    cy={yForRuns(p.runs, maxRuns)}
                    r={4}
                    fill="var(--color-card)"
                    stroke={s.color}
                    strokeWidth={2}
                  />
                ))}
              {/* End marker. */}
              <circle
                cx={xForOvers(last.overs, maxOvers)}
                cy={yForRuns(last.runs, maxRuns)}
                r={4}
                fill={s.color}
                stroke="var(--color-card)"
                strokeWidth={2}
              />
            </g>
          )
        })}
      </svg>

      {hoverOvers !== null && (
        <div
          className={cn(
            'pointer-events-none absolute top-1 rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md',
            hoverOvers / maxOvers > 0.7 ? 'right-2' : 'left-2',
          )}
        >
          <p className="mb-1 font-medium text-foreground">Over {hoverOvers.toFixed(1)}</p>
          {nonEmpty.map((s) => {
            const idx = nearestIndex(s.points, hoverOvers)
            const p = s.points[idx]
            return (
              <p key={s.key} className="flex items-center gap-1.5 text-muted-foreground">
                <span className="h-0.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: s.color }} />
                <span className="font-medium text-foreground">
                  {p ? `${p.runs}/${p.wickets}` : '0/0'}
                </span>
                {nonEmpty.length > 1 && s.label}
              </p>
            )
          })}
        </div>
      )}
      </div>
    </div>
  )
}
