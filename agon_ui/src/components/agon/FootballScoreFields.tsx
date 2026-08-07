import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { RecordEventDialog, type EventKind } from '@/components/agon/live/RecordEventDialog'
import { describeEvent, eventEmoji, goalEventsToViews, type FootballEventView } from '@/lib/liveScore'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type FootballCardEvent = components['schemas']['FootballCardEvent']
type FootballSubstitutionEvent = components['schemas']['FootballSubstitutionEvent']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']

export interface FootballScoreFieldsProps {
  sideA: MatchSide
  sideB: MatchSide
  /** Roster to pick a scorer/assist/card/sub player from — real `match.players`
   *  when editing, or a synthesized roster (see `LogMatchPage`) when composing
   *  a match that doesn't exist yet. */
  players: MatchPlayer[]
  /** Seeds from an existing confirmed score, when editing. */
  initial?: Score
  onChange: (built: { score: Score; winnerSideId?: string } | null) => void
}

function sideLabel(side: MatchSide, fallback: string): string {
  return side.name?.trim() || fallback
}

function seedPoints(initial: Score | undefined, aId: string, bId: string): [string, string] {
  if (initial?.type === 'Football') {
    return [String(initial.score[aId] ?? 0), String(initial.score[bId] ?? 0)]
  }
  return ['', '']
}

/**
 * Football's result entry: a points pair by default (unchanged from before),
 * plus an optional goal-by-goal detail section reusing `RecordEventDialog`
 * (the same dialog live scoring uses) to build up `goals`/`cards`/
 * `substitutions` locally instead of posting to `/live/events`. Once at
 * least one goal is added, the points become derived from the goal tally
 * (each goal's `side_id` already carries who it counts for, including own
 * goals) rather than left free to disagree with the detail.
 */
export function FootballScoreFields({ sideA, sideB, players, initial, onChange }: FootballScoreFieldsProps) {
  const aId = sideA.id
  const bId = sideB.id
  const nameA = sideLabel(sideA, 'Side A')
  const nameB = sideLabel(sideB, 'Side B')

  const [points, setPoints] = useState<[string, string]>(() => seedPoints(initial, aId, bId))
  const [goals, setGoals] = useState<FootballGoalEvent[]>(() =>
    initial?.type === 'Football' ? (initial.goals ?? []) : [],
  )
  const [cards, setCards] = useState<FootballCardEvent[]>(() =>
    initial?.type === 'Football' ? (initial.cards ?? []) : [],
  )
  const [substitutions, setSubstitutions] = useState<FootballSubstitutionEvent[]>(() =>
    initial?.type === 'Football' ? (initial.substitutions ?? []) : [],
  )
  const [detailOpen, setDetailOpen] = useState(goals.length > 0)
  const [dialogKind, setDialogKind] = useState<EventKind | null>(null)

  const match: Pick<Match, 'sides' | 'players'> = { sides: [sideA, sideB], players }

  const submitEvent = (event: FootballLiveEvent) => {
    if (event.kind === 'Goal') {
      setGoals((g) => [
        ...g,
        {
          side_id: event.side_id,
          scorer_player_id: event.scorer_player_id,
          assist_player_id: event.assist_player_id,
          own_goal: event.own_goal,
          penalty: event.penalty,
          minute: event.minute,
        },
      ])
    } else if (event.kind === 'Card') {
      setCards((c) => [
        ...c,
        {
          side_id: event.side_id,
          player_id: event.player_id,
          color: event.color,
          minute: event.minute,
        },
      ])
    } else if (event.kind === 'Substitution') {
      setSubstitutions((s) => [
        ...s,
        {
          side_id: event.side_id,
          player_in_id: event.player_in_id,
          player_out_id: event.player_out_id,
          minute: event.minute,
        },
      ])
    }
    setDialogKind(null)
  }

  const [pointsA, pointsB] = points
  const goalsForA = goals.filter((g) => g.side_id === aId).length
  const goalsForB = goals.filter((g) => g.side_id === bId).length

  useEffect(() => {
    const hasDetail = goals.length > 0
    const a = hasDetail ? goalsForA : Number(pointsA)
    const b = hasDetail ? goalsForB : Number(pointsB)
    if (!hasDetail && (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))) {
      onChange(null)
      return
    }
    // `players` (the score's resolved-name map) is server-only — the backend
    // never persists it (see `FootballScore.players`'s doc comment) — so a
    // manually-built score has nothing to put here.
    const score: Score = {
      type: 'Football',
      score: { [aId]: a, [bId]: b },
      players: {},
      ...(hasDetail ? { goals, cards, substitutions } : {}),
    }
    const winnerSideId = a === b ? undefined : a > b ? aId : bId
    onChange({ score, winnerSideId })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pointsA, pointsB, goals, cards, substitutions, aId, bId])

  const events: FootballEventView[] = [
    ...goalEventsToViews(goals),
    ...cards.map((c): FootballEventView => ({ kind: c.color === 'yellow' ? 'yellow_card' : 'red_card', side_id: c.side_id, minute: c.minute, player_id: c.player_id })),
    ...substitutions.map((s): FootballEventView => ({ kind: 'substitution', side_id: s.side_id, minute: s.minute, player_id: s.player_in_id, substituted_player_id: s.player_out_id })),
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
            <Button type="button" variant="outline" size="sm" onClick={() => setDialogKind('card')}>
              + Card
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => setDialogKind('substitution')}>
              + Sub
            </Button>
          </div>
        </div>
      )}

      <RecordEventDialog
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
