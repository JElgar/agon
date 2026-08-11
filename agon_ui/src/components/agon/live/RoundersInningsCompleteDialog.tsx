import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/**
 * Pops up once the innings looks over by one of its two ending conditions —
 * every batting-order slot out, or the good-ball cap reached (see
 * `allBattersOut`/`goodBallsComplete` in `lib/roundersScore`) — same
 * shape and reasoning as cricket's `AllOutDialog`/`OversCompleteDialog`:
 * nothing on the backend blocks further deliveries (the format's caps are
 * descriptive, not enforced — see `RoundersFormat`'s doc comment), so this
 * surfaces the choice rather than forcing it. "End innings" is the primary
 * action; "Continue" just dismisses the popup and lets scoring carry on.
 */
export function RoundersInningsCompleteDialog({
  open,
  battingSideName,
  reason,
  submitting,
  onEnd,
  onContinue,
}: {
  open: boolean
  battingSideName: string
  reason: 'all_batters_out' | 'good_balls_complete'
  submitting: boolean
  /** Ends the innings now, with the matching reason. */
  onEnd: () => void
  /** Dismisses the popup without ending the innings. */
  onContinue: () => void
}) {
  const title =
    reason === 'all_batters_out' ? `${battingSideName} are all out!` : `${battingSideName}'s good balls are used up`
  const body =
    reason === 'all_batters_out'
      ? 'No batters left to come in — end the innings, or bring someone back in to keep going.'
      : "This innings' good-ball cap has been reached — end it here, or play on past the quota."

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onContinue()}>
      <DialogContent className="max-w-sm text-center sm:text-center">
        <DialogHeader className="text-center sm:text-center">
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">{body}</p>
        <div className="mt-2 flex flex-col gap-2">
          <Button disabled={submitting} onClick={onEnd}>
            {submitting ? 'Ending…' : 'End innings'}
          </Button>
          <Button variant="outline" disabled={submitting} onClick={onContinue}>
            Continue
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
