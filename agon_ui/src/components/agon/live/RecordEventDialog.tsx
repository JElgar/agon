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
type FootballLiveEvent = components['schemas']['FootballLiveEvent']

export type EventKind = 'goal' | 'card' | 'substitution'

function MinuteField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex items-center gap-2">
      <Label htmlFor="event-minute" className="shrink-0 text-xs text-muted-foreground">
        Minute
      </Label>
      <Input
        id="event-minute"
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

interface DraftBase {
  side_id?: string
  minute: string
}

interface GoalDraft extends DraftBase {
  scorer_player_id?: string
  assist_player_id?: string
  own_goal: boolean
  penalty: boolean
}

interface CardDraft extends DraftBase {
  player_id?: string
  color?: 'yellow' | 'red'
}

interface SubDraft extends DraftBase {
  player_out_id?: string
  player_in_id?: string
}

function toMinute(minute: string): number | undefined {
  const n = Number(minute)
  return minute !== '' && Number.isFinite(n) && n >= 0 ? n : undefined
}

/**
 * The event-entry dialog opened from a quick-action tap on the live scoring
 * screen. One component covers all three recordable football event kinds
 * (goal / card / substitution) since they share the same side→details shape;
 * `kind` picks which fields step 2 asks for.
 */
export function RecordEventDialog({
  open,
  kind,
  match,
  initialMinute,
  onOpenChange,
  onSubmit,
  submitting,
}: {
  open: boolean
  kind: EventKind | null
  /** Only `sides`/`players` are read — a manual-entry caller without a real
   *  match yet (see `FootballScoreFields`) can pass a synthetic object with
   *  just these two fields. */
  match: Pick<Match, 'sides' | 'players'>
  initialMinute: number
  onOpenChange: (open: boolean) => void
  onSubmit: (event: FootballLiveEvent) => void
  submitting: boolean
}) {
  const [goal, setGoal] = useState<GoalDraft>({ minute: '', own_goal: false, penalty: false })
  const [card, setCard] = useState<CardDraft>({ minute: '' })
  const [sub, setSub] = useState<SubDraft>({ minute: '' })

  // Reset to a fresh draft (minute prefilled from the live clock) every time
  // the dialog is opened for a new event.
  useEffect(() => {
    if (!open) return
    const minute = String(initialMinute)
    setGoal({ minute, own_goal: false, penalty: false })
    setCard({ minute })
    setSub({ minute })
  }, [open, initialMinute, kind])

  if (!kind) return null

  const side = match.sides.find(
    (s) => s.id === (kind === 'goal' ? goal.side_id : kind === 'card' ? card.side_id : sub.side_id),
  )
  const roster = side ? playersOnSide(match.players, side) : []

  const title = kind === 'goal' ? 'Goal' : kind === 'card' ? 'Card' : 'Substitution'

  const canSubmit =
    kind === 'goal'
      ? !!goal.side_id && (goal.own_goal || !!goal.scorer_player_id)
      : kind === 'card'
        ? !!card.side_id && !!card.player_id && !!card.color
        : !!sub.side_id && !!sub.player_out_id && !!sub.player_in_id

  const submit = () => {
    if (kind === 'goal' && goal.side_id) {
      onSubmit({
        kind: 'Goal',
        side_id: goal.side_id,
        scorer_player_id: goal.own_goal ? undefined : goal.scorer_player_id,
        assist_player_id: goal.own_goal ? undefined : goal.assist_player_id,
        own_goal: goal.own_goal,
        penalty: goal.penalty,
        minute: toMinute(goal.minute),
      } as FootballLiveEvent)
    } else if (kind === 'card' && card.side_id && card.player_id && card.color) {
      onSubmit({
        kind: 'Card',
        side_id: card.side_id,
        player_id: card.player_id,
        color: card.color,
        minute: toMinute(card.minute),
      } as FootballLiveEvent)
    } else if (kind === 'substitution' && sub.side_id && sub.player_out_id && sub.player_in_id) {
      onSubmit({
        kind: 'Substitution',
        side_id: sub.side_id,
        player_in_id: sub.player_in_id,
        player_out_id: sub.player_out_id,
        minute: toMinute(sub.minute),
      } as FootballLiveEvent)
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
              value={kind === 'goal' ? goal.side_id : kind === 'card' ? card.side_id : sub.side_id}
              onChange={(sideId) => {
                if (kind === 'goal') setGoal((d) => ({ ...d, side_id: sideId, scorer_player_id: undefined, assist_player_id: undefined }))
                else if (kind === 'card') setCard((d) => ({ ...d, side_id: sideId, player_id: undefined }))
                else setSub((d) => ({ ...d, side_id: sideId, player_out_id: undefined, player_in_id: undefined }))
              }}
            />
          </div>

          {side && kind === 'goal' && (
            <>
              <div className="flex items-center gap-4">
                <label className="flex items-center gap-1.5 text-xs">
                  <input
                    type="checkbox"
                    checked={goal.own_goal}
                    onChange={(e) => setGoal((d) => ({ ...d, own_goal: e.target.checked }))}
                  />
                  Own goal
                </label>
                <label className="flex items-center gap-1.5 text-xs">
                  <input
                    type="checkbox"
                    checked={goal.penalty}
                    disabled={goal.own_goal}
                    onChange={(e) => setGoal((d) => ({ ...d, penalty: e.target.checked }))}
                  />
                  Penalty
                </label>
              </div>
              {!goal.own_goal && (
                <div>
                  <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                    Scorer
                  </p>
                  <PlayerPicker
                    players={roster}
                    value={goal.scorer_player_id}
                    onChange={(id) => setGoal((d) => ({ ...d, scorer_player_id: id }))}
                  />
                </div>
              )}
              {!goal.own_goal && goal.scorer_player_id && roster.length > 1 && (
                <div>
                  <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                    Assist (optional)
                  </p>
                  <PlayerPicker
                    players={roster}
                    value={goal.assist_player_id}
                    exclude={goal.scorer_player_id}
                    onChange={(id) =>
                      setGoal((d) => ({
                        ...d,
                        assist_player_id: d.assist_player_id === id ? undefined : id,
                      }))
                    }
                  />
                </div>
              )}
            </>
          )}

          {side && kind === 'card' && (
            <>
              <div>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Player
                </p>
                <PlayerPicker
                  players={roster}
                  value={card.player_id}
                  onChange={(id) => setCard((d) => ({ ...d, player_id: id }))}
                />
              </div>
              <div className="grid grid-cols-2 gap-2">
                {(['yellow', 'red'] as const).map((color) => (
                  <button
                    key={color}
                    type="button"
                    aria-pressed={card.color === color}
                    onClick={() => setCard((d) => ({ ...d, color }))}
                    className={cn(
                      'flex items-center justify-center gap-2 rounded-lg border p-2.5 text-sm font-medium capitalize transition-colors',
                      card.color === color
                        ? 'border-primary bg-accent text-accent-foreground'
                        : 'text-muted-foreground hover:bg-muted',
                    )}
                  >
                    <span
                      className={cn(
                        'size-3 rounded-sm',
                        color === 'yellow' ? 'bg-yellow-400' : 'bg-red-600',
                      )}
                    />
                    {color}
                  </button>
                ))}
              </div>
            </>
          )}

          {side && kind === 'substitution' && (
            <>
              <div>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Coming off
                </p>
                <PlayerPicker
                  players={roster}
                  value={sub.player_out_id}
                  exclude={sub.player_in_id}
                  onChange={(id) => setSub((d) => ({ ...d, player_out_id: id }))}
                />
              </div>
              <div>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  Going on
                </p>
                <PlayerPicker
                  players={roster}
                  value={sub.player_in_id}
                  exclude={sub.player_out_id}
                  onChange={(id) => setSub((d) => ({ ...d, player_in_id: id }))}
                />
              </div>
            </>
          )}

          <MinuteField
            value={kind === 'goal' ? goal.minute : kind === 'card' ? card.minute : sub.minute}
            onChange={(v) => {
              if (kind === 'goal') setGoal((d) => ({ ...d, minute: v }))
              else if (kind === 'card') setCard((d) => ({ ...d, minute: v }))
              else setSub((d) => ({ ...d, minute: v }))
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
