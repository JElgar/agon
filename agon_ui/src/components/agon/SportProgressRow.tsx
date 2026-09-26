import { Link } from 'react-router-dom'
import { ChevronRight } from 'lucide-react'
import type { MatchType } from '@/lib/sports'
import { sportIcon, sportLabel, sportTint } from '@/lib/sports'
import { formatWinRate } from '@/lib/stats'
import { cn } from '@/lib/utils'

export interface SportProgressRowProps {
  sport: MatchType
  matchesPlayed: number
  winPercentage: number | null | undefined
  to: string
  isFirst?: boolean
}

/**
 * One row of the profile's "Your sports" list: icon, matches played + win
 * rate, and a win-rate progress bar, linking to that sport's stats page —
 * see the Profile board on the "Agon redesign" canvas.
 */
export function SportProgressRow({
  sport,
  matchesPlayed,
  winPercentage,
  to,
  isFirst,
}: SportProgressRowProps) {
  const Icon = sportIcon(sport)
  const tint = sportTint(sport)
  const pct = Math.max(0, Math.min(100, winPercentage ?? 0))

  return (
    <Link
      to={to}
      className={cn(
        'flex min-h-16 items-center gap-3.5 px-4 py-3.5 text-foreground',
        !isFirst && 'border-t',
      )}
    >
      <span
        className="flex size-11 shrink-0 items-center justify-center rounded-full"
        style={{ background: tint.bg }}
      >
        <Icon className="size-[22px]" style={{ color: tint.fg }} strokeWidth={2} />
      </span>
      <span className="flex flex-grow flex-col gap-1.5">
        <span className="flex items-baseline gap-2">
          <span className="text-[16px] font-bold">{sportLabel(sport)}</span>
          <span className="text-[13px] text-muted-foreground">
            {matchesPlayed} matches &middot; {formatWinRate(winPercentage)} won
          </span>
        </span>
        <span className="h-1.5 overflow-hidden rounded-full bg-muted">
          <span
            className="block h-1.5 rounded-full bg-primary"
            style={{ width: `${pct}%` }}
          />
        </span>
      </span>
      <ChevronRight className="size-5 shrink-0 text-muted-foreground" />
    </Link>
  )
}
