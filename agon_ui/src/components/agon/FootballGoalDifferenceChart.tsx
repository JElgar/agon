import { useState } from 'react'
import type { components } from '@/types/api'
import {
  describeEvent,
  goalDifferenceTimeline,
  goalEventsToViews,
  type FootballGoalEvent,
  type FootballPeriodTimes,
} from '@/lib/liveScore'
import { footballFormat } from '@/lib/matchFormat'
import type { ScorePlayers } from '@/lib/members'
import { cn } from '@/lib/utils'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Below this many total goals the swings this chart is for barely register
 *  — `FootballScorersBySide`'s plain scorer list already covers a normal
 *  scoreline better. Tunable; not a backend contract. */
const MIN_GOALS_FOR_CHART = 9

const VIEW_W = 320
const PAD = { top: 22, right: 16, bottom: 22, left: 16 }
const CENTER_X = VIEW_W / 2
const HALF_PLOT_W = (VIEW_W - PAD.left - PAD.right) / 2
/** Portrait: time runs top-to-bottom, so height scales with the match's own
 *  length rather than sitting fixed like a landscape chart's viewBox. */
const MINUTE_PX = 5.4

function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/**
 * A high-scoring football match's goal difference over time — the
 * "momentum" line turned on its side: minutes run top-to-bottom (kick-off to
 * full-time) and the running score's lead runs left (toward `sideA`) or
 * right (toward `sideB`) from a center line, so a team pulling clear reads
 * as the bar swinging out toward its own half. Each goal gets a minute label
 * pinned to the center axis and a hover tooltip with who scored (and
 * assisted) and the score at that point.
 *
 * Only renders once the match has had enough goals to make the swings worth
 * looking at (`MIN_GOALS_FOR_CHART`) and there's at least one goal with a
 * derivable minute — same goal log `FootballScorersBySide`/
 * `FootballGoalContributions` read, so it stays in sync with those.
 */
export function FootballGoalDifferenceChart({
  goals,
  match,
  players,
  periodTimes,
  sideA,
  sideB,
  className,
}: {
  goals: FootballGoalEvent[]
  match: Match
  players?: ScorePlayers
  /** Lets a live-scored goal's minute be derived the same way the event
   *  timeline's does — see `goalMinuteValue`. */
  periodTimes?: FootballPeriodTimes
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  className?: string
}) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null)

  if (goals.length < MIN_GOALS_FOR_CHART || !sideA || !sideB) return null

  const points = goalDifferenceTimeline(goals, sideA.id, periodTimes)
  if (points.length === 0) return null

  const format = footballFormat(match.format)
  const regulationMinutes = format.half_length_minutes * format.num_halves
  const lastMinute = points[points.length - 1].minute
  const fullTime = Math.max(regulationMinutes, lastMinute)
  const showHalfTime = format.num_halves === 2

  const plotTop = PAD.top
  const plotBottom = PAD.top + fullTime * MINUTE_PX
  const viewH = plotBottom + PAD.bottom
  const maxAbsDiff = Math.max(1, ...points.map((p) => Math.abs(p.diff)))

  const yForMinute = (m: number) => plotTop + (m / fullTime) * (plotBottom - plotTop)
  const xOffset = (diff: number) => (Math.abs(diff) / maxAbsDiff) * HALF_PLOT_W * 0.92
  const outerX = (diff: number) => (diff > 0 ? CENTER_X - xOffset(diff) : CENTER_X + xOffset(diff))

  // Segments between consecutive goals (plus 0→first and last→full-time),
  // each holding the diff that was current for its whole span.
  const steps: { minute: number; diff: number }[] = [{ minute: 0, diff: 0 }, ...points]
  const segments = steps.map((s, i) => ({
    yStart: s.minute,
    yEnd: i + 1 < steps.length ? steps[i + 1].minute : fullTime,
    diff: s.diff,
  }))

  // One filled polygon per unbroken run of same-side segments, not a shape
  // per segment — abutting same-color shapes with different widths leave an
  // antialiasing seam right on the step corner otherwise.
  const runs: { diff: number; path: string }[] = []
  let run: typeof segments = []
  const flushRun = () => {
    if (run.length === 0) return
    const first = run[0]
    const last = run[run.length - 1]
    let d = `M ${CENTER_X} ${yForMinute(first.yStart)}`
    for (const seg of run) {
      const x = outerX(seg.diff)
      d += ` L ${x} ${yForMinute(seg.yStart)} L ${x} ${yForMinute(seg.yEnd)}`
    }
    d += ` L ${CENTER_X} ${yForMinute(last.yEnd)} Z`
    runs.push({ diff: first.diff, path: d })
    run = []
  }
  for (const seg of segments) {
    if (seg.diff === 0) {
      flushRun()
      continue
    }
    if (run.length > 0 && Math.sign(run[run.length - 1].diff) !== Math.sign(seg.diff)) flushRun()
    run.push(seg)
  }
  flushRun()

  function handleMove(e: React.MouseEvent<SVGSVGElement>) {
    const svg = e.currentTarget
    const rect = svg.getBoundingClientRect()
    const relY = ((e.clientY - rect.top) / rect.height) * viewH
    const minute = ((relY - plotTop) / (plotBottom - plotTop)) * fullTime
    let nearest = 0
    let bestDist = Infinity
    points.forEach((p, i) => {
      const dist = Math.abs(p.minute - minute)
      if (dist < bestDist) {
        bestDist = dist
        nearest = i
      }
    })
    setHoverIndex(nearest)
  }

  const hover = hoverIndex !== null ? points[hoverIndex] : null
  const finalScoreA = points[points.length - 1].scoreA
  const finalScoreB = points[points.length - 1].scoreB

  return (
    <div className={cn('rounded-xl border bg-card p-4', className)}>
      <p className="mb-3 text-sm font-medium">Goal difference</p>
      <div className="mb-1 flex items-center justify-between text-xs text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: 'var(--color-chart-1)' }} />
          {sideName(sideA, 'Side A')}{' '}
          <span className="font-medium text-foreground">{finalScoreA}</span>
        </span>
        <span className="flex items-center gap-1.5">
          <span className="font-medium text-foreground">{finalScoreB}</span>{' '}
          {sideName(sideB, 'Side B')}
          <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: 'var(--color-chart-2)' }} />
        </span>
      </div>

      <div className="relative">
        <svg
          viewBox={`0 0 ${VIEW_W} ${viewH}`}
          className="w-full touch-none"
          onMouseMove={handleMove}
          onMouseLeave={() => setHoverIndex(null)}
          role="img"
          aria-label="Goal difference by minute"
        >
          {showHalfTime && (
            <>
              <line
                x1={PAD.left}
                x2={VIEW_W - PAD.right}
                y1={yForMinute(format.half_length_minutes)}
                y2={yForMinute(format.half_length_minutes)}
                stroke="var(--color-border)"
                strokeWidth={1}
                strokeDasharray="3 3"
              />
              <text
                x={VIEW_W - PAD.right}
                y={yForMinute(format.half_length_minutes) - 6}
                textAnchor="end"
                className="fill-muted-foreground text-[9px]"
              >
                HT
              </text>
            </>
          )}
          <text x={PAD.left} y={plotTop - 8} className="fill-muted-foreground text-[9px]">
            KO
          </text>
          <text x={PAD.left} y={plotBottom + 14} className="fill-muted-foreground text-[9px]">
            FT
          </text>

          <line
            x1={CENTER_X}
            x2={CENTER_X}
            y1={plotTop}
            y2={plotBottom}
            stroke="var(--color-border)"
            strokeWidth={1}
          />

          {runs.map((r, i) => (
            <path
              key={i}
              d={r.path}
              fill={r.diff > 0 ? 'var(--color-chart-1)' : 'var(--color-chart-2)'}
            />
          ))}

          {/* Hover crosshair, snapped to the nearest goal's minute. */}
          {hover && (
            <line
              x1={PAD.left}
              x2={VIEW_W - PAD.right}
              y1={yForMinute(hover.minute)}
              y2={yForMinute(hover.minute)}
              stroke="var(--color-muted-foreground)"
              strokeWidth={1}
              strokeDasharray="3 3"
            />
          )}

          {points.map((p, i) => {
            const y = yForMinute(p.minute)
            const x = p.diff === 0 ? CENTER_X : outerX(p.diff)
            const color = p.diff > 0 ? 'var(--color-chart-1)' : p.diff < 0 ? 'var(--color-chart-2)' : 'var(--color-muted-foreground)'
            const label = p.minute + "'"
            const labelW = label.length * 6 + 6
            return (
              <g key={i}>
                {p.diff !== 0 && (
                  <>
                    {/* A colored ring around a foreground-colored center,
                        not a solid colored dot — a goal that narrows the
                        lead (rather than extends it) lands the marker on
                        ground already filled with its own color, where a
                        same-colored dot would camouflage against the fill
                        around it. */}
                    <circle cx={x} cy={y} r={hoverIndex === i ? 6 : 5} fill={color} />
                    <circle cx={x} cy={y} r={hoverIndex === i ? 3 : 2.5} fill="var(--color-card)" />
                  </>
                )}
                <rect
                  x={CENTER_X - labelW / 2}
                  y={y - 7}
                  width={labelW}
                  height={14}
                  rx={7}
                  fill="var(--color-card)"
                  stroke="var(--color-border)"
                  strokeWidth={1}
                />
                <text x={CENTER_X} y={y + 3} textAnchor="middle" className="fill-foreground text-[9px]">
                  {label}
                </text>
              </g>
            )
          })}
        </svg>

        {hover && (
          <div
            className={cn(
              'pointer-events-none absolute rounded-md border bg-popover px-2.5 py-1.5 text-xs shadow-md',
              hover.diff >= 0 ? 'right-2' : 'left-2',
            )}
            style={{ top: `${(yForMinute(hover.minute) / viewH) * 100}%`, transform: 'translateY(-50%)' }}
          >
            <p className="font-medium text-foreground">
              {describeEvent(goalEventsToViews([hover.goal])[0], match, players)}
            </p>
            <p className="text-muted-foreground">
              {hover.minute}&apos; · {hover.scoreA}-{hover.scoreB}
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
