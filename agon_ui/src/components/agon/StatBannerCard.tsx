import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { StatTile } from './StatTile'

export interface StatBannerCardProps extends React.HTMLAttributes<HTMLElement> {
  stats: { value: React.ReactNode; label: string }[]
  /** `blue` is the brand-primary treatment (overall stats, head-to-head);
   *  `terracotta` is the secondary treatment (playing-together). */
  tone?: 'blue' | 'terracotta'
  /** A `SportBreakdownBar` (or any other footer content) shown below the
   *  stat grid, e.g. the profile's head-to-head/playing-together per-sport
   *  breakdown. */
  footer?: ReactNode
}

/**
 * The full-bleed 3-up stat banner — "All sports"/"Head to head"/"Playing
 * together" on the redesigned Profile board (`Profile.dc.html` on the
 * "Agon redesign" canvas). Distinct from `StatBanner` (the feed's own-stats
 * hero card, a different shape with a greeting/avatar/season summary) —
 * kept as a separate component/file since the two were built independently
 * and serve different layouts, rather than merging them into one
 * overloaded API.
 */
export function StatBannerCard({
  stats,
  tone = 'blue',
  footer,
  className,
  ...props
}: StatBannerCardProps) {
  return (
    <section
      className={cn(
        'flex flex-col gap-4 rounded-2xl border p-[18px] text-foreground',
        tone === 'blue'
          ? 'border-banner-tint-border bg-banner-tint'
          : 'border-banner-terracotta-tint-border bg-banner-terracotta-tint',
        className,
      )}
      {...props}
    >
      <div className="grid grid-cols-3 gap-2">
        {stats.map((s, i) => (
          <StatTile
            key={i}
            value={s.value}
            label={s.label}
            tone={tone === 'blue' ? 'banner-blue' : 'banner-terracotta'}
          />
        ))}
      </div>
      {footer}
    </section>
  )
}
