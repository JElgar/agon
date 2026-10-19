import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { RecordNetballEventDialog, type NetballEventKind } from '@/components/agon/live/RecordNetballEventDialog'
import { describeEvent, eventClockLabel, eventEmoji, goalEventsToViews, type NetballEventView } from '@/lib/netballScore'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type NetballGoalEvent = components['schemas']['NetballGoalEvent']
type NetballFoulEvent = components['schemas']['NetballFoulEvent']
type NetballLiveEvent = components['schemas']['NetballLiveEvent']
type NetballPeriod = components['schemas']['NetballPeriod']

/** The quarter-end markers a manually entered quarter breakdown fills in, in
 *  play order — the same cumulative checkpoints `NetballQuarterBreakdown`
 *  reads back (see `NetballScore.period_scores`'s backend doc comment). No
 *  extra-time row here — a manually logged quarter breakdown only covers the
 *  four regulation quarters. */
const QUARTER_END_MARKERS: NetballPeriod[] = [
  'quarter_one_end',
  'quarter_two_end',
  'quarter_three_end',
  'full_time',
]

/** One quarter's own goals for each side (not cumulative) — what the user
 *  actually types in. */
interface QuarterRow {
  a: string
  b: string
}

function emptyQuarters(): QuarterRow[] {
  return QUARTER_END_MARKERS.map(() => ({ a: '', b: '' }))
}

/** Seeds the quarter rows from an existing quarter-scored result, converting
 *  `period_scores`' cumulative checkpoints back into each quarter's own
 *  tally. Falls back to blank rows unless every quarter marker is present —
 *  a partial breakdown can't be reconstructed unambiguously. */
function seedQuarters(initial: Score | undefined, aId: string, bId: string): QuarterRow[] {
  if (initial?.type !== 'Netball' || !initial.period_scores) return emptyQuarters()
  const cumulative = QUARTER_END_MARKERS.map((period) => initial.period_scores?.[period])
  if (cumulative.some((entry) => !entry)) return emptyQuarters()
  let prevA = 0
  let prevB = 0
  return cumulative.map((entry) => {
    const a = entry?.[aId] ?? 0
    const b = entry?.[bId] ?? 0
    const row = { a: String(a - prevA), b: String(b - prevB) }
    prevA = a
    prevB = b
    return row
  })
}

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
  return sideDisplayName(side) ?? fallback
}

function seedPoints(initial: Score | undefined, aId: string, bId: string): [string, string] {
  if (initial?.type === 'Netball') {
    return [String(initial.score[aId] ?? 0), String(initial.score[bId] ?? 0)]
  }
  return ['', '']
}

/** Which optional detail section is open, if any — mutually exclusive, same
 *  as the two live-scoring methods (`quarter_only`/`event_by_event`) they
 *  mirror. `'none'` is a bare final tally. */
type DetailMode = 'none' | 'quarters' | 'events'

function initialDetailMode(initial: Score | undefined): DetailMode {
  if (initial?.type !== 'Netball') return 'none'
  if ((initial.goals?.length ?? 0) > 0) return 'events'
  if (QUARTER_END_MARKERS.every((period) => initial.period_scores?.[period])) return 'quarters'
  return 'none'
}

/**
 * Netball's result entry: a points pair by default, plus one optional detail
 * section — a quarter-by-quarter breakdown, or goal-by-goal detail reusing
 * `RecordNetballEventDialog` (the same dialog event-by-event live scoring
 * uses) to build up `goals`/`fouls` locally instead of posting to
 * `/live/events` (same pattern as `FootballScoreFields`). Once either is in
 * use, the final points become derived from it (summed quarters, or the goal
 * tally with two-point-zone goals already counted double) — same relationship
 * as `period_scores`/`score` for a live-scored match.
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
  const [quarters, setQuarters] = useState<QuarterRow[]>(() => seedQuarters(initial, aId, bId))
  const [detailMode, setDetailMode] = useState<DetailMode>(() => initialDetailMode(initial))
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

  const hasEvents = detailMode === 'events' && goals.length > 0
  const quarterRowFilled = (row: QuarterRow) => row.a !== '' && row.b !== '' && Number.isFinite(Number(row.a)) && Number.isFinite(Number(row.b))
  const quartersFilled = detailMode === 'quarters' && quarters.every(quarterRowFilled)
  const quarterTotalFor = (side: 'a' | 'b') => quarters.reduce((sum, row) => sum + (Number(row[side]) || 0), 0)
  const quarterTotalA = quarterTotalFor('a')
  const quarterTotalB = quarterTotalFor('b')

  useEffect(() => {
    if (detailMode === 'quarters' && !quartersFilled) {
      onChange(null)
      return
    }
    const a = hasEvents ? goalsForA : detailMode === 'quarters' ? quarterTotalA : Number(pointsA)
    const b = hasEvents ? goalsForB : detailMode === 'quarters' ? quarterTotalB : Number(pointsB)
    if (
      detailMode === 'none' &&
      (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))
    ) {
      onChange(null)
      return
    }
    let periodScores: Partial<Record<NetballPeriod, Record<string, number>>> | undefined
    if (detailMode === 'quarters') {
      periodScores = {}
      let cumA = 0
      let cumB = 0
      QUARTER_END_MARKERS.forEach((period, i) => {
        cumA += Number(quarters[i].a) || 0
        cumB += Number(quarters[i].b) || 0
        periodScores![period] = { [aId]: cumA, [bId]: cumB }
      })
    }
    // `players` (the score's resolved-name map) is server-only — the backend
    // never persists it (see `NetballScore.players`'s doc comment) — so a
    // manually-built score has nothing to put here.
    const score: Score = {
      type: 'Netball',
      score: { [aId]: a, [bId]: b },
      players: {},
      ...(hasEvents ? { goals, fouls } : {}),
      ...(periodScores ? { period_scores: periodScores } : {}),
    }
    const winnerSideId = a === b ? undefined : a > b ? aId : bId
    onChange({ score, winnerSideId })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pointsA, pointsB, goals, fouls, quarters, detailMode, aId, bId])

  const events: NetballEventView[] = [
    ...goalEventsToViews(goals),
    ...fouls.map((f): NetballEventView => ({
      kind: 'foul',
      side_id: f.side_id,
      minute: f.minute,
      player_id: f.player_id,
      foul_kind: f.foul_kind,
    })),
    // No live clock here (see this component's doc comment) — `minute` is
    // the only time signal a manually entered event carries, so it's what
    // interleaves goals and fouls into one list.
  ].sort((a, b) => (a.minute ?? Infinity) - (b.minute ?? Infinity))

  const pointsDisabled = detailMode !== 'none'
  const [displayA, displayB] =
    detailMode === 'events' ? [goalsForA, goalsForB] : detailMode === 'quarters' ? [quarterTotalA, quarterTotalB] : [pointsA, pointsB]

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
        <div className="flex flex-col gap-1">
          <span className="truncate text-center text-xs text-muted-foreground">{nameA}</span>
          <Input
            type="number"
            min={0}
            inputMode="numeric"
            value={displayA}
            disabled={pointsDisabled}
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
            value={displayB}
            disabled={pointsDisabled}
            onChange={(e) => setPoints([pointsA, e.target.value])}
            placeholder="0"
          />
        </div>
      </div>

      {detailMode === 'none' ? (
        <div className="flex flex-wrap gap-1">
          <Button type="button" variant="ghost" size="sm" onClick={() => setDetailMode('quarters')}>
            <Plus className="size-3.5" /> Add quarter-by-quarter score
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={() => setDetailMode('events')}>
            <Plus className="size-3.5" /> Add goal-by-goal detail
          </Button>
        </div>
      ) : detailMode === 'quarters' ? (
        <div className="flex flex-col gap-2 rounded-lg border bg-muted/30 p-3">
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2 text-center text-[11px] uppercase tracking-wider text-muted-foreground">
            <span className="truncate text-left">{nameA}</span>
            <span>Quarter</span>
            <span className="truncate text-right">{nameB}</span>
          </div>
          {quarters.map((row, i) => (
            <div key={i} className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={row.a}
                onChange={(e) => setQuarters((qs) => qs.map((r, j) => (j === i ? { ...r, a: e.target.value } : r)))}
                placeholder="0"
              />
              <span className="text-xs text-muted-foreground">Q{i + 1}</span>
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={row.b}
                onChange={(e) => setQuarters((qs) => qs.map((r, j) => (j === i ? { ...r, b: e.target.value } : r)))}
                placeholder="0"
              />
            </div>
          ))}
        </div>
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
                    {eventClockLabel(event) && (
                      <span className="font-medium text-foreground">{eventClockLabel(event)}</span>
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
