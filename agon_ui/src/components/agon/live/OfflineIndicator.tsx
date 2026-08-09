import { CloudOff, RefreshCw, TriangleAlert } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * Small pill reporting the offline live-scoring queue's state — alongside
 * `LiveIndicator` ("LIVE") on both live-scoring pages. Renders nothing once
 * there's nothing to report (fully synced, no conflict) so it doesn't add
 * visual noise to the common online case.
 */
export function OfflineIndicator({
  isOffline,
  pendingCount,
  syncing,
  conflict,
  onSyncNow,
  className,
}: {
  isOffline: boolean
  pendingCount: number
  syncing: boolean
  conflict: boolean
  onSyncNow: () => void
  className?: string
}) {
  // Just a status marker here — the actual resolution action (discard local
  // events & reload) lives in `ConflictBanner`, rendered prominently in the
  // page body rather than tucked into this corner pill, since it's a
  // decision with real data-loss risk if missed.
  if (conflict) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1 rounded border border-destructive/30 bg-destructive/10 px-1.5 py-0.5 text-[10px] font-medium text-destructive',
          className,
        )}
      >
        <TriangleAlert className="size-3" />
        Sync conflict
      </span>
    )
  }

  if (pendingCount === 0 && !isOffline && !syncing) return null

  return (
    <button
      type="button"
      onClick={onSyncNow}
      disabled={isOffline}
      className={cn(
        'inline-flex items-center gap-1 rounded border border-muted-foreground/30 bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground disabled:cursor-default',
        className,
      )}
    >
      {syncing ? <RefreshCw className="size-3 animate-spin" /> : <CloudOff className="size-3" />}
      {isOffline
        ? `Offline${pendingCount > 0 ? ` · ${pendingCount} pending` : ''}`
        : syncing
          ? 'Syncing…'
          : `${pendingCount} pending`}
    </button>
  )
}
