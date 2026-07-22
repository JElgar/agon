import { cn } from '@/lib/utils'
import { sportEmoji, sportLabel, type MatchType } from '@/lib/sports'

export interface SportBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  sport: MatchType
}

/** The sport pill (e.g. "🎾 Tennis") used on match cards and detail headers. */
export function SportBadge({ sport, className, ...props }: SportBadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-full bg-muted px-2.5 py-1 text-xs font-medium text-foreground',
        className,
      )}
      {...props}
    >
      <span aria-hidden>{sportEmoji(sport)}</span>
      {sportLabel(sport)}
    </span>
  )
}
