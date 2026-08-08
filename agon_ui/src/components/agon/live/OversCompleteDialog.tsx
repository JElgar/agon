import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/**
 * Pops up the moment the innings' over quota is used up (see
 * `CricketLiveScoringPage`'s `oversComplete`) — same shape as
 * `TargetReachedDialog`/`AllOutDialog`: detected, but not auto-ended,
 * since the scorer might want to play on past the format's quota (extra
 * overs agreed on the day, a rain-affected restart, etc.) rather than end
 * the innings there. "End innings" is the primary action; "Continue" is
 * the secondary one — it just dismisses the popup, and the bowler picker
 * underneath carries on as normal for the next over, unbounded, for the
 * rest of this innings (see `oversContinued`'s doc comment).
 */
export function OversCompleteDialog({
  open,
  battingSideName,
  oversPerInnings,
  submitting,
  onEnd,
  onContinue,
}: {
  open: boolean
  battingSideName: string
  /** The format's configured overs-per-innings quota that's just been used
   *  up — always set by the time this can be open (see `oversComplete`). */
  oversPerInnings: number
  submitting: boolean
  /** Ends the innings now, with reason `overs_complete`. */
  onEnd: () => void
  /** Dismisses the popup without ending the innings — the bowler picker
   *  underneath takes over from there. */
  onContinue: () => void
}) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onContinue()}>
      <DialogContent className="max-w-sm text-center sm:text-center">
        <DialogHeader className="text-center sm:text-center">
          <DialogTitle>Overs complete</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          {battingSideName} have batted all {oversPerInnings} overs. End the innings, or continue
          with more overs.
        </p>
        <div className="mt-2 flex flex-col gap-2">
          <Button disabled={submitting} onClick={onEnd}>
            {submitting ? 'Ending…' : 'End innings'}
          </Button>
          <Button variant="outline" disabled={submitting} onClick={onContinue}>
            Continue with more overs
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
