import { Link } from 'react-router-dom'
import { ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'
import { sportIcon, sportLabel } from '@/lib/sports'
import { formatWinRate, sortedByActivity, type SportStatsEntry, type UserStats } from '@/lib/stats'

function StatCell({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center border-r px-1.5 py-3 text-center last:border-r-0">
      <div className="text-lg font-medium">{value}</div>
      <div className="mt-0.5 text-[10px] text-muted-foreground">{label}</div>
    </div>
  )
}

interface SportRowProps {
  entry: SportStatsEntry
  /** Base profile path this table is rendered under (`/profile` or
   *  `/users/:userId`) — the row links to `${statsBasePath}/stats/:sport`. */
  statsBasePath: string
}

function SportRow({ entry, statsBasePath }: SportRowProps) {
  const { sport, stats } = entry
  const Icon = sportIcon(sport)
  return (
    <Link
      to={`${statsBasePath}/stats/${sport}`}
      className="flex items-stretch border-b transition-colors last:border-b-0 hover:bg-accent/50"
    >
      <div className="flex flex-[1.4] items-center gap-2 border-r px-3.5 py-3">
        <span className="flex size-7 items-center justify-center rounded-lg bg-accent">
          <Icon className="size-4 text-accent-foreground" />
        </span>
        <span className="text-sm font-medium">{sportLabel(sport)}</span>
      </div>
      <StatCell value={String(stats.matches_played)} label="Matches" />
      <StatCell value={formatWinRate(stats.win_percentage)} label="Win rate" />
      <div className="flex items-center justify-center px-2 text-muted-foreground">
        <ChevronRight className="size-4" />
      </div>
    </Link>
  )
}

export interface SportStatsTableProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'onOpen'> {
  stats: UserStats
  /** Base profile path this table is rendered under — see `SportRowProps`. */
  statsBasePath: string
  /** Cap the number of sports shown (e.g. top 3 on the profile summary). */
  limit?: number
}

/**
 * Per-sport stats table (icon, sport, matches played, win rate), one row per
 * sport, most-active first. Each row opens that sport's full stats page.
 * Renders nothing when there are no stats.
 */
export function SportStatsTable({
  stats,
  statsBasePath,
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
      {shown.map((entry) => (
        <SportRow key={entry.sport} entry={entry} statsBasePath={statsBasePath} />
      ))}
    </div>
  )
}
