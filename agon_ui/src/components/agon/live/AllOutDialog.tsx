import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/**
 * Pops up the moment a wicket falls that leaves the batting side with no
 * fresh batter to bring in (see `CricketLiveScoringPage`'s `allOut`) — the
 * innings is over in the ordinary sense, but same as `target_reached` this
 * doesn't auto-end itself: the scorer might still want to keep going (a
 * practice match ignoring the rule, or undoing a wrongly-recorded
 * dismissal without deleting the event). "End innings" is the primary
 * action; "Continue" is the secondary, deliberately understated one — it
 * just dismisses the popup, and the batter picker underneath lets a
 * previously-out player be picked straight back in (skipping the normal
 * "show players who've already batted" toggle, since with nobody else left
 * there's no other choice to offer).
 */
export function AllOutDialog({
  open,
  battingSideName,
  submitting,
  onEnd,
  onContinue,
}: {
  open: boolean
  battingSideName: string
  submitting: boolean
  /** Ends the innings now, with reason `all_out`. */
  onEnd: () => void
  /** Dismisses the popup without ending the innings — the batter picker
   *  underneath takes over from there. */
  onContinue: () => void
}) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onContinue()}>
      <DialogContent className="max-w-sm text-center sm:text-center">
        <DialogHeader className="text-center sm:text-center">
          <DialogTitle>{battingSideName} are all out!</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          No batters left to come in — end the innings, or bring someone who's
          already batted back in to keep going.
        </p>
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
