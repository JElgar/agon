import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { useAppendCricketEvent, useLiveScore } from '@/hooks/useLiveScore'
import { LiveIndicator } from '@/components/agon/live/LiveIndicator'
import { SidePicker, PlayerPicker, sideName } from '@/components/agon/live/Pickers'
import { WicketDialog } from '@/components/agon/live/WicketDialog'
import { ExtraRunsDialog } from '@/components/agon/live/ExtraRunsDialog'
import { playersOnSide } from '@/lib/members'
import {
  battingEntryFor,
  battingLine,
  bowlingEntryFor,
  bowlingFigures,
  cricketLiveState,
  currentInnings,
  currentOverDeliveries,
  deliveryChipLabel,
  isChipHighlighted,
  nextBallContext,
  outBattersFor,
  playerNameFor,
  runRate,
} from '@/lib/cricketScore'

type Match = components['schemas']['Match']
type CricketDelivery = components['schemas']['CricketDelivery']
type CricketDeliveryWicket = components['schemas']['CricketDeliveryWicket']
type InningsEndReason = components['schemas']['InningsEndReason']

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

  const live = useLiveScore(match.id, { refetchInterval: 8000 })
  const appendEvent = useAppendCricketEvent(match.id)
  const state = cricketLiveState(live.data)
  const innings = state ? currentInnings(state) : null
  const next = innings ? nextBallContext(innings) : null

  // Locally-picked openers/replacement batter/next bowler — only used until
  // the server confirms them via an actual delivery, at which point
  // `nextBallContext` takes over as the source of truth.
  const [pickedStriker, setPickedStriker] = useState<string | null>(null)
  const [pickedNonStriker, setPickedNonStriker] = useState<string | null>(null)
  const [pickedBowler, setPickedBowler] = useState<string | null>(null)
  useEffect(() => {
    if (next?.strikerPlayerId) setPickedStriker(null)
    if (next?.nonStrikerPlayerId) setPickedNonStriker(null)
    if (next?.bowlerPlayerId) setPickedBowler(null)
  }, [next?.strikerPlayerId, next?.nonStrikerPlayerId, next?.bowlerPlayerId])

  const [wicketOpen, setWicketOpen] = useState(false)
  const [extraDialog, setExtraDialog] = useState<'wide' | 'no_ball' | 'bye' | null>(null)
  const [endInningsOpen, setEndInningsOpen] = useState(false)
  const [startBattingSide, setStartBattingSide] = useState<string | undefined>(undefined)

  if (live.isLoading) {
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
      <LiveIndicator />
    </div>
  )

  // No innings open — either the match hasn't started, or we're between
  // innings. Either way: pick who's batting (the other side bowls).
  if (!innings) {
    return (
      <div className="mx-auto flex max-w-xl flex-col gap-4">
        {header}
        <div>
          <h1 className="text-lg font-semibold">
            {match.sides[0]?.name?.trim() || 'Side A'} vs {match.sides[1]?.name?.trim() || 'Side B'}
          </h1>
          <p className="text-sm text-muted-foreground">
            {state && state.innings.length > 0 ? 'Start the next innings' : "You're scoring this match"}
          </p>
        </div>
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
            const bowlingSideId = match.sides.find((s) => s.id !== startBattingSide)?.id
            if (!startBattingSide || !bowlingSideId) return
            appendEvent.mutate(
              {
                kind: 'InningsStart',
                batting_side_id: startBattingSide,
                bowling_side_id: bowlingSideId,
              },
              { onSuccess: () => setStartBattingSide(undefined) },
            )
          }}
        >
          {appendEvent.isPending ? 'Starting…' : 'Start innings'}
        </Button>
      </div>
    )
  }

  const battingSide = match.sides.find((s) => s.id === innings.batting_side_id)
  const bowlingSide = match.sides.find((s) => s.id === innings.bowling_side_id)
  const effectiveStriker = next!.strikerPlayerId ?? pickedStriker
  const effectiveNonStriker = next!.nonStrikerPlayerId ?? pickedNonStriker
  const effectiveBowler = next!.bowlerPlayerId ?? pickedBowler
  const readyToScore = !!effectiveStriker && !!effectiveNonStriker && !!effectiveBowler

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
    appendEvent.mutate({ kind: 'Delivery', ...buildDelivery(overrides) })
  }

  const endInnings = (reason: InningsEndReason) => {
    appendEvent.mutate(
      { kind: 'InningsEnd', reason },
      { onSuccess: () => setEndInningsOpen(false) },
    )
  }

  const crr = runRate(innings.runs, innings.overs)
  const strikerEntry = battingEntryFor(innings, effectiveStriker)
  const nonStrikerEntry = battingEntryFor(innings, effectiveNonStriker)
  const bowlerEntry = bowlingEntryFor(innings, effectiveBowler)
  const overBalls = currentOverDeliveries(innings)
  const outBatters = outBattersFor(innings)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      {header}

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
            ({innings.overs.toFixed(1)} ov · CRR {crr.toFixed(2)})
          </span>
        </p>

        <div className="mt-3 space-y-1 border-t pt-3 text-sm">
          {effectiveStriker && (
            <div className="flex items-center justify-between">
              <span className="font-medium">{playerNameFor(match, effectiveStriker)}*</span>
              <span className="text-muted-foreground">{battingLine(strikerEntry)}</span>
            </div>
          )}
          {effectiveNonStriker && (
            <div className="flex items-center justify-between">
              <span>{playerNameFor(match, effectiveNonStriker)}</span>
              <span className="text-muted-foreground">{battingLine(nonStrikerEntry)}</span>
            </div>
          )}
          {effectiveBowler && (
            <div className="mt-1.5 flex items-center justify-between border-t pt-1.5 text-xs">
              <span className="text-muted-foreground">
                Bowling: <span className="font-medium text-foreground">{playerNameFor(match, effectiveBowler)}</span>
              </span>
              <span className="text-muted-foreground">{bowlingFigures(bowlerEntry)}</span>
            </div>
          )}
        </div>
      </div>

      {overBalls.length > 0 && (
        <div>
          <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            This over
          </p>
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

      {!readyToScore ? (
        <div className="rounded-xl border bg-card p-4">
          <p className="mb-3 text-sm font-medium">
            {!effectiveBowler && (!effectiveStriker || !effectiveNonStriker)
              ? 'New over, new batter — who is it?'
              : !effectiveBowler
                ? "New over — who's bowling?"
                : 'Wicket! Who is coming in to bat?'}
          </p>
          {!effectiveStriker && (
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
          {!effectiveNonStriker && (
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
          {!effectiveBowler && (
            <div>
              <p className="mb-1.5 text-xs text-muted-foreground">Bowler</p>
              <PlayerPicker
                players={playersOnSide(match.players, bowlingSide!)}
                value={pickedBowler ?? undefined}
                exclude={next!.previousOverBowlerPlayerId ?? undefined}
                onChange={setPickedBowler}
              />
            </div>
          )}
        </div>
      ) : (
        <div>
          <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            Runs off this ball
          </p>
          <div className="grid grid-cols-4 gap-2">
            {[0, 1, 2, 3].map((n) => (
              <button
                key={n}
                type="button"
                disabled={appendEvent.isPending}
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
                disabled={appendEvent.isPending}
                onClick={() => recordDelivery({ runs_off_bat: n })}
                className="rounded-xl border border-primary/30 bg-primary/10 p-4 text-lg font-semibold text-primary transition-colors hover:bg-primary/15 disabled:opacity-50"
              >
                {n}
              </button>
            ))}
            <button
              type="button"
              disabled={appendEvent.isPending}
              onClick={() => setExtraDialog('bye')}
              className="rounded-xl border bg-card p-4 text-sm font-semibold transition-colors hover:bg-muted disabled:opacity-50"
            >
              Bye
            </button>
          </div>

          <div className="mt-2 grid grid-cols-3 gap-2">
            <button
              type="button"
              disabled={appendEvent.isPending}
              onClick={() => setExtraDialog('wide')}
              className="rounded-xl border bg-card p-3 text-sm font-medium transition-colors hover:bg-muted disabled:opacity-50"
            >
              Wide
            </button>
            <button
              type="button"
              disabled={appendEvent.isPending}
              onClick={() => setExtraDialog('no_ball')}
              className="rounded-xl border bg-card p-3 text-sm font-medium transition-colors hover:bg-muted disabled:opacity-50"
            >
              No ball
            </button>
            <button
              type="button"
              disabled={appendEvent.isPending}
              onClick={() => setWicketOpen(true)}
              className="rounded-xl border border-destructive/30 bg-destructive/10 p-3 text-sm font-medium text-destructive transition-colors hover:bg-destructive/15 disabled:opacity-50"
            >
              Wicket
            </button>
          </div>
        </div>
      )}

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
            submitting={appendEvent.isPending}
            onOpenChange={setWicketOpen}
            onSubmit={(wicket: CricketDeliveryWicket, runsBeforeDismissal) => {
              recordDelivery({ runs_off_bat: runsBeforeDismissal, wicket })
              setWicketOpen(false)
            }}
          />

          <ExtraRunsDialog
            open={extraDialog === 'wide'}
            title="Wide"
            kinds={[{ value: 'wide', label: 'Wide' }]}
            submitting={appendEvent.isPending}
            onOpenChange={(open) => !open && setExtraDialog(null)}
            onPick={(kind, runs) => {
              recordDelivery({ extra: { kind, runs } })
              setExtraDialog(null)
            }}
          />
          <ExtraRunsDialog
            open={extraDialog === 'no_ball'}
            title="No ball"
            kinds={[{ value: 'no_ball', label: 'No ball' }]}
            submitting={appendEvent.isPending}
            onOpenChange={(open) => !open && setExtraDialog(null)}
            onPick={(kind, runs) => {
              recordDelivery({ extra: { kind, runs } })
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
            submitting={appendEvent.isPending}
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
