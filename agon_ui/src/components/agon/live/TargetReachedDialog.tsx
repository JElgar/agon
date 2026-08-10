import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/**
 * Pops up the moment the chasing side passes the target in the match's last
 * innings (see `lib/cricketScore`'s `targetReached`) — the win is already
 * decided, but the innings itself isn't over yet, so the scorer picks what
 * happens next rather than the app deciding for them: play the over out to
 * a natural finish, or end the innings right here. Either way the match
 * isn't marked `completed` from this dialog — same as `overs_complete`, that
 * still goes through "Finish match" once the scorer's reviewed the score.
 */
export function TargetReachedDialog({
  open,
  battingSideName,
  wicketsRemaining,
  submitting,
  onPlayOn,
  onFinish,
}: {
  open: boolean
  battingSideName: string
  wicketsRemaining: number
  submitting: boolean
  /** Dismisses the popup without ending the innings — deliveries keep
   *  recording normally, this device just won't be prompted again for this
   *  same innings. */
  onPlayOn: () => void
  /** Ends the innings now, with reason `target_reached`. */
  onFinish: () => void
}) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onPlayOn()}>
      <DialogContent className="max-w-sm text-center sm:text-center">
        <DialogHeader className="text-center sm:text-center">
          <DialogTitle>{battingSideName} have won!</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          {battingSideName} have reached the target with {wicketsRemaining} wicket
          {wicketsRemaining === 1 ? '' : 's'} in hand.
        </p>
        <div className="mt-2 flex flex-col gap-2">
          <Button disabled={submitting} onClick={onFinish}>
            {submitting ? 'Finishing…' : 'Finish the game'}
          </Button>
          <Button variant="outline" disabled={submitting} onClick={onPlayOn}>
            Play out the overs
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
