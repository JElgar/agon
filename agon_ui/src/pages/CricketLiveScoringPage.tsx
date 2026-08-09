import { useEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { useLiveSeq } from '@/hooks/useLiveScore'
import { useOfflineCricketScoring } from '@/hooks/useOfflineLiveScoring'
import { matchScoreQueryKey, useMatchScore } from '@/hooks/useMatchScore'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { OfflineIndicator } from '@/components/agon/live/OfflineIndicator'
import { ConflictBanner } from '@/components/agon/live/ConflictBanner'
import { SidePicker, PlayerPicker, sideName } from '@/components/agon/live/Pickers'
import { WicketDialog } from '@/components/agon/live/WicketDialog'
import { ExtraRunsDialog } from '@/components/agon/live/ExtraRunsDialog'
import { NoBallDialog } from '@/components/agon/live/NoBallDialog'
import { playersOnSide } from '@/lib/members'
import { cricketFormat } from '@/lib/matchFormat'
import {
  cricketScoreFrom,
  currentInnings,
  currentOverDeliveries,
  deliveryChipLabel,
  formatOvers,
  isChipHighlighted,
  playerNameFor,
  runRate,
  type CricketLiveEvent,
} from '@/lib/cricketScore'

/** How long a quick-action button stays disabled after a tap — `recordEvent`
 *  (see `useOfflineLiveScoring`) folds and returns synchronously rather than
 *  waiting on a network round-trip, so the old `mutation.isPending`-driven
 *  double-tap guard no longer exists on its own; this replaces it with an
 *  explicit short lock instead of leaving a fast double-tap free to record
 *  the same ball twice. */
const RECORD_DEBOUNCE_MS = 400

type Match = components['schemas']['Match']
type CricketDelivery = components['schemas']['CricketDelivery']
type CricketDeliveryWicket = components['schemas']['CricketDeliveryWicket']
type InningsEndReason = components['schemas']['InningsEndReason']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

const END_REASONS: { value: InningsEndReason; label: string }[] = [
  { value: 'all_out', label: 'All out' },
  { value: 'overs_complete', label: 'Overs complete' },
  { value: 'declared', label: 'Declared' },
  { value: 'target_reached', label: 'Target reached' },
]

/**
 * The live cricket scoring screen: ball-by-ball entry (runs, extras,
 * wickets), the current over, and the batting/bowling summary. Unlike
 * football there's no clock — progress is overs/deliveries, all derivable
 * from the event log itself (see `lib/cricketScore`), so no backend changes
 * were needed to support this screen.
 */
export function CricketLiveScoringPage({ match }: { match: Match }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const scoreQuery = useMatchScore(match.id, { refetchInterval: 8000 })
  const seq = useLiveSeq(match.id)
  const format = cricketFormat(match.format)
  const offline = useOfflineCricketScoring(match.id, format)
  const state = cricketScoreFrom(scoreQuery.data)
  const innings = state ? currentInnings(state) : null
  // Folded incrementally as events are recorded — server-side once synced
  // (`live_score::cricket::apply_delivery`), or locally by this same fold
  // ported to TS (`lib/cricketFold.ts`) the moment a ball is recorded,
  // ahead of the network round-trip either way (see `useOfflineLiveScoring`)
  // — so this always reflects the latest tap, not just the latest sync.
  const next = state?.next_ball_context ?? null

  // Fold-then-return (not a network round-trip) means there's no
  // `mutation.isPending` to key a double-tap guard off anymore — this
  // stands in for it. See `RECORD_DEBOUNCE_MS`.
  const [recordLocked, setRecordLocked] = useState(false)
  const record = (event: CricketLiveEvent) => {
    if (recordLocked) return
    setRecordLocked(true)
    offline.recordEvent(event)
    setTimeout(() => setRecordLocked(false), RECORD_DEBOUNCE_MS)
  }

  // The over quota's been fully bowled but nothing on the backend blocks
  // further deliveries (see `match_format`'s doc comment — enforcement is
  // intentionally out of scope there), so the picker flow would otherwise
  // just keep asking for a new bowler forever. The reason is unambiguous
  // here — the overs ran out, nobody needs to be asked — so this ends the
  // innings itself instead of prompting (see the auto-end effect below).
  const oversComplete =
    !!innings &&
    format.overs_per_innings != null &&
    innings.overs.overs >= format.overs_per_innings &&
    innings.overs.balls === 0
  const oversCompleteAwaitingEnd = oversComplete && !next?.bowler_player_id

  // Locally-picked openers/replacement batter/next bowler — only used until
  // the server confirms them via an actual delivery, at which point
  // `next_ball_context` takes over as the source of truth.
  const [pickedStriker, setPickedStriker] = useState<string | null>(null)
  const [pickedNonStriker, setPickedNonStriker] = useState<string | null>(null)
  const [pickedBowler, setPickedBowler] = useState<string | null>(null)
  useEffect(() => {
    if (next?.striker_player_id) setPickedStriker(null)
    if (next?.non_striker_player_id) setPickedNonStriker(null)
    if (next?.bowler_player_id) setPickedBowler(null)
  }, [next?.striker_player_id, next?.non_striker_player_id, next?.bowler_player_id])

  // Lets the scorer step back into the opener/bowler picker after all three
  // are picked, to fix a mis-tap — only while the innings' very first ball
  // hasn't been recorded yet, since the picks are still purely local state at
  // that point (nothing on the server to correct). Once a real delivery
  // exists corrections need to go through amending/deleting that event
  // instead (see `live_score::cricket`'s doc comment) — broader in-innings
  // correction support is a later step.
  const [editingOpeningPicks, setEditingOpeningPicks] = useState(false)
  const openingPicksUnconfirmed = innings ? (state?.recent_deliveries?.length ?? 0) === 0 : false
  useEffect(() => {
    if (!openingPicksUnconfirmed) setEditingOpeningPicks(false)
  }, [openingPicksUnconfirmed])

  const [wicketOpen, setWicketOpen] = useState(false)
  const [extraDialog, setExtraDialog] = useState<'no_ball' | 'bye' | null>(null)
  const [endInningsOpen, setEndInningsOpen] = useState(false)
  const [startBattingSide, setStartBattingSide] = useState<string | undefined>(undefined)

  // Fires the `overs_complete` end-of-innings event itself the moment the
  // quota's used up, rather than asking the scorer to pick a reason they
  // didn't choose. Guarded by a ref so it fires exactly once per innings,
  // and resets once the innings actually ends (`oversComplete` goes false)
  // so the next innings' overs-complete moment can auto-end too.
  // `recordEvent` folds locally immediately regardless of connectivity (see
  // `useOfflineLiveScoring`), so there's no "did this actually go through"
  // step to wait on here the way an in-flight mutation would need.
  const autoEndedOversRef = useRef(false)
  useEffect(() => {
    if (!oversCompleteAwaitingEnd) {
      autoEndedOversRef.current = false
      return
    }
    if (autoEndedOversRef.current) return
    autoEndedOversRef.current = true
    offline.recordEvent({ kind: 'InningsEnd', reason: 'overs_complete' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [oversCompleteAwaitingEnd])

  // Concludes the match: flips it to `completed`, sending the score built
  // from this device's own view of the live detail (per-innings totals plus
  // batting/bowling detail). The server independently derives the same
  // score from the match's persisted live detail — the authoritative source,
  // and the only one with the full log to fold — and 409s if the two
  // disagree (e.g. a delivery landed between this device's last poll and the
  // tap). That's surfaced, not silently resolved: the live detail is
  // refetched (so the summary above updates to what's actually current) and
  // the button stays "Finish match" so the user reviews it and taps again
  // themselves, rather than the app resubmitting on their behalf without
  // them seeing what changed. Still enters the normal confirmation flow
  // rather than being auto-confirmed, same as a manual "Add result" would.
  const finishMatch = useMutation({
    mutationFn: async () => {
      if (!state) throw new Error('No score recorded yet')
      // The server independently re-derives the score from the match's
      // *persisted* live detail and 409s on any mismatch — while anything
      // is still queued locally-only, `matchScoreQueryKey`'s cached score
      // reflects deliveries the server has never seen, so this would 409
      // spuriously and, worse, its handler would refetch and overwrite the
      // local fold with the stale server score. Blocked in the UI (the
      // button is hidden/disabled while `offline.pendingCount > 0`) —
      // this is a defensive backstop, not the primary guard.
      if (offline.pendingCount > 0) {
        throw new Error('Sync the pending events before finishing the match')
      }

      const fresh = cricketScoreFrom(
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
    <div className="flex items-center justify-between gap-2">
      <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${match.id}`)}>
        <ChevronLeft className="size-4" /> Back
      </Button>
      <div className="flex items-center gap-2">
        <OfflineIndicator
          isOffline={offline.isOffline}
          pendingCount={offline.pendingCount}
          syncing={offline.syncing}
          conflict={offline.conflict}
          onSyncNow={offline.syncNow}
        />
        <LiveIndicator />
      </div>
    </div>
  )

  const conflictBanner = offline.conflict && (
    <ConflictBanner
      pendingCount={offline.pendingCount}
      onDiscardAndReload={offline.discardAndReloadAfterConflict}
    />
  )

  // No innings open — either the match hasn't started, or we're between
  // innings. Either way: pick who's batting (the other side bowls).
  if (!innings) {
    // Every innings the format allows has been played (e.g. both sides have
    // had their one T20 innings) — there's no "next innings" to start, so
    // don't offer to start one. Just let the scorer finish the match.
    const inningsQuota = match.sides.length * format.innings_per_side
    const allInningsComplete = !!state && state.innings.length >= inningsQuota

    return (
      <div className="mx-auto flex max-w-xl flex-col gap-4">
        {header}
        {conflictBanner}
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
                    {match.sides.find((s) => s.id === inn.batting_side_id)?.name?.trim() ||
                      'This side'}
                  </p>
                  <p className="text-2xl font-medium tracking-tight">
                    {inn.runs}/{inn.wickets}
                    <span className="ml-2 text-sm font-normal text-muted-foreground">
                      ({formatOvers(inn.overs)} ov)
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
              disabled={!startBattingSide || recordLocked}
              onClick={() => {
                const bowlingSideId = match.sides.find((s) => s.id !== startBattingSide)?.id
                if (!startBattingSide || !bowlingSideId) return
                record({
                  kind: 'InningsStart',
                  batting_side_id: startBattingSide,
                  bowling_side_id: bowlingSideId,
                })
                setStartBattingSide(undefined)
              }}
            >
              Start innings
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
            {offline.pendingCount > 0 ? (
              <p className="text-center text-xs text-muted-foreground">
                Sync {offline.pendingCount} pending event{offline.pendingCount === 1 ? '' : 's'} before
                finishing.
              </p>
            ) : (
              <Button
                variant={allInningsComplete ? 'default' : 'outline'}
                size="lg"
                disabled={finishMatch.isPending}
                onClick={() => finishMatch.mutate()}
              >
                {finishMatch.isPending ? 'Finishing…' : 'Finish match'}
              </Button>
            )}
          </>
        )}
      </div>
    )
  }

  const battingSide = match.sides.find((s) => s.id === innings.batting_side_id)
  const bowlingSide = match.sides.find((s) => s.id === innings.bowling_side_id)
  const effectiveStriker = next!.striker_player_id ?? pickedStriker
  const effectiveNonStriker = next!.non_striker_player_id ?? pickedNonStriker
  const effectiveBowler = next!.bowler_player_id ?? pickedBowler
  const readyToScore = !!effectiveStriker && !!effectiveNonStriker && !!effectiveBowler
  const showPickerPanel = !readyToScore || editingOpeningPicks

  const buildDelivery = (overrides: Partial<CricketDelivery>): CricketDelivery => ({
    over: next!.over,
    ball: next!.ball,
    bowler_player_id: effectiveBowler!,
    striker_player_id: effectiveStriker!,
    non_striker_player_id: effectiveNonStriker!,
    runs_off_bat: 0,
    ...overrides,
  })

  const recordDelivery = (overrides: Partial<CricketDelivery>) => {
    record({ kind: 'Delivery', ...buildDelivery(overrides) })
  }

  const endInnings = (reason: InningsEndReason) => {
    record({ kind: 'InningsEnd', reason })
    setEndInningsOpen(false)
  }

  if (oversCompleteAwaitingEnd) {
    // Transitional only — `recordEvent` folds the auto-fired `InningsEnd`
    // locally and near-instantly (see the auto-end effect above), so
    // `innings` normally goes `null` (moving to the "no innings open"
    // branch) within the same tick or two, regardless of connectivity.
    return (
      <div className="mx-auto flex max-w-xl flex-col gap-4">
        {header}
        {conflictBanner}
        <div>
          <h1 className="text-lg font-semibold">
            {sideName(battingSide!, 'Side A')} vs {sideName(bowlingSide!, 'Side B')}
          </h1>
          <p className="text-sm text-muted-foreground">Overs complete</p>
        </div>
        <div className="rounded-xl border bg-card p-4">
          <p className="text-sm font-medium">{sideName(battingSide!, 'Side A')} batting</p>
          <p className="mt-0.5 text-3xl font-medium tracking-tight">
            {innings.runs}/{innings.wickets}
            <span className="ml-2 text-sm font-normal text-muted-foreground">
              ({formatOvers(innings.overs)}/{format.overs_per_innings} ov)
            </span>
          </p>
        </div>
        <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
          <p className="text-sm font-medium text-muted-foreground">
            All {format.overs_per_innings} overs bowled — ending the innings…
          </p>
        </div>
      </div>
    )
  }

  const crr = runRate(innings.runs, innings.overs, format.balls_per_over)
  const overBalls = currentOverDeliveries(state!.recent_deliveries ?? [])
  // Excluded from the batter picker in addition to whoever's currently at
  // the crease — `innings.batting` carries the full card while live-scored,
  // so a dismissal here is as reliable as the server's own scorecard, not
  // just a locally-replayed guess. Retired hurt is excluded from this list
  // on purpose: unlike every other dismissal it isn't final — the same
  // batter can simply be picked again to resume (see `CricketRetireEvent`'s
  // doc comment on the backend).
  const outBatters = (innings.batting ?? [])
    .filter((b) => b.dismissal && b.dismissal.kind !== 'retired_hurt')
    .map((b) => b.player_id)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      {header}
      {conflictBanner}

      <div>
        <h1 className="text-lg font-semibold">
          {sideName(battingSide!, 'Side A')} vs {sideName(bowlingSide!, 'Side B')}
        </h1>
        <p className="text-sm text-muted-foreground">You're scoring this match</p>
      </div>

      <div className="rounded-xl border bg-card p-4">
        <p className="text-sm font-medium">{sideName(battingSide!, 'Side A')} batting</p>
        <p className="mt-0.5 text-3xl font-medium tracking-tight">
          {innings.runs}/{innings.wickets}
          <span className="ml-2 text-sm font-normal text-muted-foreground">
            ({formatOvers(innings.overs)}
            {format.overs_per_innings ? `/${format.overs_per_innings}` : ''} ov · CRR{' '}
            {crr.toFixed(2)})
          </span>
        </p>

        <div className="mt-3 space-y-1 border-t pt-3 text-sm">
          {effectiveStriker && (
            <div className="flex items-center justify-between">
              <span className="font-medium">
                {playerNameFor(match, effectiveStriker, state?.players) ?? '—'}*
              </span>
            </div>
          )}
          {effectiveNonStriker && (
            <div className="flex items-center justify-between">
              <span>{playerNameFor(match, effectiveNonStriker, state?.players) ?? '—'}</span>
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
      </div>

      {overBalls.length > 0 && (
        <div>
          <div className="mb-2 flex items-center justify-between">
            <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              This over
            </p>
            {offline.canUndo && (
              <Button
                variant="ghost"
                size="sm"
                className="h-auto px-0 text-xs text-muted-foreground hover:bg-transparent hover:underline"
                onClick={offline.undo}
              >
                Undo last ball
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            {overBalls.map((d, i) => (
              <span
                key={i}
                className={`flex size-9 items-center justify-center rounded-full text-sm font-semibold ${
                  isChipHighlighted(d) === 'boundary'
                    ? 'bg-primary/15 text-primary'
                    : isChipHighlighted(d) === 'wicket'
                      ? 'bg-destructive/15 text-destructive'
                      : 'bg-muted text-foreground'
                }`}
              >
                {deliveryChipLabel(d)}
              </span>
            ))}
          </div>
        </div>
      )}

      {showPickerPanel ? (
        <div className="rounded-xl border bg-card p-4">
          <p className="mb-3 text-sm font-medium">
            {editingOpeningPicks && readyToScore
              ? 'Edit your selections'
              : !effectiveBowler && (!effectiveStriker || !effectiveNonStriker)
                ? 'New over, new batter — who is it?'
                : !effectiveBowler
                  ? "New over — who's bowling?"
                  : 'Wicket! Who is coming in to bat?'}
          </p>
          {(!effectiveStriker || editingOpeningPicks) && (
            <div className="mb-3">
              <p className="mb-1.5 text-xs text-muted-foreground">Striker</p>
              <PlayerPicker
                players={playersOnSide(match.players, battingSide!)}
                value={pickedStriker ?? undefined}
                exclude={[effectiveNonStriker, ...outBatters]}
                onChange={setPickedStriker}
              />
            </div>
          )}
          {(!effectiveNonStriker || editingOpeningPicks) && (
            <div className="mb-3">
              <p className="mb-1.5 text-xs text-muted-foreground">Non-striker</p>
              <PlayerPicker
                players={playersOnSide(match.players, battingSide!)}
                value={pickedNonStriker ?? undefined}
                exclude={[effectiveStriker, ...outBatters]}
                onChange={setPickedNonStriker}
              />
            </div>
          )}
          {(!effectiveBowler || editingOpeningPicks) && (
            <div>
              <p className="mb-1.5 text-xs text-muted-foreground">Bowler</p>
              <PlayerPicker
                players={playersOnSide(match.players, bowlingSide!)}
                value={pickedBowler ?? undefined}
                exclude={next!.previous_over_bowler_player_id ?? undefined}
                onChange={setPickedBowler}
              />
            </div>
          )}
          {editingOpeningPicks && readyToScore && (
            <Button
              size="sm"
              className="mt-3"
              onClick={() => setEditingOpeningPicks(false)}
            >
              Done
            </Button>
          )}
        </div>
      ) : (
        <div>
          {openingPicksUnconfirmed && (
            <Button
              variant="ghost"
              size="sm"
              className="mb-2 h-auto px-0 text-xs text-muted-foreground hover:bg-transparent hover:underline"
              onClick={() => setEditingOpeningPicks(true)}
            >
              Edit selections
            </Button>
          )}
          <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Runs off this ball
          </p>
          <div className="grid grid-cols-4 gap-2">
            {[0, 1, 2, 3].map((n) => (
              <button
                key={n}
                type="button"
                disabled={recordLocked}
                onClick={() => recordDelivery({ runs_off_bat: n })}
                className="rounded-xl border bg-card p-4 text-lg font-semibold transition-colors hover:bg-muted disabled:opacity-50"
              >
                {n}
              </button>
            ))}
            {[4, 6].map((n) => (
              <button
                key={n}
                type="button"
                disabled={recordLocked}
                onClick={() => recordDelivery({ runs_off_bat: n })}
                className="rounded-xl border border-primary/30 bg-primary/10 p-4 text-lg font-semibold text-primary transition-colors hover:bg-primary/15 disabled:opacity-50"
              >
                {n}
              </button>
            ))}
            <button
              type="button"
              disabled={recordLocked}
              onClick={() => setExtraDialog('bye')}
              className="rounded-xl border bg-card p-4 text-sm font-semibold transition-colors hover:bg-muted disabled:opacity-50"
            >
              Bye
            </button>
          </div>

          <div className="mt-2 grid grid-cols-3 gap-2">
            <button
              type="button"
              disabled={recordLocked}
              onClick={() =>
                recordDelivery({ extra: { kind: 'wide', runs: format.wide_penalty_runs } })
              }
              className="rounded-xl border bg-card p-3 text-sm font-medium transition-colors hover:bg-muted disabled:opacity-50"
            >
              Wide
            </button>
            <button
              type="button"
              disabled={recordLocked}
              onClick={() => setExtraDialog('no_ball')}
              className="rounded-xl border bg-card p-3 text-sm font-medium transition-colors hover:bg-muted disabled:opacity-50"
            >
              No ball
            </button>
            <button
              type="button"
              disabled={recordLocked}
              onClick={() => setWicketOpen(true)}
              className="rounded-xl border border-destructive/30 bg-destructive/10 p-3 text-sm font-medium text-destructive transition-colors hover:bg-destructive/15 disabled:opacity-50"
            >
              Wicket
            </button>
          </div>
        </div>
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
                disabled={recordLocked}
                onClick={() => endInnings(r.value)}
              >
                {r.label}
              </Button>
            ))}
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="mt-2"
            onClick={() => setEndInningsOpen(false)}
          >
            Cancel
          </Button>
        </div>
      ) : (
        <Button variant="outline" onClick={() => setEndInningsOpen(true)}>
          End innings
        </Button>
      )}

      {readyToScore && (
        <>
          <WicketDialog
            open={wicketOpen}
            match={match}
            battingSideId={innings.batting_side_id}
            strikerPlayerId={effectiveStriker!}
            nonStrikerPlayerId={effectiveNonStriker!}
            submitting={recordLocked}
            onOpenChange={setWicketOpen}
            onSubmit={(wicket: CricketDeliveryWicket, runsBeforeDismissal) => {
              recordDelivery({ runs_off_bat: runsBeforeDismissal, wicket })
              setWicketOpen(false)
            }}
          />

          <NoBallDialog
            open={extraDialog === 'no_ball'}
            penaltyRuns={format.no_ball_penalty_runs}
            submitting={recordLocked}
            onOpenChange={(open) => !open && setExtraDialog(null)}
            onPick={(runsOffBat) => {
              recordDelivery({
                runs_off_bat: runsOffBat,
                extra: { kind: 'no_ball', runs: format.no_ball_penalty_runs },
              })
              setExtraDialog(null)
            }}
          />
          <ExtraRunsDialog
            open={extraDialog === 'bye'}
            title="Byes"
            kinds={[
              { value: 'bye', label: 'Bye' },
              { value: 'leg_bye', label: 'Leg bye' },
            ]}
            submitting={recordLocked}
            onOpenChange={(open) => !open && setExtraDialog(null)}
            onPick={(kind, runs) => {
              recordDelivery({ extra: { kind, runs } })
              setExtraDialog(null)
            }}
          />
        </>
      )}
    </div>
  )
}
