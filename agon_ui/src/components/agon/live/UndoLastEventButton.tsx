import { useState } from 'react'
import { Undo2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useUndoLastLiveEvent } from '@/hooks/useLiveScore'

/**
 * "Undo" for the live scoring screens (football and cricket alike) —
 * deletes the most recently recorded event outright via
 * `useUndoLastLiveEvent`. That's the only correction the backend supports:
 * no arbitrary-position edits, just undo-the-tip (see `delete_live_event`'s
 * doc comment on the server) — so this is deliberately a single "undo last"
 * action rather than a per-event delete button in the event log.
 *
 * Renders nothing once there's no `seq` yet (scoring hasn't started on this
 * match) — same "nothing to undo" gate the disabled state would otherwise
 * need to express. Confirms first since this genuinely deletes history
 * rather than marking it undone; there's no redo.
 */
export function UndoLastEventButton({
  matchId,
  seq,
}: {
  matchId: string
  /** The log's current tip, e.g. from `useLiveSeq` — `undefined` while
   *  still loading, `0` before anything's been recorded. */
  seq: number | undefined
}) {
  const [confirmOpen, setConfirmOpen] = useState(false)
  const undo = useUndoLastLiveEvent(matchId)

  if (!seq) return null

  return (
    <>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="text-muted-foreground"
        onClick={() => setConfirmOpen(true)}
      >
        <Undo2 className="size-3.5" /> Undo last
      </Button>

      <Dialog open={confirmOpen} onOpenChange={(open) => !undo.isPending && setConfirmOpen(open)}>
        <DialogContent className="max-w-sm text-center sm:text-center">
          <DialogHeader className="text-center sm:text-center">
            <DialogTitle>Undo the last event?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            This removes the most recently recorded event for good — there's no redo.
          </p>
          {undo.isError && (
            <p className="text-xs text-destructive">{(undo.error as Error).message}</p>
          )}
          <div className="mt-2 flex flex-col gap-2">
            <Button
              type="button"
              variant="destructive"
              disabled={undo.isPending}
              onClick={() => undo.mutate(seq, { onSuccess: () => setConfirmOpen(false) })}
            >
              {undo.isPending ? 'Undoing…' : 'Yes, undo it'}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={undo.isPending}
              onClick={() => setConfirmOpen(false)}
            >
              Cancel
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
