import type { MatchType } from '@/lib/sports'
import { sportLabel } from '@/lib/sports'
import { cn } from '@/lib/utils'

export interface SportBreakdownBarProps {
  /** Per-sport counts, already sorted (largest segment first — the mock
   *  puts the most-played sport first in both the bar and its legend). */
  entries: { sport: MatchType; count: number }[]
  tone?: 'blue' | 'terracotta'
  className?: string
}

/** Segment colors for the stacked bar, one array per banner tone — the same
 *  "white first, tinted after" treatment as the feed's stats banner
 *  (`StatsOptions.dc.html`, option A) and the Profile board's head-to-head /
 *  playing-together banners. Only two sports are designed; a third+ sport
 *  falls back to a fading tint of the same family rather than an
 *  undesigned color. */
const PALETTES: Record<'blue' | 'terracotta', string[]> = {
  blue: ['#FFFFFF', '#FFC9A8', 'rgba(255,255,255,0.55)', 'rgba(255,255,255,0.3)'],
  terracotta: ['#FFFFFF', '#FFE0C7', 'rgba(255,255,255,0.55)', 'rgba(255,255,255,0.3)'],
}

/**
 * The stacked matches-per-sport bar + legend shown under a stat banner's
 * headline numbers — e.g. "Football 8 / Cricket 3" under the feed's stats
 * banner, and under the profile's Head to head / Playing together banners
 * (per James's 2026-09-26 request to add it there too).
 */
export function SportBreakdownBar({ entries, tone = 'blue', className }: SportBreakdownBarProps) {
  const total = entries.reduce((sum, e) => sum + e.count, 0)
  if (total === 0) return null
  const palette = PALETTES[tone]

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <div className="flex h-2 gap-[3px] overflow-hidden rounded-full">
        {entries.map((e, i) => (
          <span
            key={e.sport}
            className="h-2 rounded-full"
            style={{
              width: `${(e.count / total) * 100}%`,
              background: palette[i % palette.length],
              flexGrow: i === entries.length - 1 ? 1 : 0,
            }}
          />
        ))}
      </div>
      <div className="flex flex-wrap gap-4 text-[13px] text-primary-foreground/80">
        {entries.map((e, i) => (
          <span key={e.sport} className="flex items-center gap-1.5">
            <span
              className="size-2 shrink-0 rounded-full"
              style={{ background: palette[i % palette.length] }}
            />
            {sportLabel(e.sport)} {e.count}
          </span>
        ))}
      </div>
    </div>
  )
}
