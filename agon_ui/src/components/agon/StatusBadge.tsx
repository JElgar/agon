import { TrendingUp, TrendingDown } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'

type Match = components['schemas']['Match']

/**
 * The badge state for a match. Confirmation (confirmed/unconfirmed) only applies
 * to a match that has actually been played; a match still to come is "scheduled"
 * (and "in_progress"/"cancelled" cover the other lifecycle states). Deriving the
 * badge purely from score confirmation would mislabel a scheduled match — which
 * has no score yet — as "Unconfirmed".
 */
export type MatchBadgeStatus =
  | 'scheduled'
  | 'in_progress'
  | 'confirmed'
  | 'unconfirmed'
  | 'cancelled'

/**
 * The badge status for a match: its lifecycle status drives the label, except a
 * completed match distinguishes confirmed vs unconfirmed by whether its score is
 * agreed. A completed match with no confirmed score is "unconfirmed" (awaiting
 * agreement); anything not yet completed shows its lifecycle state.
 */
export function matchBadgeStatus(
  match: Pick<Match, 'status' | 'confirmed_score'>,
): MatchBadgeStatus {
  switch (match.status) {
    case 'scheduled':
      return 'scheduled'
    case 'in_progress':
      return 'in_progress'
    case 'cancelled':
      return 'cancelled'
    case 'completed':
      return match.confirmed_score ? 'confirmed' : 'unconfirmed'
  }
}

const BADGE: Record<MatchBadgeStatus, { label: string; className: string }> = {
  scheduled: {
    label: 'Scheduled',
    className: 'border-primary/30 bg-primary/10 text-primary',
  },
  in_progress: {
    label: 'In progress',
    className: 'border-primary/30 bg-primary/10 text-primary',
  },
  confirmed: {
    label: 'Confirmed',
    className: 'border-success/30 bg-success/10 text-success',
  },
  unconfirmed: {
    label: 'Unconfirmed',
    className: 'border-warning/30 bg-warning/10 text-warning',
  },
  cancelled: {
    label: 'Cancelled',
    className: 'border-muted-foreground/30 bg-muted text-muted-foreground',
  },
}

export function StatusBadge({
  status,
  className,
}: {
  status: MatchBadgeStatus
  className?: string
}) {
  const { label, className: tone } = BADGE[status]
  return (
    <span
      className={cn(
        'inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] font-medium',
        tone,
        className,
      )}
    >
      {label}
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
