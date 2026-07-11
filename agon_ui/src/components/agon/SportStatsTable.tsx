import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { sportIcon, sportLabel } from '@/lib/sports'
import { formatWinRate, sortedByActivity } from '@/lib/stats'

type UserSportStats = components['schemas']['UserSportStats']

function StatCell({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center border-r px-1.5 py-3 text-center last:border-r-0">
      <div className="text-lg font-medium">{value}</div>
      <div className="mt-0.5 text-[10px] text-muted-foreground">{label}</div>
    </div>
  )
}

function SportRow({ stat }: { stat: UserSportStats }) {
  const Icon = sportIcon(stat.match_type)
  return (
    <div className="flex items-stretch border-b last:border-b-0">
      <div className="flex flex-[1.4] items-center gap-2 border-r px-3.5 py-3">
        <span className="flex size-7 items-center justify-center rounded-lg bg-accent">
          <Icon className="size-4 text-accent-foreground" />
        </span>
        <span className="text-sm font-medium">{sportLabel(stat.match_type)}</span>
      </div>
      <StatCell value={String(stat.matches_played)} label="Matches" />
      <StatCell value={formatWinRate(stat.win_percentage)} label="Win rate" />
    </div>
  )
}

export interface SportStatsTableProps
  extends React.HTMLAttributes<HTMLDivElement> {
  stats: UserSportStats[]
  /** Cap the number of sports shown (e.g. top 3 on the profile summary). */
  limit?: number
}

/**
 * Per-sport stats table (icon, sport, matches played, win rate), one row per
 * sport, most-active first. Renders nothing when there are no stats.
 */
export function SportStatsTable({
  stats,
  limit,
  className,
  ...props
}: SportStatsTableProps) {
  const rows = sortedByActivity(stats)
  const shown = limit ? rows.slice(0, limit) : rows

  if (shown.length === 0) {
    return (
      <div
        className={cn(
          'rounded-xl border bg-card p-6 text-center text-sm text-muted-foreground',
          className,
        )}
        {...props}
      >
        No matches played yet.
      </div>
    )
  }

  return (
    <div
      className={cn('overflow-hidden rounded-xl border bg-card', className)}
      {...props}
    >
      {shown.map((stat) => (
        <SportRow key={stat.match_type} stat={stat} />
      ))}
    </div>
  )
}
