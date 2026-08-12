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
import { playersOnSide } from '@/lib/members'
import { PlayerPicker, SidePicker } from './Pickers'

type Match = components['schemas']['Match']
type NetballLiveEvent = components['schemas']['NetballLiveEvent']
type NetballFoulKind = components['schemas']['NetballFoulKind']

export type NetballEventKind = 'goal' | 'foul'

const FOUL_KINDS: NetballFoulKind[] = ['contact', 'obstruction', 'footwork', 'offside', 'held_ball', 'other']
const FOUL_KIND_LABEL: Record<NetballFoulKind, string> = {
  contact: 'Contact',
  obstruction: 'Obstruction',
  footwork: 'Footwork',
  offside: 'Offside',
  held_ball: 'Held ball',
  other: 'Other',
}

function MinuteField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex items-center gap-2">
      <Label htmlFor="netball-event-minute" className="shrink-0 text-xs text-muted-foreground">
        Minute (this quarter)
      </Label>
      <Input
        id="netball-event-minute"
        type="number"
        min={0}
        inputMode="numeric"
        className="h-8 w-20"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  )
}

interface GoalDraft {
  side_id?: string
  scorer_player_id?: string
  two_points: boolean
  minute: string
}

interface FoulDraft {
  side_id?: string
  player_id?: string
  foul_kind?: NetballFoulKind
  minute: string
}

function toMinute(minute: string): number | undefined {
  const n = Number(minute)
  return minute !== '' && Number.isFinite(n) && n >= 0 ? n : undefined
}

/**
 * The event-entry dialog for netball's event-by-event live scoring — one
 * component covers both recordable kinds (goal / foul), same pattern as
 * football's `RecordEventDialog`. Quarter-only scoring never opens this;
 * it records period-end scores directly (see `LiveScoringPage`'s netball
 * flow).
 */
export function RecordNetballEventDialog({
  open,
  kind,
  match,
  initialMinute,
  onOpenChange,
  onSubmit,
  submitting,
}: {
  open: boolean
  kind: NetballEventKind | null
  /** Only `sides`/`players` are read — a manual-entry caller without a real
   *  match yet (see `NetballScoreFields`) can pass a synthetic object with
   *  just these two fields. */
  match: Pick<Match, 'sides' | 'players'>
  initialMinute: number
  onOpenChange: (open: boolean) => void
  onSubmit: (event: NetballLiveEvent) => void
  submitting: boolean
}) {
  const [goal, setGoal] = useState<GoalDraft>({ two_points: false, minute: '' })
  const [foul, setFoul] = useState<FoulDraft>({ minute: '' })

  useEffect(() => {
    if (!open) return
    const minute = String(initialMinute)
    setGoal({ two_points: false, minute })
    setFoul({ minute })
  }, [open, initialMinute, kind])

  if (!kind) return null

  const side = match.sides.find((s) => s.id === (kind === 'goal' ? goal.side_id : foul.side_id))
  const roster = side ? playersOnSide(match.players, side) : []

  const title = kind === 'goal' ? 'Goal' : 'Foul'

  const canSubmit = kind === 'goal' ? !!goal.side_id : !!foul.side_id && !!foul.foul_kind

  const submit = () => {
    if (kind === 'goal' && goal.side_id) {
      onSubmit({
        kind: 'Goal',
        side_id: goal.side_id,
        scorer_player_id: goal.scorer_player_id,
        two_points: goal.two_points,
        minute: toMinute(goal.minute),
      } as NetballLiveEvent)
    } else if (kind === 'foul' && foul.side_id && foul.foul_kind) {
      onSubmit({
        kind: 'Foul',
        side_id: foul.side_id,
        player_id: foul.player_id,
        foul_kind: foul.foul_kind,
        minute: toMinute(foul.minute),
      } as NetballLiveEvent)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Side
            </p>
            <SidePicker
              sides={match.sides}
              value={kind === 'goal' ? goal.side_id : foul.side_id}
              onChange={(sideId) => {
                if (kind === 'goal') setGoal((d) => ({ ...d, side_id: sideId, scorer_player_id: undefined }))
                else setFoul((d) => ({ ...d, side_id: sideId, player_id: undefined }))
              }}
            />
          </div>

          {side && kind === 'goal' && (
            <>
              <label className="flex items-center gap-1.5 text-xs">
                <input
                  type="checkbox"
                  checked={goal.two_points}
                  onChange={(e) => setGoal((d) => ({ ...d, two_points: e.target.checked }))}
                />
                Two-point zone
              </label>
              <div>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Scorer (optional)
                </p>
                <PlayerPicker
                  players={roster}
                  value={goal.scorer_player_id}
                  onChange={(id) =>
                    setGoal((d) => ({
                      ...d,
                      scorer_player_id: d.scorer_player_id === id ? undefined : id,
                    }))
                  }
                />
              </div>
            </>
          )}

          {side && kind === 'foul' && (
            <>
              <div>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Player (optional)
                </p>
                <PlayerPicker
                  players={roster}
                  value={foul.player_id}
                  onChange={(id) =>
                    setFoul((d) => ({ ...d, player_id: d.player_id === id ? undefined : id }))
                  }
                />
              </div>
              <div className="grid grid-cols-3 gap-2">
                {FOUL_KINDS.map((fk) => (
                  <button
                    key={fk}
                    type="button"
                    aria-pressed={foul.foul_kind === fk}
                    onClick={() => setFoul((d) => ({ ...d, foul_kind: fk }))}
                    className={cn(
                      'rounded-lg border p-2 text-xs font-medium transition-colors',
                      foul.foul_kind === fk
                        ? 'border-primary bg-accent text-accent-foreground'
                        : 'text-muted-foreground hover:bg-muted',
                    )}
                  >
                    {FOUL_KIND_LABEL[fk]}
                  </button>
                ))}
              </div>
            </>
          )}

          <MinuteField
            value={kind === 'goal' ? goal.minute : foul.minute}
            onChange={(v) => {
              if (kind === 'goal') setGoal((d) => ({ ...d, minute: v }))
              else setFoul((d) => ({ ...d, minute: v }))
            }}
          />

          <Button disabled={!canSubmit || submitting} onClick={submit}>
            {submitting ? 'Saving…' : `Add ${title.toLowerCase()}`}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
