import { cn } from '@/lib/utils'

export interface StatTileProps extends React.HTMLAttributes<HTMLDivElement> {
  value: React.ReactNode
  label: string
  /** `banner-blue`/`banner-terracotta` are for placing the tile on the
   *  matching soft-tint stat banner (its label uses that banner's own muted
   *  tone instead of the generic `--muted-foreground`); `default` is for a
   *  white/muted surface. */
  tone?: 'default' | 'banner-blue' | 'banner-terracotta'
}

/**
 * A big-numeral stat ("11 / Matches played") — the atom every stats
 * treatment in the redesign is built from (the feed's stats banner, the
 * profile sport rows, the football/cricket stats pages).
 */
export function StatTile({
  value,
  label,
  tone = 'default',
  className,
  ...props
}: StatTileProps) {
  return (
    <div className={cn('flex flex-col gap-0.5', className)} {...props}>
      <span className="font-display text-3xl leading-none font-extrabold text-foreground">{value}</span>
      <span
        className={cn(
          'text-xs',
          tone === 'banner-blue' && 'text-banner-tint-muted-foreground',
          tone === 'banner-terracotta' && 'text-banner-terracotta-tint-muted-foreground',
          tone === 'default' && 'text-muted-foreground',
        )}
      >
        {label}
      </span>
    </div>
  )
}
