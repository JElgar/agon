import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { RecordNetballEventDialog, type NetballEventKind } from '@/components/agon/live/RecordNetballEventDialog'
import { describeEvent, eventEmoji, goalEventsToViews, type NetballEventView } from '@/lib/netballScore'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type NetballGoalEvent = components['schemas']['NetballGoalEvent']
type NetballFoulEvent = components['schemas']['NetballFoulEvent']
type NetballLiveEvent = components['schemas']['NetballLiveEvent']

export interface NetballScoreFieldsProps {
  sideA: MatchSide
  sideB: MatchSide
  /** Roster to pick a scorer/fouling player from — real `match.players` when
   *  editing, or a synthesized roster (see `LogMatchPage`) when composing a
   *  match that doesn't exist yet. */
  players: MatchPlayer[]
  /** Seeds from an existing confirmed score, when editing. */
  initial?: Score
  onChange: (built: { score: Score; winnerSideId?: string } | null) => void
}

function sideLabel(side: MatchSide, fallback: string): string {
  return side.name?.trim() || fallback
}

function seedPoints(initial: Score | undefined, aId: string, bId: string): [string, string] {
  if (initial?.type === 'Netball') {
    return [String(initial.score[aId] ?? 0), String(initial.score[bId] ?? 0)]
  }
  return ['', '']
}

/**
 * Netball's result entry: a points pair by default, plus an optional
 * goal-by-goal detail section reusing `RecordNetballEventDialog` (the same
 * dialog event-by-event live scoring uses) to build up `goals`/`fouls`
 * locally instead of posting to `/live/events` — same pattern as
 * `FootballScoreFields`. Once at least one goal is added, the points become
 * derived from the goal tally (two-point-zone goals already counted double).
 * There's no manual-entry equivalent of quarter-only scoring's
 * `period_scores` breakdown here — a bare final tally covers the same
 * "just the numbers" case for a manually-logged result.
 */
export function NetballScoreFields({ sideA, sideB, players, initial, onChange }: NetballScoreFieldsProps) {
  const aId = sideA.id
  const bId = sideB.id
  const nameA = sideLabel(sideA, 'Side A')
  const nameB = sideLabel(sideB, 'Side B')

  const [points, setPoints] = useState<[string, string]>(() => seedPoints(initial, aId, bId))
  const [goals, setGoals] = useState<NetballGoalEvent[]>(() =>
    initial?.type === 'Netball' ? (initial.goals ?? []) : [],
  )
  const [fouls, setFouls] = useState<NetballFoulEvent[]>(() =>
    initial?.type === 'Netball' ? (initial.fouls ?? []) : [],
  )
  const [detailOpen, setDetailOpen] = useState(goals.length > 0)
  const [dialogKind, setDialogKind] = useState<NetballEventKind | null>(null)

  const match: Pick<Match, 'sides' | 'players'> = { sides: [sideA, sideB], players }

  const submitEvent = (event: NetballLiveEvent) => {
    if (event.kind === 'Goal') {
      setGoals((g) => [
        ...g,
        {
          side_id: event.side_id,
          scorer_player_id: event.scorer_player_id,
          scorer_position: event.scorer_position,
          two_points: event.two_points,
          minute: event.minute,
        },
      ])
    } else if (event.kind === 'Foul') {
      setFouls((f) => [
        ...f,
        {
          side_id: event.side_id,
          player_id: event.player_id,
          foul_kind: event.foul_kind,
          minute: event.minute,
        },
      ])
    }
    setDialogKind(null)
  }

  const [pointsA, pointsB] = points
  const tallyFor = (sideId: string) =>
    goals.filter((g) => g.side_id === sideId).reduce((sum, g) => sum + (g.two_points ? 2 : 1), 0)
  const goalsForA = tallyFor(aId)
  const goalsForB = tallyFor(bId)

  useEffect(() => {
    const hasDetail = goals.length > 0
    const a = hasDetail ? goalsForA : Number(pointsA)
    const b = hasDetail ? goalsForB : Number(pointsB)
    if (!hasDetail && (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))) {
      onChange(null)
      return
    }
    // `players` (the score's resolved-name map) is server-only — the backend
    // never persists it (see `NetballScore.players`'s doc comment) — so a
    // manually-built score has nothing to put here.
    const score: Score = {
      type: 'Netball',
      score: { [aId]: a, [bId]: b },
      players: {},
      ...(hasDetail ? { goals, fouls } : {}),
    }
    const winnerSideId = a === b ? undefined : a > b ? aId : bId
    onChange({ score, winnerSideId })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pointsA, pointsB, goals, fouls, aId, bId])

  const events: NetballEventView[] = [
    ...goalEventsToViews(goals),
    ...fouls.map((f): NetballEventView => ({ kind: 'foul', side_id: f.side_id, minute: f.minute, player_id: f.player_id })),
  ].sort((a, b) => (a.minute ?? Infinity) - (b.minute ?? Infinity))

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
        <div className="flex flex-col gap-1">
          <span className="truncate text-center text-xs text-muted-foreground">{nameA}</span>
          <Input
            type="number"
            min={0}
            inputMode="numeric"
            value={goals.length > 0 ? goalsForA : pointsA}
            disabled={goals.length > 0}
            onChange={(e) => setPoints([e.target.value, pointsB])}
            placeholder="0"
          />
        </div>
        <span className="pt-5 text-muted-foreground">–</span>
        <div className="flex flex-col gap-1">
          <span className="truncate text-center text-xs text-muted-foreground">{nameB}</span>
          <Input
            type="number"
            min={0}
            inputMode="numeric"
            value={goals.length > 0 ? goalsForB : pointsB}
            disabled={goals.length > 0}
            onChange={(e) => setPoints([pointsA, e.target.value])}
            placeholder="0"
          />
        </div>
      </div>

      {!detailOpen ? (
        <Button type="button" variant="ghost" size="sm" className="self-start" onClick={() => setDetailOpen(true)}>
          <Plus className="size-3.5" /> Add goal-by-goal detail
        </Button>
      ) : (
        <div className="flex flex-col gap-2 rounded-lg border bg-muted/30 p-3">
          {events.length > 0 && (
            <div className="flex flex-col gap-1">
              {events.map((event, i) => {
                const isSideB = event.side_id === bId
                return (
                  <p
                    key={i}
                    className={`flex items-baseline gap-1.5 text-xs ${isSideB ? 'flex-row-reverse text-right' : ''}`}
                  >
                    <span aria-hidden>{eventEmoji(event.kind)}</span>
                    {event.minute !== undefined && (
                      <span className="font-medium text-foreground">{event.minute}'</span>
                    )}
                    <span className="truncate">{describeEvent(event, match)}</span>
                  </p>
                )
              })}
            </div>
          )}
          <div className="flex gap-1.5">
            <Button type="button" variant="outline" size="sm" onClick={() => setDialogKind('goal')}>
              + Goal
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => setDialogKind('foul')}>
              + Foul
            </Button>
          </div>
        </div>
      )}

      <RecordNetballEventDialog
        open={dialogKind !== null}
        kind={dialogKind}
        match={match}
        initialMinute={0}
        onOpenChange={(open) => !open && setDialogKind(null)}
        onSubmit={submitEvent}
        submitting={false}
      />
    </div>
  )
}
