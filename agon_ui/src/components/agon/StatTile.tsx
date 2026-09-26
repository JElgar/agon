import { cn } from '@/lib/utils'

export interface StatTileProps extends React.HTMLAttributes<HTMLDivElement> {
  value: React.ReactNode
  label: string
  /** `inverted` is for placing the tile directly on the primary-blue stat
   *  banner; `default` is for a white/muted surface. */
  tone?: 'default' | 'inverted'
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
      <span
        className={cn(
          'font-display text-3xl leading-none font-extrabold',
          tone === 'inverted' ? 'text-primary-foreground' : 'text-foreground',
        )}
      >
        {value}
      </span>
      <span
        className={cn(
          'text-xs',
          tone === 'inverted' ? 'text-primary-foreground/80' : 'text-muted-foreground',
        )}
      >
        {label}
      </span>
    </div>
  )
}
