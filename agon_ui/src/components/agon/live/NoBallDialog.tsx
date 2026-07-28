import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'

/**
 * A no-ball is the one extra where the batter can still score off the bat in
 * addition to the mandatory penalty run — unlike a wide (never faced) or a
 * bye/leg-bye (nothing off the bat by definition). This asks for runs off the
 * bat (0-6); the mandatory penalty itself is fixed at 1 run for now — making
 * that configurable (some formats use 2) is part of the larger match-format
 * settings this doesn't attempt yet.
 */
export function NoBallDialog({
  open,
  penaltyRuns,
  onOpenChange,
  onPick,
  submitting,
}: {
  open: boolean
  /** The configured no-ball penalty (see `lib/matchFormat`'s
   *  `no_ball_penalty_runs`, 1 by default). */
  penaltyRuns: number
  onOpenChange: (open: boolean) => void
  onPick: (runsOffBat: number) => void
  submitting: boolean
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xs">
        <DialogHeader>
          <DialogTitle>No ball — runs off the bat</DialogTitle>
        </DialogHeader>
        <div className="grid grid-cols-4 gap-2">
          {[0, 1, 2, 3, 4, 6].map((n) => (
            <button
              key={n}
              type="button"
              disabled={submitting}
              onClick={() => onPick(n)}
              className={cn(
                'rounded-lg border p-3 text-sm font-semibold transition-colors hover:bg-muted disabled:opacity-50',
                (n === 4 || n === 6) && 'border-primary/30 bg-primary/10 text-primary',
              )}
            >
              {n}
            </button>
          ))}
        </div>
        <p className="text-xs text-muted-foreground">
          Plus the mandatory {penaltyRuns}-run penalty, charged to the bowler either way.
        </p>
      </DialogContent>
    </Dialog>
  )
}
