import { cn } from '@/lib/utils'
import { sportIcon, sportLabel, type MatchType } from '@/lib/sports'

export interface SportBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  sport: MatchType
  /** Show the sport's icon alongside the label. Off by default (pill is text-only). */
  withIcon?: boolean
}

/** The blue sport pill (e.g. "TENNIS") used on match cards and detail headers. */
export function SportBadge({
  sport,
  withIcon = false,
  className,
  ...props
}: SportBadgeProps) {
  const Icon = sportIcon(sport)
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1 rounded bg-accent px-2 py-0.5 text-[10px] font-medium uppercase tracking-wider text-accent-foreground',
        className,
      )}
      {...props}
    >
      {withIcon && <Icon className="size-3" />}
      {sportLabel(sport)}
    </span>
  )
}
