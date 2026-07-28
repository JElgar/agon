import { useEffect, useState } from 'react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

type CricketExtraKind = components['schemas']['CricketExtraKind']

/**
 * Picks the run count (and, when there's a choice, the kind) for an extra —
 * every wide/no-ball/bye/leg-bye carries at least 1 run by definition. The
 * mockup's quick-action row has a single "Bye" button, so byes vs leg-byes is
 * folded into this dialog as a toggle rather than a separate grid button.
 * Capped to a simple 1-4 picker; a rarer 5+ run extra can be corrected later
 * via the existing delete/amend-by-seq API (no dedicated UI for that yet).
 */
export function ExtraRunsDialog({
  open,
  title,
  kinds,
  onOpenChange,
  onPick,
  submitting,
}: {
  open: boolean
  title: string
  kinds: { value: CricketExtraKind; label: string }[]
  onOpenChange: (open: boolean) => void
  onPick: (kind: CricketExtraKind, runs: number) => void
  submitting: boolean
}) {
  const [kind, setKind] = useState<CricketExtraKind | undefined>(kinds[0]?.value)

  useEffect(() => {
    if (open) setKind(kinds[0]?.value)
  }, [open, kinds])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xs">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          {kinds.length > 1 && (
            <div className="grid grid-cols-2 gap-2">
              {kinds.map((k) => (
                <button
                  key={k.value}
                  type="button"
                  aria-pressed={kind === k.value}
                  onClick={() => setKind(k.value)}
                  className={cn(
                    'rounded-lg border p-2 text-sm font-medium transition-colors',
                    kind === k.value
                      ? 'border-primary bg-accent text-accent-foreground'
                      : 'text-muted-foreground hover:bg-muted',
                  )}
                >
                  {k.label}
                </button>
              ))}
            </div>
          )}
          <div className="grid grid-cols-4 gap-2">
            {[1, 2, 3, 4].map((runs) => (
              <Button
                key={runs}
                variant="outline"
                disabled={submitting || !kind}
                onClick={() => kind && onPick(kind, runs)}
              >
                {runs}
              </Button>
            ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
