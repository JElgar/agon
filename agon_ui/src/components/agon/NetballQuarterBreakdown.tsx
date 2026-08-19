import type { components } from '@/types/api'
import { periodLabel, type NetballPeriod, type NetballScore } from '@/lib/netballScore'

type MatchSide = components['schemas']['MatchSide']

/** The quarter-end markers shown here, in play order — `start`/
 *  `extra_time_end` aren't quarter *scores* so they're excluded. */
const QUARTER_MARKERS: NetballPeriod[] = [
  'quarter_one_end',
  'quarter_two_end',
  'quarter_three_end',
  'full_time',
  'extra_time_start',
]

/**
 * Quarter-by-quarter score breakdown — "Q1 12-9, Q2 22-18, ..." — read
 * straight off `NetballScore.period_scores`. Unlike `NetballScorecard`
 * (goal-by-goal detail, event-by-event mode only) this renders identically
 * regardless of which of netball's two live-scoring methods produced the
 * score, since both populate `period_scores` the same way (see its backend
 * doc comment). Renders nothing once there's no quarter marker recorded yet.
 */
export function NetballQuarterBreakdown({
  score,
  sideA,
  sideB,
}: {
  score: NetballScore
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
}) {
  const rows = QUARTER_MARKERS
    .map((period) => ({ period, entry: score.period_scores?.[period] }))
    .filter((row): row is { period: NetballPeriod; entry: Record<string, number> } => !!row.entry)

  if (rows.length === 0) return null

  return (
    <div className="rounded-xl border bg-card p-4">
      <p className="mb-3 text-sm font-medium">Quarter by quarter</p>
      <div className="flex flex-col gap-1.5 text-sm">
        {rows.map(({ period, entry }) => (
          <div key={period} className="flex items-center justify-between">
            <span className="text-muted-foreground">{periodLabel(period)}</span>
            <span className="font-medium tabular-nums">
              {entry[sideA?.id ?? ''] ?? 0} – {entry[sideB?.id ?? ''] ?? 0}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}
