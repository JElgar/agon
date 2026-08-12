import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate, Link } from 'react-router-dom'
import { CircleDot, OctagonAlert, TimerReset } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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
  currentMinute,
  describeEvent,
  eventEmoji,
  eventsFromDetail,
  isLivePlayPhase,
  loadNetballScoringMethod,
  netballScoreFrom,
  nextPeriodForPhase,
  nextPhaseActionLabel,
  phaseFromState,
  phaseLabel,
  saveNetballScoringMethod,
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
 * hands off to the matching screen. Once a match has *any* live state,
 * the method is inferred from it instead (goal-by-goal detail present =
 * event-by-event; otherwise quarter-only) rather than trusting the saved
 * preference, so a second device picking up an already-started match
 * renders the right screen even without its own local preference.
 */
export function NetballLiveScoringPage({ match }: { match: Match }) {
  const scoreQuery = useMatchScore(match.id, { refetchInterval: 8000 })
  const state = netballScoreFrom(scoreQuery.data)
  // Only used as the picker's initial value below — once there's *any* live
  // state, `method` is inferred from it instead (see the picker's own doc
  // comment), so a stale/never-set local preference can't override reality.
  const [pickedMethod, setPickedMethod] = useState<NetballScoringMethod | null>(() =>
    loadNetballScoringMethod(match.id),
  )

  if (scoreQuery.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  const inferredMethod: NetballScoringMethod | null = state
    ? (state.goals?.length ?? 0) > 0
      ? 'event_by_event'
      : 'quarter_only'
    : null
  const method = inferredMethod ?? pickedMethod

  if (!method) {
    return (
      <NetballScoringMethodPicker
        match={match}
        onChoose={(m) => {
          saveNetballScoringMethod(match.id, m)
          setPickedMethod(m)
        }}
      />
    )
  }

  return method === 'event_by_event' ? (
    <NetballEventByEventScoringPage match={match} state={state} />
  ) : (
    <NetballQuarterOnlyScoringPage match={match} state={state} />
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

/** How often the on-screen clock re-renders while a quarter is running. */
const CLOCK_TICK_MS = 15_000

/**
 * Event-by-event netball scoring: quick actions to log goals/fouls as they
 * happen, a running per-quarter clock, and the event log so far — the
 * netball equivalent of football's `LiveScoringPage`. Advancing the clock
 * (end of a quarter) auto-fills the `Period` marker's `score` from the
 * running goal tally the client already has — the user never types a number
 * here; that's quarter-only mode's job (`NetballQuarterOnlyScoringPage`).
 */
function NetballEventByEventScoringPage({ match, state }: { match: Match; state: NetballScore | null }) {
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
  const minute = state ? currentMinute(state, now) : null
  const isDraw = !!state && goalsFor(aId) === goalsFor(bId)
  const progressionCtx = { isDraw, extraTime: format.extra_time }

  const handleQuarterEnd = () => {
    const period = nextPeriodForPhase(phase, progressionCtx)
    if (!period) return
    append.mutate({ kind: 'Period', period, score: state?.score ?? {} })
  }

  const advanceClockPhases: ClockPhase[] = ['not_started', 'quarter_1', 'quarter_2', 'quarter_3', 'quarter_4', 'extra_time']
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
            key: 'quarter_end',
            label: nextPhaseActionLabel(phase, progressionCtx),
            icon: <TimerReset className="size-5" />,
            onClick: handleQuarterEnd,
            disabled: nextPeriodForPhase(phase, progressionCtx) === null,
          },
        ]
      : []),
  ]

  const decidingPhase = phase === 'full_time'
  const continuationAvailable = decidingPhase && isDraw && format.extra_time
  const readyToFinish = (decidingPhase && !continuationAvailable) || phase === 'finished'

  const events = state
    ? eventsFromDetail({ goals: state.goals ?? [], fouls: state.fouls ?? [], players: state.players }).reverse()
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
        <p className="text-sm text-muted-foreground">You're scoring this match</p>
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
            <div className="mt-0.5 text-xs text-primary">
              {minute !== null && `${minute}' · `}
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
          <Button size="lg" disabled={append.isPending} onClick={handleQuarterEnd}>
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
                  <span className="w-8 shrink-0 text-xs text-muted-foreground">
                    {event.minute !== undefined ? `${event.minute}'` : ''}
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
        initialMinute={minute ?? 0}
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
function NetballQuarterOnlyScoringPage({ match, state }: { match: Match; state: NetballScore | null }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
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

  const QUARTER_LABEL: Record<NetballPeriod, string> = {
    start: 'Start',
    quarter_one_end: 'End of 1st quarter',
    quarter_two_end: 'End of 2nd quarter (half-time)',
    quarter_three_end: 'End of 3rd quarter',
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
        <LiveIndicator />
      </div>

      <div>
        <h1 className="text-lg font-semibold">
          {nameA} vs {nameB}
        </h1>
        <p className="text-sm text-muted-foreground">Scoring by quarter</p>
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
