import { TrendingUp, TrendingDown } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * Score confirmation state of a match, mapped to a small coloured pill.
 * "confirmed" = both sides agreed; "unconfirmed" = a pending/awaited result.
 */
export type MatchConfirmation = 'confirmed' | 'unconfirmed'

export function StatusBadge({
  status,
  className,
}: {
  status: MatchConfirmation
  className?: string
}) {
  const confirmed = status === 'confirmed'
  return (
    <span
      className={cn(
        'inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] font-medium',
        confirmed
          ? 'border-success/30 bg-success/10 text-success'
          : 'border-warning/30 bg-warning/10 text-warning',
        className,
      )}
    >
      {confirmed ? 'Confirmed' : 'Unconfirmed'}
    </span>
  )
}

/**
 * Elo/rating delta pill (e.g. "+14" / "−8"). Green for a gain, red for a loss.
 * `delta` is the signed points change.
 */
export function EloBadge({
  delta,
  className,
}: {
  delta: number
  className?: string
}) {
  const positive = delta >= 0
  const Icon = positive ? TrendingUp : TrendingDown
  return (
    <span
      className={cn(
        'inline-flex items-center gap-0.5 text-[11px] font-medium',
        positive ? 'text-success' : 'text-destructive',
        className,
      )}
    >
      <Icon className="size-3" />
      {positive ? `+${delta}` : delta}
    </span>
  )
}
