import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams, Link } from 'react-router-dom'
import { ChevronLeft, CircleDot, Flag, Repeat2, TimerReset } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { useLiveScore, useAppendFootballEvent } from '@/hooks/useLiveScore'
import { RecordEventDialog, type EventKind } from '@/components/agon/live/RecordEventDialog'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { CricketLiveScoringPage } from './CricketLiveScoringPage'
import {
  currentMinute,
  describeEvent,
  eventEmoji,
  footballLiveState,
  loadTrackPrefs,
  nextPeriodForPhase,
  nextPhaseActionLabel,
  phaseFromState,
  phaseLabel,
} from '@/lib/liveScore'

type Match = components['schemas']['Match']

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

  const live = useLiveScore(match.id, { refetchInterval: 8000 })
  const append = useAppendFootballEvent(match.id)

  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), CLOCK_TICK_MS)
    return () => clearInterval(id)
  }, [])

  const prefs = loadTrackPrefs(match.id)
  const [dialogKind, setDialogKind] = useState<EventKind | null>(null)

  if (live.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')
  const state = footballLiveState(live.data)
  const goalsFor = (sideId: string | undefined) =>
    state?.score.find((s) => s.side_id === sideId)?.goals ?? 0

  // Before the first event, there's no snapshot yet at all — treat that the
  // same as an explicit "not started" phase (kickoff just hasn't happened).
  const phase = state ? phaseFromState(state) : 'not_started'
  const minute = state ? currentMinute(state, now) : null

  const handleHalfFt = () => {
    const period = nextPeriodForPhase(phase)
    if (!period) return
    append.mutate({ kind: 'Period', period })
  }

  const actions: {
    key: EventKind | 'half_ft'
    label: string
    icon: React.ReactNode
    onClick: () => void
    disabled?: boolean
  }[] = [
    { key: 'goal', label: 'Goal', icon: <CircleDot className="size-5" />, onClick: () => setDialogKind('goal') },
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
    {
      key: 'half_ft',
      label: nextPhaseActionLabel(phase),
      icon: <TimerReset className="size-5" />,
      onClick: handleHalfFt,
      disabled: nextPeriodForPhase(phase) === null,
    },
  ]

  const events = state ? [...state.events].reverse() : []

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
          <ChevronLeft className="size-4" /> Back
        </Button>
        <LiveIndicator />
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
              {goalsFor(match.sides[0]?.id)}
              <span className="text-muted-foreground">–</span>
              {goalsFor(match.sides[1]?.id)}
            </div>
            <div className="mt-0.5 text-xs text-primary">
              {minute !== null && `${minute}' · `}
              {phaseLabel(phase)}
            </div>
          </div>
          <p className="flex-1 truncate text-right text-sm font-medium">{nameB}</p>
        </div>
      </div>

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
            {events.map((event, i) => (
              <div key={i} className="flex items-baseline gap-2 text-sm">
                <span className="w-8 shrink-0 text-xs text-muted-foreground">
                  {event.minute !== undefined ? `${event.minute}'` : ''}
                </span>
                <span aria-hidden>{eventEmoji(event.kind)}</span>
                <span className="min-w-0 truncate">{describeEvent(event, match)}</span>
              </div>
            ))}
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
