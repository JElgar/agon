import { useEffect, useState } from 'react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { PlayerPicker } from './Pickers'
import { creditsBowler, dismissalLabel, type CricketDismissalKind } from '@/lib/cricketScore'
import { playersOnSide } from '@/lib/members'

type Match = components['schemas']['Match']
type CricketDeliveryWicket = components['schemas']['CricketDeliveryWicket']

const DISMISSAL_KINDS: CricketDismissalKind[] = [
  'bowled',
  'caught',
  'leg_before_wicket',
  'run_out',
  'stumped',
  'hit_wicket',
]

/**
 * Records a wicket: dismissal kind, which batter is out (either end for a
 * run-out, otherwise always the striker), an optional fielder, and — since a
 * run-out can happen mid-run — how many runs had already been completed
 * before the dismissal. Bowler credit follows the backend's own rule
 * (`creditsBowler`), so it's implicit rather than asked.
 */
export function WicketDialog({
  open,
  match,
  battingSideId,
  strikerPlayerId,
  nonStrikerPlayerId,
  onOpenChange,
  onSubmit,
  submitting,
}: {
  open: boolean
  match: Match
  battingSideId: string
  strikerPlayerId: string
  nonStrikerPlayerId: string
  onOpenChange: (open: boolean) => void
  onSubmit: (wicket: CricketDeliveryWicket, runsBeforeDismissal: number) => void
  submitting: boolean
}) {
  const [kind, setKind] = useState<CricketDismissalKind>('bowled')
  const [dismissedPlayerId, setDismissedPlayerId] = useState(strikerPlayerId)
  const [fielderPlayerId, setFielderPlayerId] = useState<string | undefined>(undefined)
  const [runs, setRuns] = useState('0')

  useEffect(() => {
    if (!open) return
    setKind('bowled')
    setDismissedPlayerId(strikerPlayerId)
    setFielderPlayerId(undefined)
    setRuns('0')
  }, [open, strikerPlayerId])

  const battingSide = match.sides.find((s) => s.id === battingSideId)
  const fieldingRoster = match.sides
    .filter((s) => s.id !== battingSideId)
    .flatMap((s) => playersOnSide(match.players, s))
  const needsFielder = kind === 'caught' || kind === 'run_out' || kind === 'stumped'

  const submit = () => {
    onSubmit(
      {
        kind,
        dismissed_player_id: dismissedPlayerId,
        bowler_player_id: undefined,
        fielder_player_id: fielderPlayerId,
      },
      Number(runs) || 0,
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Wicket</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              How out
            </p>
            <div className="grid grid-cols-2 gap-2">
              {DISMISSAL_KINDS.map((k) => (
                <button
                  key={k}
                  type="button"
                  aria-pressed={kind === k}
                  onClick={() => setKind(k)}
                  className={cn(
                    'rounded-lg border p-2 text-sm font-medium transition-colors',
                    kind === k
                      ? 'border-primary bg-accent text-accent-foreground'
                      : 'text-muted-foreground hover:bg-muted',
                  )}
                >
                  {dismissalLabel(k)}
                </button>
              ))}
            </div>
          </div>

          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Batter out
            </p>
            {kind === 'run_out' ? (
              <PlayerPicker
                players={playersOnSide(match.players, battingSide!).filter((p) =>
                  [strikerPlayerId, nonStrikerPlayerId].includes(p.member.id),
                )}
                value={dismissedPlayerId}
                onChange={setDismissedPlayerId}
              />
            ) : (
              <p className="text-sm text-muted-foreground">
                Striker — the only batter who can be out this way.
              </p>
            )}
          </div>

          {needsFielder && (
            <div>
              <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                Fielder (optional)
              </p>
              <PlayerPicker
                players={fieldingRoster}
                value={fielderPlayerId}
                onChange={(id) => setFielderPlayerId(fielderPlayerId === id ? undefined : id)}
              />
            </div>
          )}

          {kind === 'run_out' && (
            <div className="flex items-center gap-2">
              <Label htmlFor="wicket-runs" className="shrink-0 text-xs text-muted-foreground">
                Runs completed first
              </Label>
              <Input
                id="wicket-runs"
                type="number"
                min={0}
                inputMode="numeric"
                className="h-8 w-20"
                value={runs}
                onChange={(e) => setRuns(e.target.value)}
              />
            </div>
          )}

          <p className="text-xs text-muted-foreground">
            {creditsBowler(kind)
              ? 'Credited to the bowler.'
              : "Not credited to the bowler's figures."}
          </p>

          <Button disabled={submitting} onClick={submit}>
            {submitting ? 'Saving…' : 'Record wicket'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
