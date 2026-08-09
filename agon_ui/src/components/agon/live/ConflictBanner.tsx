import { useState } from 'react'
import { Button } from '@/components/ui/button'

/**
 * Shown when offline sync detects a genuine conflict — the server's log has
 * moved on in a way that doesn't match this device's own queued backlog
 * (not just a lost response to its own retry, which is resolved
 * automatically — see `useOfflineLiveScoring.ts`'s `reconcileConflict`).
 * No auto-merge: the app's live scoring is built around one active scorer
 * at a time (see `amend_live_event`'s doc comment on the backend), so the
 * only offered resolution is discarding this device's unsynced backlog and
 * adopting whatever the server has instead. Requires an explicit
 * confirmation tap, since discarding is real data loss for this device.
 */
export function ConflictBanner({
  pendingCount,
  onDiscardAndReload,
}: {
  pendingCount: number
  onDiscardAndReload: () => void
}) {
  const [confirming, setConfirming] = useState(false)

  return (
    <div className="rounded-xl border border-destructive/30 bg-destructive/5 p-4">
      <p className="text-sm font-medium">This match was scored elsewhere while you were offline</p>
      <p className="mt-1 text-xs text-muted-foreground">
        {pendingCount} event{pendingCount === 1 ? '' : 's'} recorded on this device couldn't be
        synced automatically. Discard them and reload the current score, or keep them queued and
        try again later.
      </p>
      {confirming ? (
        <div className="mt-3 flex gap-2">
          <Button variant="destructive" size="sm" onClick={onDiscardAndReload}>
            Discard & reload
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
            Cancel
          </Button>
        </div>
      ) : (
        <Button variant="outline" size="sm" className="mt-3" onClick={() => setConfirming(true)}>
          Discard my offline events…
        </Button>
      )}
    </div>
  )
}
