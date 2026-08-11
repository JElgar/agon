import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams, Link } from 'react-router-dom'
import { ChevronLeft, CircleDot, Flag, Repeat2, TimerReset } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { useAppendFootballEvent, useLiveSeq } from '@/hooks/useLiveScore'
import { matchScoreQueryKey, useMatchScore } from '@/hooks/useMatchScore'
import { RecordEventDialog, type EventKind } from '@/components/agon/live/RecordEventDialog'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { UndoLastEventButton } from '@/components/agon/live/UndoLastEventButton'
import { CricketLiveScoringPage } from './CricketLiveScoringPage'
import { footballFormat } from '@/lib/matchFormat'
import {
  currentMinute,
  describeEvent,
  eventEmoji,
  eventsFromDetail,
  footballScoreFrom,
  isLivePlayPhase,
  loadTrackPrefs,
  nextPeriodForPhase,
  nextPhaseActionLabel,
  penaltiesComplete,
  phaseFromState,
  phaseLabel,
  shootoutScoreFor,
  type ClockPhase,
} from '@/lib/liveScore'

type Match = components['schemas']['Match']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

function sideName(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/**
 * Route entry for `/matches/:matchId/live`: fetches the match once and
 * dispatches to the sport-specific scoring screen. Football and cricket have
 * different live-scoring shapes entirely (a running clock vs. overs/wickets),
 * so past the match fetch they don't share a component.
 */
export function LiveScoringPage() {
  const { matchId } = useParams()

  const matchQuery = useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  if (matchQuery.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  if (matchQuery.isError || !matchQuery.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this match.</p>
        <Button variant="outline" onClick={() => matchQuery.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const match = matchQuery.data
  if (match.match_type === 'cricket') {
    return <CricketLiveScoringPage match={match} />
  }
  return <FootballLiveScoringPage match={match} />
}

/** How often the on-screen clock re-renders while a half is running. */
const CLOCK_TICK_MS = 15_000

/**
 * The live scoring screen: quick actions to log goals/cards/subs as they
 * happen, a running clock, and the event log so far. The clock is computed
 * from the server-recorded kickoff/half-time timestamps (see `lib/liveScore`),
 * so it's the same for every viewer, not just this device.
 */
function FootballLiveScoringPage({ match }: { match: Match }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const scoreQuery = useMatchScore(match.id, { refetchInterval: 8000 })
  const seq = useLiveSeq(match.id)
  const append = useAppendFootballEvent(match.id)

  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), CLOCK_TICK_MS)
    return () => clearInterval(id)
  }, [])

  const prefs = loadTrackPrefs(match.id)
  const [dialogKind, setDialogKind] = useState<EventKind | null>(null)

  // Concludes the match once it's decided (full-time, extra-time full-time,
  // or the penalty shootout is done): flips the match to `completed`,
  // sending the score built from this device's own view of the live detail.
  // The server independently derives the same score from the match's
  // persisted live detail and 409s if the two disagree (e.g. a goal landed
  // between this device's last poll and the tap). That's surfaced, not
  // silently resolved: the live detail is refetched (so the score shown
  // above updates to what's actually current) and the button stays
  // "Finish match" so the user reviews it and taps again themselves, rather
  // than the app resubmitting on their behalf without them seeing what
  // changed. Still enters the normal confirmation flow rather than being
  // auto-confirmed, same as a manual "Add result" would.
  const finishMatch = useMutation({
    mutationFn: async () => {
      const state = footballScoreFrom(
        queryClient.getQueryData<Score | null>(matchScoreQueryKey(match.id)),
      )
      const body: UpdateMatchInput = {
        status: 'completed',
        score: state ?? undefined,
      }
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

  if (scoreQuery.isLoading || seq.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')
  const state = footballScoreFrom(scoreQuery.data)
  const goalsFor = (sideId: string | undefined) =>
    (sideId ? state?.score[sideId] : undefined) ?? 0
  const format = footballFormat(match.format)
  const aId = match.sides[0]?.id
  const bId = match.sides[1]?.id

  // Before the first event, there's no snapshot yet at all — treat that the
  // same as an explicit "not started" phase (kickoff just hasn't happened).
  const phase = state ? phaseFromState(state) : 'not_started'
  const minute = state ? currentMinute(state, now) : null
  const isDraw = !!state && goalsFor(aId) === goalsFor(bId)
  const progressionCtx = { isDraw, extraTime: format.extra_time }

  const handleHalfFt = () => {
    const period = nextPeriodForPhase(phase, progressionCtx)
    if (!period) return
    append.mutate({ kind: 'Period', period })
  }

  // These phases are where the match is up for grabs on a period-marker
  // "advance the clock" tap (kickoff/half-time/full-time and their extra-time
  // equivalents). `full_time`/`extra_time_full_time` are deliberately
  // excluded — they're decision points (finish, or continue into extra
  // time/penalties) handled by the dedicated panel below instead of this
  // grid tile, and `penalties` isn't clocked at all (see `ClockPhase`).
  const advanceClockPhases: ClockPhase[] = [
    'not_started',
    'first_half',
    'half_time',
    'second_half',
    'extra_time_first_half',
    'extra_time_half_time',
    'extra_time_second_half',
  ]
  const showAdvanceClockTile = advanceClockPhases.includes(phase)

  // Goal/card/sub are only offered while the ball's actually in play — not
  // before kickoff, not at half-time/full-time, not during extra-time's
  // break, and not during the penalty shootout (which has its own recording
  // panel below). The clock-advance tile (kick off / start 2nd half / etc.)
  // is offered independently via `showAdvanceClockTile`, so a break phase
  // still gets its "move to the next phase" action even with no live events.
  const actions: {
    key: EventKind | 'half_ft'
    label: string
    icon: React.ReactNode
    onClick: () => void
    disabled?: boolean
  }[] = [
    ...(isLivePlayPhase(phase)
      ? [
          { key: 'goal' as const, label: 'Goal', icon: <CircleDot className="size-5" />, onClick: () => setDialogKind('goal') },
          ...(prefs.cards
            ? [{ key: 'card' as const, label: 'Card', icon: <Flag className="size-5" />, onClick: () => setDialogKind('card') }]
            : []),
          ...(prefs.substitutions
            ? [
                {
                  key: 'substitution' as const,
                  label: 'Sub',
                  icon: <Repeat2 className="size-5" />,
                  onClick: () => setDialogKind('substitution'),
                },
              ]
            : []),
        ]
      : []),
    ...(showAdvanceClockTile
      ? [
          {
            key: 'half_ft' as const,
            label: nextPhaseActionLabel(phase, progressionCtx),
            icon: <TimerReset className="size-5" />,
            onClick: handleHalfFt,
            disabled: nextPeriodForPhase(phase, progressionCtx) === null,
          },
        ]
      : []),
  ]

  // At full-time/extra-time-full-time still level, the format may call for
  // more play — offer it, with an explicit override to finish as a draw
  // anyway (the format is a plan, not a rule the organiser is locked into).
  // Otherwise (decided on goals, or level with no more play configured) the
  // match is ready to finish. The penalty shootout has its own completion
  // signal (`penaltiesComplete`) instead of a phase-advance action.
  const decidingPhase = phase === 'full_time' || phase === 'extra_time_full_time'
  const continuationAvailable =
    decidingPhase && isDraw && (phase === 'full_time' ? format.extra_time : format.penalties)
  const readyToFinish =
    (decidingPhase && !continuationAvailable) || (phase === 'penalties' && !!state && penaltiesComplete(state))

  const events = state
    ? eventsFromDetail({
        goals: state.goals ?? [],
        cards: state.cards ?? [],
        substitutions: state.substitutions ?? [],
        players: state.players,
      }).reverse()
    : []

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
          <ChevronLeft className="size-4" /> Back
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
            {phase === 'penalties' && (
              <div className="mt-1 text-lg font-medium tracking-tight text-muted-foreground">
                {state && shootoutScoreFor(state, aId)}
                <span className="mx-1 text-xs">pens</span>
                {state && shootoutScoreFor(state, bId)}
              </div>
            )}
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

      {phase === 'penalties' && state && !penaltiesComplete(state) && (
        <div className="grid grid-cols-2 gap-3">
          {([
            [aId, nameA],
            [bId, nameB],
          ] as const).map(([sideId, name]) => (
            <div key={sideId} className="flex flex-col gap-2 rounded-xl border bg-card p-4">
              <p className="truncate text-center text-xs font-medium uppercase tracking-wider text-muted-foreground">
                {name}
              </p>
              <Button
                variant="outline"
                disabled={append.isPending || !sideId}
                onClick={() => sideId && append.mutate({ kind: 'PenaltyShootoutKick', side_id: sideId, scored: true })}
              >
                Scored
              </Button>
              <Button
                variant="ghost"
                disabled={append.isPending || !sideId}
                onClick={() => sideId && append.mutate({ kind: 'PenaltyShootoutKick', side_id: sideId, scored: false })}
              >
                Missed
              </Button>
            </div>
          ))}
        </div>
      )}

      {phase === 'penalties' && state && (state.penalty_shootout?.length ?? 0) > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {(state.penalty_shootout ?? []).map((kick, i) => (
            <span
              key={i}
              className={`flex size-7 items-center justify-center rounded-full text-xs font-semibold ${
                kick.scored ? 'bg-primary/15 text-primary' : 'bg-muted text-muted-foreground'
              }`}
              title={kick.side_id === aId ? nameA : nameB}
            >
              {kick.scored ? '✓' : '✗'}
            </span>
          ))}
        </div>
      )}

      {phase === 'penalties' && state && !penaltiesComplete(state) && (
        <Button
          variant="outline"
          disabled={append.isPending}
          onClick={() => append.mutate({ kind: 'Period', period: 'penalties_complete' })}
        >
          End penalties
        </Button>
      )}

      {continuationAvailable && (
        <div className="flex flex-col gap-2">
          <Button size="lg" disabled={append.isPending} onClick={handleHalfFt}>
            {nextPhaseActionLabel(phase, progressionCtx)}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={finishMatch.isPending}
            onClick={() => finishMatch.mutate()}
          >
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
        <p className="text-center text-xs text-destructive">
          {(finishMatch.error as Error).message}
        </p>
      )}

      <Link
        to={`/matches/${match.id}/live/setup`}
        className="text-center text-sm text-primary hover:underline"
      >
        + Track more (cards, subs)
      </Link>

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
                <div
                  key={i}
                  className={`flex items-baseline gap-2 text-sm ${isSideB ? 'flex-row-reverse text-right' : ''}`}
                >
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

      {append.isError && (
        <p className="text-center text-xs text-destructive">
          Failed to record that event — try again.
        </p>
      )}

      <RecordEventDialog
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
