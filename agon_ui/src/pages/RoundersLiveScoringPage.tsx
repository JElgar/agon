import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { useAppendRoundersEvent, useLiveSeq } from '@/hooks/useLiveScore'
import { matchScoreQueryKey, useMatchScore } from '@/hooks/useMatchScore'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { UndoLastEventButton } from '@/components/agon/live/UndoLastEventButton'
import { SidePicker, PlayerPicker, sideName } from '@/components/agon/live/Pickers'
import { RoundersInningsCompleteDialog } from '@/components/agon/live/RoundersInningsCompleteDialog'
import { playersOnSide } from '@/lib/members'
import { roundersFormat } from '@/lib/matchFormat'
import {
  allBattersOut,
  currentRoundersInnings,
  formatHalfRounders,
  goodBallsComplete,
  outKindLabel,
  playerNameFor,
  postLabel,
  roundersScoreFrom,
  sideNameFor,
  type RoundersOutKind,
  type RoundersPost,
  type RoundersRunnerMovement,
} from '@/lib/roundersScore'

type Match = components['schemas']['Match']
type RoundersDelivery = components['schemas']['RoundersDelivery']
type RoundersInningsEndReason = components['schemas']['RoundersInningsEndReason']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

const END_REASONS: { value: RoundersInningsEndReason; label: string }[] = [
  { value: 'all_batters_out', label: 'All batters out' },
  { value: 'good_balls_complete', label: 'Good balls complete' },
  { value: 'time_up', label: "Time's up" },
]

const POSTS: RoundersPost[] = ['first', 'second', 'third', 'fourth']
const OUT_KINDS: RoundersOutKind[] = [
  'caught',
  'stumped_at_post',
  'overtook_runner',
  'obstruction',
  'failed_to_run',
]

/** A movement being drafted for one candidate (the picked striker, or a
 *  runner already on base) before it's submitted with the delivery. */
interface MovementDraft {
  post?: RoundersPost
  out?: { kind: RoundersOutKind; fielderId?: string }
}

/**
 * The live rounders scoring screen: pick a bowler and the batter facing this
 * ball, then record what every runner on the track (including the new
 * striker) did on it — mirrors `CricketLiveScoringPage`'s shape, adapted for
 * rounders' concurrent runners instead of a fixed striker/non-striker pair
 * (see `RoundersDelivery`'s doc comment on the backend). No clock, same as
 * cricket — progress is good balls/outs, derivable from the event log
 * itself.
 */
export function RoundersLiveScoringPage({ match }: { match: Match }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const scoreQuery = useMatchScore(match.id, { refetchInterval: 8000 })
  const seq = useLiveSeq(match.id)
  const appendEvent = useAppendRoundersEvent(match.id)
  const state = roundersScoreFrom(scoreQuery.data)
  const innings = state ? currentRoundersInnings(state) : null
  const format = roundersFormat(match.format)
  const next = state?.next_ball_context ?? null

  const [startBattingSide, setStartBattingSide] = useState<string | undefined>(undefined)
  const [pickedBowler, setPickedBowler] = useState<string | null>(null)
  const [pickedStriker, setPickedStriker] = useState<string | null>(null)
  const [noBall, setNoBall] = useState(false)
  const [byes, setByes] = useState(false)
  const [outcomes, setOutcomes] = useState<Record<string, MovementDraft>>({})
  const [endInningsOpen, setEndInningsOpen] = useState(false)

  // The bowler persists server-side across balls (`next_ball_context.
  // bowler_player_id`) — once it's confirmed there, drop the local pick so
  // the picker reflects the server's view, same pattern as cricket's opener
  // picks.
  useEffect(() => {
    if (next?.bowler_player_id) setPickedBowler(null)
  }, [next?.bowler_player_id])

  const outBatters = (innings?.batting ?? []).filter((b) => b.out).map((b) => b.player_id)
  const runnersOnBase = next?.runners_on_base ?? {}

  const allOut = !!innings && allBattersOut(match, innings)
  const ballsComplete = !!innings && goodBallsComplete(innings, format)
  const [allOutContinued, setAllOutContinued] = useState(false)
  const [ballsContinued, setBallsContinued] = useState(false)
  useEffect(() => {
    if (!allOut) setAllOutContinued(false)
  }, [allOut])
  useEffect(() => {
    if (!ballsComplete) setBallsContinued(false)
  }, [ballsComplete])

  // Concludes the match — same conflict-handling shape as
  // `CricketLiveScoringPage.finishMatch`: the server independently derives
  // the score from the persisted live detail and 409s if this device's view
  // has fallen behind, surfaced rather than silently resolved.
  const finishMatch = useMutation({
    mutationFn: async () => {
      if (!state) throw new Error('No score recorded yet')
      const fresh = roundersScoreFrom(
        queryClient.getQueryData<Score | null>(matchScoreQueryKey(match.id)),
      )
      const body: UpdateMatchInput = {
        status: 'completed',
        score: fresh ?? undefined,
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

  const header = (
    <div className="flex items-center justify-between">
      <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
        <ChevronLeft className="size-4" /> Back
      </Button>
      <div className="flex items-center gap-1">
        <UndoLastEventButton matchId={match.id} seq={seq.data} />
        <LiveIndicator />
      </div>
    </div>
  )

  // No innings open — either the match hasn't started, or we're between
  // innings. Pick who's batting (the other side fields).
  if (!innings) {
    const inningsQuota = match.sides.length * format.innings_per_side
    const allInningsComplete = !!state && state.innings.length >= inningsQuota

    return (
      <div className="mx-auto flex max-w-xl flex-col gap-4">
        {header}
        <div>
          <h1 className="text-lg font-semibold">
            {match.sides[0]?.name?.trim() || 'Side A'} vs {match.sides[1]?.name?.trim() || 'Side B'}
          </h1>
          <p className="text-sm text-muted-foreground">
            {allInningsComplete
              ? 'All innings complete'
              : state && state.innings.length > 0
                ? 'Start the next innings'
                : "You're scoring this match"}
          </p>
        </div>
        {state && state.innings.length > 0 && (
          <div className="rounded-xl border bg-card p-4">
            <div className="space-y-3">
              {state.innings.map((inn, i) => (
                <div key={i}>
                  <p className="mb-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                    {sideNameFor(match, inn.batting_side_id)}
                  </p>
                  <p className="text-2xl font-medium tracking-tight">
                    {formatHalfRounders(inn.half_rounders)}
                    <span className="ml-2 text-sm font-normal text-muted-foreground">
                      ({inn.outs} out · {inn.good_balls_bowled} balls)
                    </span>
                  </p>
                </div>
              ))}
            </div>
          </div>
        )}

        {!allInningsComplete && (
          <>
            <div className="rounded-xl border bg-card p-4">
              <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
                Who's batting?
              </p>
              <SidePicker sides={match.sides} value={startBattingSide} onChange={setStartBattingSide} />
            </div>
            <Button
              size="lg"
              disabled={!startBattingSide || appendEvent.isPending}
              onClick={() => {
                const fieldingSideId = match.sides.find((s) => s.id !== startBattingSide)?.id
                if (!startBattingSide || !fieldingSideId) return
                appendEvent.mutate(
                  {
                    kind: 'InningsStart',
                    batting_side_id: startBattingSide,
                    fielding_side_id: fieldingSideId,
                  },
                  { onSuccess: () => setStartBattingSide(undefined) },
                )
              }}
            >
              {appendEvent.isPending ? 'Starting…' : 'Start innings'}
            </Button>
          </>
        )}

        {state && state.innings.length > 0 && (
          <>
            {!allInningsComplete && <p className="text-center text-xs text-muted-foreground">or</p>}
            {finishMatch.isError && (
              <p className="text-center text-xs text-destructive">
                {(finishMatch.error as Error).message}
              </p>
            )}
            <Button
              variant={allInningsComplete ? 'default' : 'outline'}
              size="lg"
              disabled={finishMatch.isPending}
              onClick={() => finishMatch.mutate()}
            >
              {finishMatch.isPending ? 'Finishing…' : 'Finish match'}
            </Button>
          </>
        )}
      </div>
    )
  }

  const battingSide = match.sides.find((s) => s.id === innings.batting_side_id)
  const fieldingSide = match.sides.find((s) => s.id === innings.fielding_side_id)
  const effectiveBowler = next?.bowler_player_id ?? pickedBowler
  // A candidate for a movement this ball: the newly picked striker, plus
  // every runner already on base — deduplicated in case a runner and the
  // striker pick somehow coincide.
  const candidates = Array.from(
    new Set([...(pickedStriker ? [pickedStriker] : []), ...Object.keys(runnersOnBase)]),
  )

  const setOutcome = (playerId: string, patch: MovementDraft | undefined) =>
    setOutcomes((o) => {
      if (!patch) {
        const rest = { ...o }
        delete rest[playerId]
        return rest
      }
      return { ...o, [playerId]: patch }
    })

  const resetBall = () => {
    setPickedStriker(null)
    setOutcomes({})
    setNoBall(false)
    setByes(false)
  }

  const recordDelivery = () => {
    if (!effectiveBowler || !pickedStriker) return
    const movements: RoundersRunnerMovement[] = candidates.flatMap((playerId): RoundersRunnerMovement[] => {
      const o = outcomes[playerId]
      if (!o) return []
      if (o.out) {
        return [{ player_id: playerId, out: { kind: o.out.kind, fielder_player_id: o.out.fielderId } }]
      }
      if (o.post) return [{ player_id: playerId, post_reached: o.post }]
      return []
    })
    const delivery: RoundersDelivery = {
      bowler_player_id: effectiveBowler,
      striker_player_id: pickedStriker,
      no_ball: noBall,
      byes,
      movements,
    }
    appendEvent.mutate({ kind: 'Delivery', ...delivery }, { onSuccess: resetBall })
  }

  const endInnings = (reason: RoundersInningsEndReason) => {
    appendEvent.mutate(
      { kind: 'InningsEnd', reason },
      { onSuccess: () => setEndInningsOpen(false) },
    )
  }

  const goodBallsLabel = format.good_balls_per_innings
    ? `${innings.good_balls_bowled}/${format.good_balls_per_innings}`
    : `${innings.good_balls_bowled}`

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      {header}

      <div>
        <h1 className="text-lg font-semibold">
          {sideName(battingSide!, 'Side A')} vs {sideName(fieldingSide!, 'Side B')}
        </h1>
        <p className="text-sm text-muted-foreground">You're scoring this match</p>
      </div>

      <div className="rounded-xl border bg-card p-4">
        <p className="text-sm font-medium">{sideName(battingSide!, 'Side A')} batting</p>
        <p className="mt-0.5 text-3xl font-medium tracking-tight">
          {formatHalfRounders(innings.half_rounders)}
          <span className="ml-2 text-sm font-normal text-muted-foreground">
            ({innings.outs} out · {goodBallsLabel} balls)
          </span>
        </p>

        {Object.keys(runnersOnBase).length > 0 && (
          <div className="mt-3 space-y-1 border-t pt-3 text-sm">
            {Object.entries(runnersOnBase).map(([playerId, post]) => (
              <div key={playerId} className="flex items-center justify-between">
                <span>{playerNameFor(match, playerId, state?.players) ?? '—'}</span>
                <span className="text-xs text-muted-foreground">{postLabel(post)}</span>
              </div>
            ))}
          </div>
        )}

        {effectiveBowler && (
          <div className="mt-1.5 flex items-center justify-between border-t pt-1.5 text-xs">
            <span className="text-muted-foreground">
              Bowling:{' '}
              <span className="font-medium text-foreground">
                {playerNameFor(match, effectiveBowler, state?.players) ?? '—'}
              </span>
            </span>
          </div>
        )}
      </div>

      <div className="rounded-xl border bg-card p-4">
        <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">Bowler</p>
        <PlayerPicker
          players={playersOnSide(match.players, fieldingSide!)}
          value={effectiveBowler ?? undefined}
          onChange={setPickedBowler}
        />
      </div>

      <div className="rounded-xl border bg-card p-4">
        <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Who's facing this ball?
        </p>
        <PlayerPicker
          players={playersOnSide(match.players, battingSide!)}
          value={pickedStriker ?? undefined}
          exclude={[...Object.keys(runnersOnBase), ...outBatters]}
          onChange={setPickedStriker}
        />
      </div>

      <div className="flex gap-4">
        <label className="flex items-center gap-1.5 text-sm">
          <input type="checkbox" checked={noBall} onChange={(e) => setNoBall(e.target.checked)} />
          No ball
        </label>
        <label className="flex items-center gap-1.5 text-sm">
          <input type="checkbox" checked={byes} onChange={(e) => setByes(e.target.checked)} />
          Byes
        </label>
      </div>

      {candidates.length > 0 && (
        <div className="flex flex-col gap-3">
          <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            What happened
          </p>
          {candidates.map((playerId) => (
            <MovementRow
              key={playerId}
              name={playerNameFor(match, playerId, state?.players) ?? '—'}
              fieldingRoster={playersOnSide(match.players, fieldingSide!)}
              value={outcomes[playerId]}
              onChange={(v) => setOutcome(playerId, v)}
            />
          ))}
        </div>
      )}

      <Button
        size="lg"
        disabled={!effectiveBowler || !pickedStriker || appendEvent.isPending}
        onClick={recordDelivery}
      >
        {appendEvent.isPending ? 'Recording…' : 'Record ball'}
      </Button>

      {appendEvent.isError && (
        <p className="text-center text-xs text-destructive">
          Failed to record that ball — try again.
        </p>
      )}

      {endInningsOpen ? (
        <div className="rounded-xl border border-destructive/30 bg-destructive/5 p-4">
          <p className="mb-2 text-sm font-medium">End this innings — why?</p>
          <div className="grid grid-cols-2 gap-2">
            {END_REASONS.map((r) => (
              <Button
                key={r.value}
                variant="outline"
                size="sm"
                disabled={appendEvent.isPending}
                onClick={() => endInnings(r.value)}
              >
                {r.label}
              </Button>
            ))}
          </div>
          <Button variant="ghost" size="sm" className="mt-2" onClick={() => setEndInningsOpen(false)}>
            Cancel
          </Button>
        </div>
      ) : (
        <Button variant="outline" onClick={() => setEndInningsOpen(true)}>
          End innings
        </Button>
      )}

      {allOut && (
        <RoundersInningsCompleteDialog
          open={!allOutContinued}
          battingSideName={sideName(battingSide!, 'Side A')}
          reason="all_batters_out"
          submitting={appendEvent.isPending}
          onEnd={() => endInnings('all_batters_out')}
          onContinue={() => setAllOutContinued(true)}
        />
      )}

      {ballsComplete && !allOut && (
        <RoundersInningsCompleteDialog
          open={!ballsContinued}
          battingSideName={sideName(battingSide!, 'Side A')}
          reason="good_balls_complete"
          submitting={appendEvent.isPending}
          onEnd={() => endInnings('good_balls_complete')}
          onContinue={() => setBallsContinued(true)}
        />
      )}
    </div>
  )
}

/** One candidate's outcome picker for the current ball: stayed (default,
 *  nothing picked), a post they reached, or out (with a kind and optional
 *  fielder). */
function MovementRow({
  name,
  fieldingRoster,
  value,
  onChange,
}: {
  name: string
  fieldingRoster: components['schemas']['MatchPlayer'][]
  value: MovementDraft | undefined
  onChange: (v: MovementDraft | undefined) => void
}) {
  const isOut = !!value?.out

  return (
    <div className="rounded-lg border p-3">
      <p className="mb-2 text-sm font-medium">{name}</p>
      <div className="grid grid-cols-3 gap-1.5">
        <button
          type="button"
          aria-pressed={!value}
          onClick={() => onChange(undefined)}
          className={`rounded border px-2 py-1.5 text-xs font-medium ${!value ? 'border-primary bg-accent' : 'text-muted-foreground'}`}
        >
          Stayed
        </button>
        {POSTS.map((post) => (
          <button
            key={post}
            type="button"
            aria-pressed={value?.post === post}
            onClick={() => onChange({ post })}
            className={`rounded border px-2 py-1.5 text-xs font-medium ${value?.post === post ? 'border-primary bg-accent' : 'text-muted-foreground'}`}
          >
            {postLabel(post)}
          </button>
        ))}
        <button
          type="button"
          aria-pressed={isOut}
          onClick={() => onChange({ out: { kind: value?.out?.kind ?? 'caught', fielderId: value?.out?.fielderId } })}
          className={`rounded border px-2 py-1.5 text-xs font-medium ${isOut ? 'border-destructive bg-destructive/10 text-destructive' : 'text-muted-foreground'}`}
        >
          Out
        </button>
      </div>

      {isOut && (
        <div className="mt-2 flex flex-col gap-1.5 border-t pt-2">
          <div className="grid grid-cols-2 gap-1">
            {OUT_KINDS.map((k) => (
              <button
                key={k}
                type="button"
                aria-pressed={value?.out?.kind === k}
                onClick={() => onChange({ out: { kind: k, fielderId: value?.out?.fielderId } })}
                className={`rounded border px-1.5 py-1 text-[10px] ${value?.out?.kind === k ? 'border-primary bg-accent' : 'text-muted-foreground'}`}
              >
                {outKindLabel(k)}
              </button>
            ))}
          </div>
          <PlayerPicker
            players={fieldingRoster}
            value={value?.out?.fielderId}
            emptyLabel="Fielder (optional)"
            onChange={(id) =>
              onChange({
                out: { kind: value?.out?.kind ?? 'caught', fielderId: value?.out?.fielderId === id ? undefined : id },
              })
            }
          />
        </div>
      )}
    </div>
  )
}
