import { useEffect, useState } from 'react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

type CricketExtraKind = components['schemas']['CricketExtraKind']

/**
 * Picks the run count (and, when there's a choice, the kind) for an extra —
 * every wide/no-ball/bye/leg-bye carries at least 1 run by definition. The
 * mockup's quick-action row has a single "Bye" button, so byes vs leg-byes is
 * folded into this dialog as a toggle rather than a separate grid button.
 * 1-4 are one-tap buttons for the common case; "Other" covers the rarer 5+
 * (an overthrown wide reaching the boundary, say) without capping what can be
 * entered.
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
  const [customRuns, setCustomRuns] = useState('')

  useEffect(() => {
    if (open) {
      setKind(kinds[0]?.value)
      setCustomRuns('')
    }
  }, [open, kinds])

  const custom = Number(customRuns)
  const customValid = customRuns !== '' && Number.isFinite(custom) && custom >= 1

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
          <div className="flex items-center gap-2">
            <Input
              type="number"
              min={1}
              inputMode="numeric"
              placeholder="Other"
              className="h-9"
              value={customRuns}
              onChange={(e) => setCustomRuns(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={submitting || !kind || !customValid}
              onClick={() => kind && onPick(kind, custom)}
            >
              Add
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
