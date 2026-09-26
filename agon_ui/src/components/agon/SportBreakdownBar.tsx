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

/** The banner's own background (`--primary`/`--destructive`) is identical in
 *  light/dark mode, so a fixed white-first segment reads fine in both — but
 *  it's still resolved through `--primary-foreground` (equal to white) and
 *  the `--banner-*-accent` tokens (`index.css`) rather than inlined hex, per
 *  the "tokens only" rule. Only two sports are designed (the "white first,
 *  tinted after" treatment from `StatsOptions.dc.html`, option A); a third+
 *  sport fades the same white token via `color-mix` rather than inventing an
 *  undesigned color. */
function segmentColor(tone: 'blue' | 'terracotta', index: number): string {
  if (index === 0) return 'var(--primary-foreground)'
  if (index === 1) return tone === 'blue' ? 'var(--banner-blue-accent)' : 'var(--banner-terracotta-accent)'
  const opacity = index === 2 ? 55 : 30
  return `color-mix(in oklch, var(--primary-foreground) ${opacity}%, transparent)`
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

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <div className="flex h-2 gap-[3px] overflow-hidden rounded-full">
        {entries.map((e, i) => (
          <span
            key={e.sport}
            className="h-2 rounded-full"
            style={{
              width: `${(e.count / total) * 100}%`,
              background: segmentColor(tone, i),
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
              style={{ background: segmentColor(tone, i) }}
            />
            {sportLabel(e.sport)} {e.count}
          </span>
        ))}
      </div>
    </div>
  )
}
