import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate, Link } from 'react-router-dom'
import { CircleDot, OctagonAlert, TimerReset } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useAppendNetballEvent, useLiveSeq } from '@/hooks/useLiveScore'
import { matchScoreQueryKey, useMatchScore } from '@/hooks/useMatchScore'
import {
  RecordNetballEventDialog,
  type NetballEventKind,
} from '@/components/agon/live/RecordNetballEventDialog'
import { NetballQuarterBreakdown } from '@/components/agon/NetballQuarterBreakdown'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { UndoLastEventButton } from '@/components/agon/live/UndoLastEventButton'
import { netballFormat } from '@/lib/matchFormat'
import {
  currentQuarterElapsedSeconds,
  describeEvent,
  eventClockLabel,
  eventEmoji,
  eventsFromDetail,
  formatClock,
  isLivePlayPhase,
  isQuarterOvertime,
  netballScoreFrom,
  nextPeriodForPhase,
  nextPhaseActionLabel,
  phaseFromState,
  phaseLabel,
  type ClockPhase,
  type NetballPeriod,
  type NetballScore,
  type NetballScoringMethod,
} from '@/lib/netballScore'

type Match = components['schemas']['Match']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

function sideName(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/** The quarter-end markers in play order, extra time last — what
 *  quarter-only mode walks through one at a time. */
const QUARTER_SEQUENCE: NetballPeriod[] = [
  'quarter_one_end',
  'quarter_two_end',
  'quarter_three_end',
  'full_time',
]

/**
 * Route entry for a netball match at `/matches/:matchId/live` (dispatched
 * from `LiveScoringPage`). Netball has two live-scoring methods sharing one
 * event vocabulary (see `live_score::netball`'s backend doc comment) — this
 * asks once, before the first event, which one this scorer wants, then
 * hands off to the matching screen.
 *
 * The recorded log is the source of truth once it says anything unambiguous:
 * a `Start` marker or any goal means event-by-event (quarter-only never
 * records `Start` — its first event is always a quarter-end marker, see
 * `NetballQuarterOnlyScoringPage`); a quarter-end marker with neither means
 * quarter-only. Crucially, a `Start` marker with zero goals *yet* (right
 * after tapping "Start match", before the first goal) must still read as
 * event-by-event, not fall back to "no goals ⇒ quarter-only" — that
 * inversion was a real bug here: it flipped the screen to the quarter-only
 * score-entry form the instant the match started, before anyone could log a
 * goal or foul.
 *
 * Before anything's recorded there's no log to read, so the picked method
 * lives only in this component's own state — not persisted anywhere. Nothing
 * has been written to the server yet at that point, so there's nothing to
 * lose by picking again: leaving the page and coming back (or tapping
 * "Change scoring method" on either screen, offered only while the log is
 * still empty) just re-shows the picker. Once the log says something,
 * changing your mind means undoing that event instead (see
 * `UndoLastEventButton`, present on both screens) — the log is the only
 * source of truth then, not a component-local guess.
 */
export function NetballLiveScoringPage({ match }: { match: Match }) {
  const scoreQuery = useMatchScore(match.id, { refetchInterval: 8000 })
  const state = netballScoreFrom(scoreQuery.data)
  const [pickedMethod, setPickedMethod] = useState<NetballScoringMethod | null>(null)

  if (scoreQuery.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  const inferredMethod: NetballScoringMethod | null = state
    ? (state.goals?.length ?? 0) > 0 || !!state.period_times?.start
      ? 'event_by_event'
      : Object.keys(state.period_scores ?? {}).length > 0
        ? 'quarter_only'
        : null
    : null
  const method = inferredMethod ?? pickedMethod

  if (!method) {
    return <NetballScoringMethodPicker match={match} onChoose={setPickedMethod} />
  }

  // Only offered while nothing's recorded yet — once the log has anything,
  // `inferredMethod` (not this local pick) is what decides the screen, so
  // "changing your mind" at that point means undoing the event instead.
  const onBackToPicker = inferredMethod ? undefined : () => setPickedMethod(null)

  return method === 'event_by_event' ? (
    <NetballEventByEventScoringPage match={match} state={state} onBackToPicker={onBackToPicker} />
  ) : (
    <NetballQuarterOnlyScoringPage match={match} state={state} onBackToPicker={onBackToPicker} />
  )
}

function NetballScoringMethodPicker({
  match,
  onChoose,
}: {
  match: Match
  onChoose: (method: NetballScoringMethod) => void
}) {
  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div>
        <h1 className="text-lg font-semibold">How do you want to score this?</h1>
        <p className="text-sm text-muted-foreground">
          {nameA} vs {nameB} · Netball
        </p>
      </div>

      <button
        type="button"
        onClick={() => onChoose('event_by_event')}
        className="rounded-xl border bg-card p-4 text-left transition-colors hover:bg-muted"
      >
        <p className="font-medium">Goal by goal</p>
        <p className="mt-1 text-sm text-muted-foreground">
          Log every goal and foul as it happens, with a running quarter clock.
        </p>
      </button>

      <button
        type="button"
        onClick={() => onChoose('quarter_only')}
        className="rounded-xl border bg-card p-4 text-left transition-colors hover:bg-muted"
      >
        <p className="font-medium">Score at the end of each quarter</p>
        <p className="mt-1 text-sm text-muted-foreground">
          Just enter the running score after each quarter — no goal-by-goal detail.
        </p>
      </button>
    </div>
  )
}

/** Shared "finish the match" mutation — PATCHes `status: completed` with
 *  this device's own view of the live-derived score, same pattern (and same
 *  409-on-disagreement handling) as football's `finishMatch`. */
function useFinishNetballMatch(match: Match) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      const state = netballScoreFrom(
        queryClient.getQueryData<Score | null>(matchScoreQueryKey(match.id)),
      )
      const body: UpdateMatchInput = { status: 'completed', score: state ?? undefined }
      const { error, response } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (response.status === 409) {
        await queryClient.refetchQueries({ queryKey: matchScoreQueryKey(match.id) })
        throw new Error('The live score just changed — check the updated score above, then finish again')
      }
      if (error) throw new Error('Failed to finish the match')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      navigate(`/matches/${match.id}`)
    },
  })
}

/** How often the on-screen clock re-renders while a quarter is running —
 *  every second, so the scorer sees it actually ticking (not just jumping a
 *  minute at a time) and notices promptly once it goes red (see
 *  `isQuarterOvertime`). */
const CLOCK_TICK_MS = 1_000

/**
 * Event-by-event netball scoring: quick actions to log goals/fouls as they
 * happen, a running per-quarter clock, and the event log so far — the
 * netball equivalent of football's `LiveScoringPage`. Advancing the clock
 * (end of a quarter) auto-fills the `Period` marker's `score` from the
 * running goal tally the client already has — the user never types a number
 * here; that's quarter-only mode's job (`NetballQuarterOnlyScoringPage`).
 */
function NetballEventByEventScoringPage({
  match,
  state,
  onBackToPicker,
}: {
  match: Match
  state: NetballScore | null
  /** Present only while nothing's been recorded yet — see
   *  `NetballLiveScoringPage`'s doc comment. */
  onBackToPicker?: () => void
}) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const seq = useLiveSeq(match.id)
  const append = useAppendNetballEvent(match.id)
  const finishMatch = useFinishNetballMatch(match)

  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), CLOCK_TICK_MS)
    return () => clearInterval(id)
  }, [])

  const [dialogKind, setDialogKind] = useState<NetballEventKind | null>(null)

  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')
  const aId = match.sides[0]?.id
  const bId = match.sides[1]?.id
  const goalsFor = (sideId: string | undefined) => (sideId ? state?.score[sideId] : undefined) ?? 0

  const format = netballFormat(match.format)
  const phase: ClockPhase = state ? phaseFromState(state) : 'not_started'
  const elapsedSeconds = state ? currentQuarterElapsedSeconds(state, now) : null
  const overtime = !!state && isQuarterOvertime(state, format, now)
  const isDraw = !!state && goalsFor(aId) === goalsFor(bId)
  const progressionCtx = { isDraw, extraTime: format.extra_time }

  const handleAdvancePhase = () => {
    const period = nextPeriodForPhase(phase, progressionCtx)
    if (!period) return
    append.mutate({ kind: 'Period', period, score: state?.score ?? {} })
  }

  // Every phase with an advance action: ending the current quarter, or
  // (from a `break_*` phase) starting the next one once the scorer taps
  // "start" — see `phaseFromState`'s doc comment for why a break doesn't
  // advance on its own.
  const advanceClockPhases: ClockPhase[] = [
    'not_started',
    'quarter_1',
    'break_1',
    'quarter_2',
    'break_2',
    'quarter_3',
    'break_3',
    'quarter_4',
    'extra_time',
  ]
  const showAdvanceClockTile = advanceClockPhases.includes(phase)

  const actions: { key: string; label: string; icon: React.ReactNode; onClick: () => void; disabled?: boolean }[] = [
    ...(isLivePlayPhase(phase)
      ? [
          { key: 'goal', label: 'Goal', icon: <CircleDot className="size-5" />, onClick: () => setDialogKind('goal') },
          { key: 'foul', label: 'Foul', icon: <OctagonAlert className="size-5" />, onClick: () => setDialogKind('foul') },
        ]
      : []),
    ...(showAdvanceClockTile
      ? [
          {
            key: 'advance_phase',
            label: nextPhaseActionLabel(phase, progressionCtx),
            icon: <TimerReset className="size-5" />,
            onClick: handleAdvancePhase,
            disabled: nextPeriodForPhase(phase, progressionCtx) === null,
          },
        ]
      : []),
  ]

  const decidingPhase = phase === 'full_time'
  const continuationAvailable = decidingPhase && isDraw && format.extra_time
  const readyToFinish = (decidingPhase && !continuationAvailable) || phase === 'finished'

  const events = state
    ? eventsFromDetail({
        goals: state.goals ?? [],
        fouls: state.fouls ?? [],
        players: state.players,
        period_times: state.period_times,
      }).reverse()
    : []

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
          Back
        </Button>
        <div className="flex items-center gap-1">
          <UndoLastEventButton matchId={match.id} seq={seq.data} />
          <LiveIndicator />
        </div>
      </div>

      <div>
        <h1 className="text-lg font-semibold">
          {nameA} vs {nameB}
        </h1>
        <p className="text-sm text-muted-foreground">
          You're scoring this match — goal by goal
          {onBackToPicker && (
            <>
              {' · '}
              <button type="button" onClick={onBackToPicker} className="text-primary hover:underline">
                Change scoring method
              </button>
            </>
          )}
        </p>
      </div>

      <div className="rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between">
          <p className="flex-1 truncate text-sm font-medium">{nameA}</p>
          <div className="px-3 text-center">
            <div className="text-3xl font-medium tracking-tight">
              {goalsFor(aId)}
              <span className="text-muted-foreground">–</span>
              {goalsFor(bId)}
            </div>
            <div className={cn('mt-0.5 text-xs', overtime ? 'font-semibold text-destructive' : 'text-primary')}>
              {elapsedSeconds !== null && `${formatClock(elapsedSeconds)} · `}
              {phaseLabel(phase)}
            </div>
          </div>
          <p className="flex-1 truncate text-right text-sm font-medium">{nameB}</p>
        </div>
      </div>

      {actions.length > 0 && (
        <div className="grid grid-cols-2 gap-3">
          {actions.map((a) => (
            <button
              key={a.key}
              type="button"
              disabled={a.disabled}
              onClick={a.onClick}
              className="flex flex-col items-center gap-1.5 rounded-xl border bg-card p-5 text-sm font-medium transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
            >
              {a.icon}
              {a.label}
            </button>
          ))}
        </div>
      )}

      {continuationAvailable && (
        <div className="flex flex-col gap-2">
          <Button size="lg" disabled={append.isPending} onClick={handleAdvancePhase}>
            {nextPhaseActionLabel(phase, progressionCtx)}
          </Button>
          <Button variant="ghost" size="sm" disabled={finishMatch.isPending} onClick={() => finishMatch.mutate()}>
            Or finish as a draw
          </Button>
        </div>
      )}

      {readyToFinish && (
        <Button size="lg" disabled={finishMatch.isPending} onClick={() => finishMatch.mutate()}>
          {finishMatch.isPending ? 'Finishing…' : 'Finish match'}
        </Button>
      )}

      {finishMatch.isError && (
        <p className="text-center text-xs text-destructive">{(finishMatch.error as Error).message}</p>
      )}

      <div className="border-t pt-3">
        <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Match events
        </p>
        {events.length === 0 ? (
          <p className="text-sm text-muted-foreground">No events recorded yet.</p>
        ) : (
          <div className="flex flex-col gap-2">
            {events.map((event, i) => {
              const isSideB = event.side_id === match.sides[1]?.id
              return (
                <div key={i} className={`flex items-baseline gap-2 text-sm ${isSideB ? 'flex-row-reverse text-right' : ''}`}>
                  <span className="w-10 shrink-0 text-xs text-muted-foreground">
                    {eventClockLabel(event, state?.period_times)}
                  </span>
                  <span aria-hidden>{eventEmoji(event.kind)}</span>
                  <span className="min-w-0 truncate">{describeEvent(event, match, state?.players)}</span>
                </div>
              )
            })}
          </div>
        )}
      </div>

      {append.isError && <p className="text-center text-xs text-destructive">Failed to record that event — try again.</p>}

      <RecordNetballEventDialog
        open={dialogKind !== null}
        kind={dialogKind}
        match={match}
        liveMode
        onOpenChange={(open) => !open && setDialogKind(null)}
        submitting={append.isPending}
        onSubmit={(event) => {
          append.mutate(event, {
            onSuccess: () => {
              setDialogKind(null)
              queryClient.invalidateQueries({ queryKey: ['feed'] })
            },
          })
        }}
      />
    </div>
  )
}

/**
 * Quarter-only netball scoring: no goal-by-goal detail at all — just the
 * running score, typed in fresh after each quarter, submitted as a single
 * `Period` event carrying that score (see `NetballPeriodEvent::score`'s
 * backend doc comment for why that's the *only* source of the score in this
 * mode). Walks `QUARTER_SEQUENCE` one marker at a time; each already-entered
 * quarter shows in `NetballQuarterBreakdown` below.
 */
function NetballQuarterOnlyScoringPage({
  match,
  state,
  onBackToPicker,
}: {
  match: Match
  state: NetballScore | null
  /** Present only while nothing's been recorded yet — see
   *  `NetballLiveScoringPage`'s doc comment. */
  onBackToPicker?: () => void
}) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const seq = useLiveSeq(match.id)
  const append = useAppendNetballEvent(match.id)
  const finishMatch = useFinishNetballMatch(match)

  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')
  const [sideA, sideB] = match.sides
  const aId = sideA?.id
  const bId = sideB?.id

  const recordedPeriods = new Set(Object.keys(state?.period_scores ?? {}))
  const nextPeriod = QUARTER_SEQUENCE.find((p) => !recordedPeriods.has(p)) ?? null
  const currentTotalA = aId ? (state?.score[aId] ?? 0) : 0
  const currentTotalB = bId ? (state?.score[bId] ?? 0) : 0

  const [draft, setDraft] = useState<[string, string]>(['', ''])
  useEffect(() => {
    setDraft([String(currentTotalA), String(currentTotalB)])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nextPeriod])

  const [draftA, draftB] = draft
  const a = Number(draftA)
  const b = Number(draftB)
  const canSubmit =
    !!nextPeriod && !!aId && !!bId && draftA !== '' && draftB !== '' && Number.isFinite(a) && Number.isFinite(b) && a >= 0 && b >= 0

  const submitQuarter = () => {
    if (!canSubmit || !nextPeriod || !aId || !bId) return
    append.mutate({ kind: 'Period', period: nextPeriod, score: { [aId]: a, [bId]: b } })
  }

  // Quarter-only mode has no live clock, so it never records the explicit
  // `*Start` markers event-by-event mode's breaks use (see `NetballPeriod`'s
  // backend doc comment) — `quarter_two_start`/`quarter_three_start`/
  // `quarter_four_start` are listed here only because `NetballPeriod` needs
  // every variant covered, not because this screen ever looks them up.
  const QUARTER_LABEL: Record<NetballPeriod, string> = {
    start: 'Start',
    quarter_one_end: 'End of 1st quarter',
    quarter_two_start: 'Start of 2nd quarter',
    quarter_two_end: 'End of 2nd quarter (half-time)',
    quarter_three_start: 'Start of 3rd quarter',
    quarter_three_end: 'End of 3rd quarter',
    quarter_four_start: 'Start of 4th quarter',
    full_time: 'Full-time',
    extra_time_start: 'Extra time',
    extra_time_end: 'End of extra time',
  }

  const isDraw = currentTotalA === currentTotalB
  const format = netballFormat(match.format)
  const readyToFinish = !nextPeriod || (nextPeriod === 'full_time' && recordedPeriods.has('full_time'))
  const allQuartersDone = recordedPeriods.has('full_time')
  const offerExtraTime = allQuartersDone && isDraw && format.extra_time && !recordedPeriods.has('extra_time_end')

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
          Back
        </Button>
        <div className="flex items-center gap-1">
          <UndoLastEventButton matchId={match.id} seq={seq.data} />
          <LiveIndicator />
        </div>
      </div>

      <div>
        <h1 className="text-lg font-semibold">
          {nameA} vs {nameB}
        </h1>
        <p className="text-sm text-muted-foreground">
          Scoring by quarter
          {onBackToPicker && (
            <>
              {' · '}
              <button type="button" onClick={onBackToPicker} className="text-primary hover:underline">
                Change scoring method
              </button>
            </>
          )}
        </p>
      </div>

      <div className="rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between">
          <p className="flex-1 truncate text-sm font-medium">{nameA}</p>
          <div className="px-3 text-center text-3xl font-medium tracking-tight">
            {currentTotalA}
            <span className="text-muted-foreground">–</span>
            {currentTotalB}
          </div>
          <p className="flex-1 truncate text-right text-sm font-medium">{nameB}</p>
        </div>
      </div>

      {nextPeriod && !allQuartersDone && (
        <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
          <p className="text-sm font-medium">{QUARTER_LABEL[nextPeriod]} — running score</p>
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
            <div className="flex flex-col gap-1">
              <span className="truncate text-center text-xs text-muted-foreground">{nameA}</span>
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={draftA}
                onChange={(e) => setDraft([e.target.value, draftB])}
              />
            </div>
            <span className="pt-5 text-muted-foreground">–</span>
            <div className="flex flex-col gap-1">
              <span className="truncate text-center text-xs text-muted-foreground">{nameB}</span>
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={draftB}
                onChange={(e) => setDraft([draftA, e.target.value])}
              />
            </div>
          </div>
          <Button disabled={!canSubmit || append.isPending} onClick={submitQuarter}>
            {append.isPending ? 'Saving…' : `Save ${QUARTER_LABEL[nextPeriod].toLowerCase()}`}
          </Button>
        </div>
      )}

      {offerExtraTime && (
        <Button
          variant="outline"
          disabled={!aId || !bId || append.isPending}
          onClick={() => aId && bId && append.mutate({ kind: 'Period', period: 'extra_time_start', score: { [aId]: currentTotalA, [bId]: currentTotalB } })}
        >
          Start extra time
        </Button>
      )}
      {recordedPeriods.has('extra_time_start') && !recordedPeriods.has('extra_time_end') && (
        <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
          <p className="text-sm font-medium">Extra time — final score</p>
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
            <Input
              type="number"
              min={0}
              inputMode="numeric"
              value={draftA}
              onChange={(e) => setDraft([e.target.value, draftB])}
            />
            <span className="pt-2 text-center text-muted-foreground">–</span>
            <Input
              type="number"
              min={0}
              inputMode="numeric"
              value={draftB}
              onChange={(e) => setDraft([draftA, e.target.value])}
            />
          </div>
          <Button
            disabled={!aId || !bId || draftA === '' || draftB === '' || append.isPending}
            onClick={() =>
              aId && bId && append.mutate({ kind: 'Period', period: 'extra_time_end', score: { [aId]: a, [bId]: b } })
            }
          >
            Save extra time result
          </Button>
        </div>
      )}

      {readyToFinish && !offerExtraTime && (
        <Button size="lg" disabled={finishMatch.isPending} onClick={() => finishMatch.mutate()}>
          {finishMatch.isPending ? 'Finishing…' : 'Finish match'}
        </Button>
      )}

      {(finishMatch.isError || append.isError) && (
        <p className="text-center text-xs text-destructive">
          {finishMatch.isError ? (finishMatch.error as Error).message : 'Failed to save that quarter — try again.'}
        </p>
      )}

      {state && <NetballQuarterBreakdown score={state} sideA={sideA} sideB={sideB} />}

      <Link
        to={`/matches/${match.id}`}
        className="text-center text-sm text-muted-foreground hover:underline"
        onClick={() => queryClient.invalidateQueries({ queryKey: ['feed'] })}
      >
        Done for now
      </Link>
    </div>
  )
}
