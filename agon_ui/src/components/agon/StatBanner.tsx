import { cn } from '@/lib/utils'
import { StatTile } from './StatTile'

export interface StatBannerProps extends React.HTMLAttributes<HTMLElement> {
  stats: { value: React.ReactNode; label: string }[]
  /** `blue` is the brand-primary treatment (overall stats, head-to-head);
   *  `terracotta` is the secondary treatment (playing-together). */
  tone?: 'blue' | 'terracotta'
  footnote?: string
}

/**
 * The full-bleed 3-up stat banner — "All sports"/"Head to head"/"Playing
 * together" on the redesigned Profile board (`Profile.dc.html` on the
 * "Agon redesign" canvas), and the feed's own-stats banner. `tone`
 * "terracotta" is the "playing together" banner's warmer variant; every
 * other banner in the design set uses brand blue ("option A" of the
 * `StatsOptions` board).
 */
export function StatBanner({
  stats,
  tone = 'blue',
  footnote,
  className,
  ...props
}: StatBannerProps) {
  return (
    <section
      className={cn(
        'grid grid-cols-3 gap-2 rounded-2xl p-[18px] text-primary-foreground',
        tone === 'blue' ? 'bg-primary' : 'bg-destructive',
        className,
      )}
      {...props}
    >
      {stats.map((s, i) => (
        <StatTile key={i} value={s.value} label={s.label} tone="inverted" />
      ))}
      {footnote && (
        <span className="col-span-3 mt-0.5 text-xs text-primary-foreground/80">
          {footnote}
        </span>
      )}
    </section>
  )
}
