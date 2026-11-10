import { useMemo, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { ChevronLeft, Info, Minus, Pin, Plus, RotateCw, Shuffle } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { defaultSquadLabel } from '@/lib/sideColours'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '@/components/ui/sheet'
import { Avatar } from './Avatar'

type Preview = components['schemas']['AutoAssignPreview']
type PreviewSide = components['schemas']['AutoAssignSide']
type PreviewPlayer = components['schemas']['AutoAssignPlayer']
type Method = components['schemas']['AutoAssignMethod']
type Scope = components['schemas']['AutoAssignScope']
type Level = components['schemas']['PlayerLevel']
type SquadLabel = components['schemas']['SquadLabel']

/** Where the split lands: a teamless tournament's squads, or a match's
 *  existing sides. */
export type AutoAssignTarget =
  | {
      kind: 'tournament'
      tournamentId: string
      poolSize: number
      unassignedCount: number
      squadSize: number
      squadLabels: Record<string, SquadLabel>
      subtitle?: string
    }
  | {
      kind: 'match'
      matchId: string
      poolSize: number
      unassignedCount: number
      sides: { id: string; name: string; colour?: string }[]
      subtitle?: string
    }

export interface PoolDisplay {
  name: string
  imageUrl?: string
}

/** A seed that survives JSON and every generated client's int32. */
function newSeed() {
  return Math.floor(Math.random() * 2 ** 31)
}

function pct(x: number) {
  return Math.round(x * 100)
}

const CERTAINTY_TEXT = ['', 'Little history', 'Some history', 'Solid'] as const

/**
 * Auto-assign: pick Random or Auto Balanced, preview the split, re-roll or
 * pin as you like, then apply. Organiser-only — players never see that a
 * split was auto-built or anyone's strength estimate. Preview writes
 * nothing; apply sends back exactly the previewed assignment plus its
 * `roster_version`, and a 409 (someone joined or moved meanwhile) simply
 * re-previews.
 */
export function AutoAssignSheet({
  open,
  onOpenChange,
  target,
  displays,
  onApplied,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  target: AutoAssignTarget
  displays: Map<string, PoolDisplay>
  onApplied?: () => void
}) {
  const queryClient = useQueryClient()
  const defaultSquadCount =
    target.kind === 'tournament' ? Math.max(2, Math.ceil(target.poolSize / Math.max(1, target.squadSize))) : 0

  const [step, setStep] = useState<'options' | 'preview'>('options')
  const [method, setMethod] = useState<Method>('auto_balanced')
  const [scope, setScope] = useState<Scope>(
    target.unassignedCount > 0 && target.unassignedCount < target.poolSize ? 'unassigned' : 'everyone',
  )
  const [squadCount, setSquadCount] = useState(defaultSquadCount)
  // Explicit organiser pins (id -> squad index or side id) and level overrides.
  const [pins, setPins] = useState<Map<string, number | string>>(new Map())
  const [levels, setLevels] = useState<Map<string, Level>>(new Map())
  const [preview, setPreview] = useState<Preview | null>(null)
  const [detailId, setDetailId] = useState<string | null>(null)
  const [howOpen, setHowOpen] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)

  const reset = () => {
    setStep('options')
    setPreview(null)
    setPins(new Map())
    setLevels(new Map())
    setNotice(null)
  }

  const previewMutation = useMutation({
    mutationFn: async (args: { seed: number; pins: Map<string, number | string>; levels: Map<string, Level> }) => {
      const levelList = [...args.levels].map(([id, level]) => ({ id, level }))
      if (target.kind === 'tournament') {
        const { data, error } = await fetchClient.POST('/tournaments/{tournament_id}/squads/auto-assign', {
          params: { path: { tournament_id: target.tournamentId } },
          body: {
            method,
            scope,
            squad_count: squadCount,
            pins: [...args.pins].map(([id, squad]) => ({ id, squad: Number(squad) })),
            levels: levelList,
            seed: args.seed,
          },
        })
        if (error || !data) throw new Error(typeof error === 'string' ? error : 'Could not preview squads')
        return data
      }
      const { data, error } = await fetchClient.POST('/matches/{match_id}/sides/auto-assign', {
        params: { path: { match_id: target.matchId } },
        body: {
          method,
          scope,
          pins: [...args.pins].map(([id, side_id]) => ({ id, side_id: String(side_id) })),
          levels: levelList,
          seed: args.seed,
        },
      })
      if (error || !data) throw new Error(typeof error === 'string' ? error : 'Could not preview sides')
      return data
    },
    onSuccess: (data) => {
      setPreview(data)
      setStep('preview')
    },
  })

  const runPreview = (opts?: { seed?: number; pins?: Map<string, number | string>; levels?: Map<string, Level> }) =>
    previewMutation.mutate({
      seed: opts?.seed ?? preview?.seed ?? newSeed(),
      pins: opts?.pins ?? pins,
      levels: opts?.levels ?? levels,
    })

  const applyMutation = useMutation({
    mutationFn: async (p: Preview): Promise<'applied' | 'stale'> => {
      if (target.kind === 'tournament') {
        const assignments = p.sides.flatMap((s) => s.players.map((pl) => ({ id: pl.id, squad: s.squad ?? 0 })))
        const { error, response } = await fetchClient.PUT('/tournaments/{tournament_id}/squads/assignments', {
          params: { path: { tournament_id: target.tournamentId } },
          body: { roster_version: p.roster_version, assignments },
        })
        if (response.status === 409) return 'stale'
        if (error) throw new Error('Could not apply these squads')
        // The server has no default squad label: save the one the preview
        // showed for any squad nobody has named yet.
        for (const side of p.sides) {
          const squad = side.squad ?? 0
          if (!target.squadLabels[String(squad)] && side.players.length > 0) {
            const { error: labelError } = await fetchClient.PUT('/tournaments/{tournament_id}/squads/{squad}', {
              params: { path: { tournament_id: target.tournamentId, squad } },
              body: defaultSquadLabel(squad),
            })
            if (labelError) throw new Error('Could not name a squad')
          }
        }
        return 'applied'
      }
      const assignments = p.sides.flatMap((s) => s.players.map((pl) => ({ id: pl.id, side_id: s.side_id ?? '' })))
      const { error, response } = await fetchClient.PUT('/matches/{match_id}/sides/assignments', {
        params: { path: { match_id: target.matchId } },
        body: { roster_version: p.roster_version, assignments },
      })
      if (response.status === 409) return 'stale'
      if (error) throw new Error('Could not apply these sides')
      return 'applied'
    },
    onSuccess: (result) => {
      if (result === 'stale') {
        setNotice('Someone joined or moved since this preview, so here’s a fresh one.')
        runPreview({ seed: newSeed() })
        return
      }
      if (target.kind === 'tournament') {
        queryClient.invalidateQueries({ queryKey: ['tournament', target.tournamentId] })
      } else {
        queryClient.invalidateQueries({ queryKey: ['match', target.matchId] })
        queryClient.invalidateQueries({ queryKey: ['feed'] })
      }
      onOpenChange(false)
      reset()
      onApplied?.()
    },
  })

  const sideMeta = (side: PreviewSide, index: number) => {
    if (target.kind === 'tournament') {
      const squad = side.squad ?? index
      const label = target.squadLabels[String(squad)] ?? defaultSquadLabel(squad)
      return { name: label.name, colour: label.colour }
    }
    const s = target.sides.find((x) => x.id === side.side_id)
    return { name: s?.name ?? 'Side', colour: s?.colour }
  }

  const balanced = preview?.method === 'auto_balanced'
  const togglePin = (player: PreviewPlayer, side: PreviewSide) => {
    const next = new Map(pins)
    if (next.has(player.id)) next.delete(player.id)
    else next.set(player.id, target.kind === 'tournament' ? (side.squad ?? 0) : (side.side_id ?? ''))
    setPins(next)
    setPreview((prev) =>
      prev && {
        ...prev,
        sides: prev.sides.map((s) => ({
          ...s,
          players: s.players.map((p) => (p.id === player.id ? { ...p, pinned: next.has(p.id) } : p)),
        })),
      },
    )
  }
  const setLevel = (id: string, level: Level | null) => {
    const next = new Map(levels)
    if (level) next.set(id, level)
    else next.delete(id)
    setLevels(next)
    runPreview({ levels: next })
  }

  const detail = useMemo(() => {
    if (!preview || !detailId) return null
    for (const [i, side] of preview.sides.entries()) {
      const player = side.players.find((p) => p.id === detailId)
      if (player) return { player, sideName: sideMeta(side, i).name }
    }
    return null
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preview, detailId])

  const unit = target.kind === 'tournament' ? 'squad' : 'side'

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={(o) => {
          onOpenChange(o)
          if (!o) reset()
        }}
      >
        <SheetContent
          side="bottom"
          className="mx-auto max-h-[92vh] w-full max-w-xl overflow-y-auto rounded-t-[20px] bg-background p-4"
        >
          {step === 'options' ? (
            <OptionsStep
              target={target}
              method={method}
              onMethod={setMethod}
              scope={scope}
              onScope={setScope}
              squadCount={squadCount}
              defaultSquadCount={defaultSquadCount}
              onSquadCount={setSquadCount}
              busy={previewMutation.isPending}
              error={previewMutation.isError ? previewMutation.error.message : null}
              onCancel={() => onOpenChange(false)}
              onPreview={() => runPreview({ seed: newSeed() })}
            />
          ) : (
            preview && (
              <div className="flex flex-col gap-3">
                <SheetHeader className="flex-row items-center gap-1 space-y-0 p-0 pr-8 text-left">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="-ml-2"
                    aria-label="Back to options"
                    onClick={() => setStep('options')}
                  >
                    <ChevronLeft className="size-5" />
                  </Button>
                  <SheetTitle className="flex-1 text-left font-display text-xl font-extrabold">Preview</SheetTitle>
                  <span className="rounded-full bg-accent px-2.5 py-1 text-xs font-semibold text-accent-foreground">
                    {balanced ? 'Auto Balanced' : 'Random'} · {preview.sides.length} {unit}s
                  </span>
                </SheetHeader>
                <SheetDescription className="sr-only">Preview of the {unit} split before applying it</SheetDescription>

                {notice && (
                  <p className="rounded-lg border border-amber-300 bg-amber-50 p-2.5 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200">
                    {notice}
                  </p>
                )}

                {balanced && (
                  <BalanceCard
                    preview={preview}
                    sideMeta={sideMeta}
                    unit={unit}
                    onHow={() => setHowOpen(true)}
                  />
                )}

                <p className="px-1 text-xs text-muted-foreground">
                  {balanced && 'Tap a player to see how their strength was estimated. '}Pin a player to keep them
                  where they are when you re-roll.
                </p>

                {preview.sides.map((side, i) => {
                  const meta = sideMeta(side, i)
                  return (
                    <div key={side.side_id ?? side.squad ?? i} className="rounded-[20px] border bg-card px-3 py-1.5">
                      <div className="flex items-center gap-2 py-2">
                        <span
                          className="size-3 shrink-0 rounded-full border border-black/10"
                          style={{ background: meta.colour }}
                          aria-hidden
                        />
                        <span className="flex-1 truncate text-[15px] font-bold">{meta.name}</span>
                        {balanced && side.average != null && (
                          <span className="text-xs text-muted-foreground">
                            avg ≈ <strong className="text-foreground">{side.average}</strong>
                          </span>
                        )}
                        {balanced && side.unsure_count > 0 && (
                          <span className="rounded-full bg-amber-100 px-2 py-0.5 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200">
                            {side.unsure_count} unsure
                          </span>
                        )}
                      </div>
                      {side.players.map((p) => (
                        <PlayerRow
                          key={p.id}
                          player={p}
                          display={displays.get(p.id)}
                          balanced={balanced}
                          showPlaced={scope === 'unassigned'}
                          pinnedByScope={p.pinned && !pins.has(p.id)}
                          onOpen={() => setDetailId(p.id)}
                          onPin={() => togglePin(p, side)}
                        />
                      ))}
                    </div>
                  )
                })}

                {(previewMutation.isError || applyMutation.isError) && (
                  <p className="text-sm text-destructive">
                    {(applyMutation.error ?? previewMutation.error)?.message}
                  </p>
                )}

                <div className="sticky -bottom-4 -mx-4 -mb-4 flex gap-2 border-t bg-card px-4 pt-3 pb-5">
                  <Button
                    variant="outline"
                    className="h-12"
                    disabled={previewMutation.isPending || applyMutation.isPending}
                    onClick={() => {
                      setNotice(null)
                      runPreview({ seed: newSeed() })
                    }}
                  >
                    <RotateCw className={cn('size-4', previewMutation.isPending && 'animate-spin')} />
                    Re-roll
                  </Button>
                  <Button
                    className="h-12 flex-1"
                    disabled={previewMutation.isPending || applyMutation.isPending}
                    onClick={() => applyMutation.mutate(preview)}
                  >
                    {applyMutation.isPending
                      ? 'Applying…'
                      : target.kind === 'tournament'
                        ? 'Apply to entries'
                        : 'Apply to match'}
                  </Button>
                </div>
              </div>
            )
          )}
        </SheetContent>
      </Sheet>

      <StrengthDialog
        detail={detail}
        name={detail ? (displays.get(detail.player.id)?.name ?? 'Player') : ''}
        imageUrl={detail ? displays.get(detail.player.id)?.imageUrl : undefined}
        onClose={() => setDetailId(null)}
        onLevel={(level) => detail && setLevel(detail.player.id, level)}
        busy={previewMutation.isPending}
      />
      <HowItWorksDialog open={howOpen} onOpenChange={setHowOpen} />
    </>
  )
}

function OptionsStep({
  target,
  method,
  onMethod,
  scope,
  onScope,
  squadCount,
  defaultSquadCount,
  onSquadCount,
  busy,
  error,
  onCancel,
  onPreview,
}: {
  target: AutoAssignTarget
  method: Method
  onMethod: (m: Method) => void
  scope: Scope
  onScope: (s: Scope) => void
  squadCount: number
  defaultSquadCount: number
  onSquadCount: (n: number) => void
  busy: boolean
  error: string | null
  onCancel: () => void
  onPreview: () => void
}) {
  const unit = target.kind === 'tournament' ? 'squads' : 'sides'
  const lo = Math.floor(target.poolSize / Math.max(1, squadCount))
  const hi = Math.ceil(target.poolSize / Math.max(1, squadCount))
  const methods: { id: Method; label: string; desc: string }[] = [
    { id: 'random', label: 'Random', desc: 'Shuffle and deal. Re-roll as often as you like.' },
    { id: 'auto_balanced', label: 'Auto Balanced', desc: 'Agon evens out squads from rating and win rate.' },
  ]
  const scopes: { id: Scope; label: string; desc: string }[] = [
    {
      id: 'unassigned',
      label: `Only unassigned (${target.unassignedCount})`,
      desc: `Anyone already in a ${unit.slice(0, -1)} stays put.`,
    },
    { id: 'everyone', label: 'Reshuffle everyone', desc: 'Start over. Pin players in the preview to keep them.' },
  ]

  return (
    <div className="flex flex-col gap-4">
      <SheetHeader className="space-y-1 p-0 pr-8 text-left">
        <SheetTitle className="font-display text-xl font-extrabold">Auto-assign {unit}</SheetTitle>
        <SheetDescription>
          {target.poolSize} players{target.subtitle ? ` · ${target.subtitle}` : ''}
        </SheetDescription>
      </SheetHeader>

      <section className="flex flex-col gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Method</span>
        <div className="grid grid-cols-2 gap-2">
          {methods.map((m) => (
            <button
              key={m.id}
              type="button"
              aria-pressed={method === m.id}
              onClick={() => onMethod(m.id)}
              className={cn(
                'flex min-h-[88px] flex-col items-start gap-1 rounded-[14px] border p-3 text-left transition-colors',
                method === m.id ? 'border-2 border-primary bg-accent/60' : 'm-px border-input bg-card',
              )}
            >
              <span className="text-[15px] font-bold">{m.label}</span>
              <span className="text-xs leading-snug text-muted-foreground">{m.desc}</span>
            </button>
          ))}
        </div>
        {method === 'auto_balanced' && (
          <p className="text-[13px] leading-snug text-muted-foreground">
            Uses ranked rating once a player has 10+ ranked games. Below that it leans on win rate, and players with
            no history count as this group’s average. Tap any player in the preview to see how they were rated.
          </p>
        )}
      </section>

      {target.kind === 'tournament' && (
        <section className="flex flex-col gap-2">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Squads</span>
          <div className="flex items-center gap-3">
            <Button
              variant="outline"
              size="icon"
              className="size-11"
              aria-label="Fewer squads"
              disabled={squadCount <= 2}
              onClick={() => onSquadCount(squadCount - 1)}
            >
              <Minus className="size-4" />
            </Button>
            <span className="min-w-7 text-center font-display text-2xl font-extrabold">{squadCount}</span>
            <Button
              variant="outline"
              size="icon"
              className="size-11"
              aria-label="More squads"
              disabled={squadCount >= target.poolSize}
              onClick={() => onSquadCount(squadCount + 1)}
            >
              <Plus className="size-4" />
            </Button>
            <div className="flex flex-1 flex-col">
              <span className="text-sm font-semibold">
                {lo === hi ? lo : `${lo}–${hi}`} per squad
              </span>
              {squadCount === defaultSquadCount ? (
                <span className="text-xs text-muted-foreground">Default · from squad size {target.squadSize}</span>
              ) : (
                <button
                  type="button"
                  className="self-start text-xs font-semibold text-primary"
                  onClick={() => onSquadCount(defaultSquadCount)}
                >
                  Reset to default ({defaultSquadCount})
                </button>
              )}
            </div>
          </div>
        </section>
      )}

      <section className="flex flex-col gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Who to place</span>
        {scopes.map((o) => (
          <button
            key={o.id}
            type="button"
            aria-pressed={scope === o.id}
            disabled={o.id === 'unassigned' && target.unassignedCount === 0}
            onClick={() => onScope(o.id)}
            className={cn(
              'flex items-center gap-3 rounded-[14px] border p-3 text-left disabled:opacity-50',
              scope === o.id ? 'border-2 border-primary bg-accent/60' : 'm-px border-input bg-card',
            )}
          >
            <span
              className={cn(
                'size-[18px] shrink-0 rounded-full',
                scope === o.id ? 'border-[6px] border-primary' : 'border-2 border-muted-foreground/50',
              )}
              aria-hidden
            />
            <span className="flex flex-col">
              <span className="text-sm font-semibold">{o.label}</span>
              <span className="text-xs text-muted-foreground">{o.desc}</span>
            </span>
          </button>
        ))}
      </section>

      {error && <p className="text-sm text-destructive">{error}</p>}

      <div className="flex gap-2 pt-1">
        <Button variant="outline" className="h-12" onClick={onCancel}>
          Cancel
        </Button>
        <Button className="h-12 flex-1" disabled={busy} onClick={onPreview}>
          <Shuffle className="size-4" />
          {busy ? 'Working…' : `Preview ${unit}`}
        </Button>
      </div>
    </div>
  )
}

function BalanceCard({
  preview,
  sideMeta,
  unit,
  onHow,
}: {
  preview: Preview
  sideMeta: (side: PreviewSide, index: number) => { name: string; colour?: string }
  unit: string
  onHow: () => void
}) {
  const verdictText = { even: 'Even', close: 'Close', uneven: 'Uneven' }[preview.verdict]
  const verdictClass = {
    even: 'text-success',
    close: 'text-amber-700 dark:text-amber-300',
    uneven: 'text-destructive',
  }[preview.verdict]
  const maxUnsure = Math.max(0, ...preview.sides.map((s) => s.unsure_count))
  const w = preview.win_probability

  return (
    <div className="flex flex-col gap-3 rounded-[20px] border bg-card p-4">
      {w && preview.sides.length === 2 ? (
        <>
          <div className="flex items-baseline justify-between">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Predicted</span>
            <span className={cn('text-sm font-semibold', verdictClass)}>{verdictText}</span>
          </div>
          <div className="flex items-baseline justify-between font-display text-[28px] font-extrabold">
            <span style={{ color: sideMeta(preview.sides[0], 0).colour }}>{pct(w.expected)}%</span>
            <span className="text-muted-foreground">{100 - pct(w.expected)}%</span>
          </div>
          <div className="flex h-3 overflow-hidden rounded-full border" aria-hidden>
            <span style={{ width: `${pct(w.expected)}%`, background: sideMeta(preview.sides[0], 0).colour }} />
            <span className="flex-1" style={{ background: sideMeta(preview.sides[1], 1).colour, opacity: 0.35 }} />
          </div>
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{sideMeta(preview.sides[0], 0).name}</span>
            <span>
              could be {pct(w.low)}–{pct(w.high)}%{preview.unsure_count > 0 && ` · ${preview.unsure_count} unsure`}
            </span>
            <span>{sideMeta(preview.sides[1], 1).name}</span>
          </div>
        </>
      ) : (
        <div className="flex items-baseline justify-between">
          <span className={cn('font-display text-2xl font-extrabold', verdictClass)}>{verdictText}</span>
          <span className="text-[13px] text-muted-foreground">
            {unit[0].toUpperCase() + unit.slice(1)} averages within {preview.spread} pts
          </span>
        </div>
      )}
      <p className="text-[13px] leading-snug text-muted-foreground">
        {preview.unsure_count > 0 ? (
          <>
            <strong className="text-foreground">
              {preview.unsure_count} {preview.unsure_count === 1 ? 'player has' : 'players have'} little history.
            </strong>{' '}
            {preview.unsure_count > 1
              ? `They're spread out so no ${unit} has more than ${maxUnsure}.`
              : 'Their estimate is the group average.'}
          </>
        ) : (
          'Everyone has enough history to go on.'
        )}
      </p>
      <button
        type="button"
        onClick={onHow}
        className="flex h-9 items-center gap-1.5 self-start text-[13px] font-semibold text-primary"
      >
        <Info className="size-4" />
        How does Auto Balanced work?
      </button>
    </div>
  )
}

function chipFor(p: PreviewPlayer): { main: string; sub: string; tone: 'rated' | 'blend' | 'wr' | 'unknown' } {
  const wr = p.win_rate ? `${pct(p.win_rate.raw)}% W` : null
  const levelSub = p.level === 'newer' ? 'set: newer' : p.level === 'stronger' ? 'set: stronger' : null
  switch (p.signal) {
    case 'rated':
      return { main: String(p.estimate ?? ''), sub: `${p.ranked_games ?? 0} ranked`, tone: 'rated' }
    case 'blended':
      return { main: `≈${p.estimate ?? ''}`, sub: `${p.ranked_games ?? 0} ranked${wr ? ` · ${wr}` : ''}`, tone: 'blend' }
    case 'placement':
      return { main: wr ?? 'Placing', sub: 'In placements', tone: 'blend' }
    case 'win_rate':
      return { main: wr ?? '', sub: levelSub ?? `${p.win_rate?.played ?? 0} played`, tone: 'wr' }
    case 'new':
      return { main: 'New', sub: levelSub ?? 'group avg', tone: 'unknown' }
    default:
      return { main: 'Guest', sub: levelSub ?? 'group avg', tone: 'unknown' }
  }
}

function CertaintyDots({ certainty, className }: { certainty: number; className?: string }) {
  return (
    <span className={cn('flex items-center gap-[3px]', className)} aria-hidden>
      {[1, 2, 3].map((i) => (
        <span key={i} className={cn('size-1.5 rounded-full', i <= certainty ? 'bg-primary' : 'bg-muted-foreground/25')} />
      ))}
    </span>
  )
}

function PlayerRow({
  player,
  display,
  balanced,
  showPlaced,
  pinnedByScope,
  onOpen,
  onPin,
}: {
  player: PreviewPlayer
  display?: PoolDisplay
  balanced: boolean
  showPlaced: boolean
  pinnedByScope: boolean
  onOpen: () => void
  onPin: () => void
}) {
  const name = display?.name ?? 'Player'
  const chip = chipFor(player)
  const toneClass = {
    rated: 'bg-accent text-accent-foreground',
    blend: 'border border-primary/20 bg-accent/50 text-accent-foreground',
    wr: 'bg-muted text-foreground',
    unknown: 'border-[1.5px] border-dashed border-muted-foreground/40 text-foreground',
  }[chip.tone]

  return (
    <div className="flex min-h-[52px] items-center gap-2.5 border-t py-1">
      <Avatar name={name} imageUrl={display?.imageUrl} size="lg" />
      <button
        type="button"
        onClick={onOpen}
        disabled={!balanced}
        className="flex min-w-0 flex-1 flex-col items-start text-left disabled:cursor-default"
      >
        <span className="flex max-w-full items-center gap-1.5">
          <span className="truncate text-sm font-semibold">{name}</span>
          {showPlaced && player.moved && (
            <span className="shrink-0 rounded-md bg-success/15 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-success">
              Placed
            </span>
          )}
        </span>
        {balanced && (
          <span className="flex items-center gap-1" aria-label={`Certainty: ${CERTAINTY_TEXT[player.certainty]}`}>
            <CertaintyDots certainty={player.certainty} />
            <span className="ml-0.5 text-[11px] text-muted-foreground">
              {CERTAINTY_TEXT[player.certainty].toLowerCase()}
            </span>
          </span>
        )}
      </button>
      {balanced && (
        <span className={cn('flex min-w-[62px] flex-col items-end rounded-[10px] px-2 py-1', toneClass)}>
          <span className="text-[13px] font-bold">{chip.main}</span>
          <span className="text-[10px] text-muted-foreground">{chip.sub}</span>
        </span>
      )}
      <button
        type="button"
        onClick={onPin}
        disabled={pinnedByScope}
        aria-pressed={player.pinned}
        aria-label={
          pinnedByScope ? `${name} is already placed` : player.pinned ? `Unpin ${name}` : `Pin ${name} here`
        }
        className={cn(
          'flex h-11 w-9 shrink-0 items-center justify-center disabled:opacity-40',
          player.pinned ? 'text-primary' : 'text-muted-foreground',
        )}
      >
        <Pin className="size-4" fill={player.pinned ? 'currentColor' : 'none'} />
      </button>
    </div>
  )
}

function StrengthDialog({
  detail,
  name,
  imageUrl,
  onClose,
  onLevel,
  busy,
}: {
  detail: { player: PreviewPlayer; sideName: string } | null
  name: string
  imageUrl?: string
  onClose: () => void
  onLevel: (level: Level | null) => void
  busy: boolean
}) {
  const p = detail?.player
  const weight = p ? Math.round(p.rating_weight * 100) : 0
  const level = p?.level ?? null
  const levels: { id: Level | null; label: string }[] = [
    { id: 'newer', label: 'Newer' },
    { id: null, label: 'As expected' },
    { id: 'stronger', label: 'Stronger' },
  ]
  const levelNote = {
    newer: 'Counted as below this group’s average for this draw only. Their rating is untouched.',
    stronger: 'Counted as above this group’s average for this draw only. Their rating is untouched.',
  }

  return (
    <Dialog open={!!detail} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-md rounded-[20px] p-5">
        {p && (
          <div className="flex flex-col gap-4">
            <DialogHeader className="flex-row items-center gap-3 space-y-0 text-left">
              <Avatar name={name} imageUrl={imageUrl} size="xl" className="size-12 text-base" />
              <div className="flex flex-col">
                <DialogTitle className="font-display text-xl font-extrabold">{name}</DialogTitle>
                <span className="text-[13px] text-muted-foreground">{detail.sideName}</span>
              </div>
            </DialogHeader>

            <div className="flex items-center justify-between rounded-2xl bg-muted/60 p-3.5">
              <div className="flex flex-col">
                <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  Estimated strength
                </span>
                <span
                  className={cn(
                    'font-display font-extrabold',
                    p.estimate != null ? 'text-[28px]' : 'text-xl leading-tight',
                  )}
                >
                  {p.estimate != null ? `≈ ${p.estimate}` : 'Still in placements'}
                </span>
              </div>
              <div className="flex flex-col items-end gap-1">
                <CertaintyDots certainty={p.certainty} />
                <span className="text-xs text-muted-foreground">{CERTAINTY_TEXT[p.certainty]}</span>
              </div>
            </div>

            {(p.rating_weight > 0 || p.win_rate) && (
              <div className="flex flex-col gap-2">
                <div className="flex justify-between text-[13px]">
                  <span className="font-semibold">Made from</span>
                  {p.ranked_games != null && p.ranked_games < 10 && p.ranked_games > 0 && (
                    <span className="text-muted-foreground">{p.ranked_games} of 10 ranked games</span>
                  )}
                </div>
                <div className="flex h-7 overflow-hidden rounded-lg text-xs font-semibold">
                  {weight > 0 && (
                    <span
                      className="flex items-center bg-primary pl-2 whitespace-nowrap text-primary-foreground"
                      style={{ width: `${weight}%` }}
                    >
                      Rating {weight}%
                    </span>
                  )}
                  {weight < 100 && (
                    <span className="flex flex-1 items-center justify-end bg-muted pr-2 whitespace-nowrap">
                      Win rate {100 - weight}%
                    </span>
                  )}
                </div>
              </div>
            )}

            <div className="grid grid-cols-2 gap-2">
              <div className="flex flex-col gap-1 rounded-[14px] border p-3">
                <span className="text-xs text-muted-foreground">Ranked rating</span>
                <span className="text-[17px] font-bold">
                  {p.signal === 'placement'
                    ? 'Still in placements'
                    : p.signal === 'rated'
                      ? p.estimate
                      : p.ranked_games
                        ? `${p.ranked_games} ranked`
                        : 'None yet'}
                </span>
              </div>
              <div className="flex flex-col gap-1 rounded-[14px] border p-3">
                <span className="text-xs text-muted-foreground">Win rate · all games</span>
                <span className="text-[17px] font-bold">
                  {p.win_rate ? (
                    <>
                      {pct(p.win_rate.raw)}%{' '}
                      <span className="text-[13px] font-medium text-muted-foreground">
                        → {pct(p.win_rate.adjusted)}%
                      </span>
                    </>
                  ) : (
                    'No games yet'
                  )}
                </span>
              </div>
            </div>

            <div className="flex flex-col gap-2">
              <span className="text-[13px] font-semibold">Know better? Adjust for this draw</span>
              <div className="grid grid-cols-3 gap-1 rounded-xl bg-muted p-1">
                {levels.map((l) => (
                  <button
                    key={l.label}
                    type="button"
                    disabled={busy}
                    aria-pressed={level === l.id}
                    onClick={() => onLevel(l.id)}
                    className={cn(
                      'h-10 rounded-[9px] text-[13px] font-semibold',
                      level === l.id ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground',
                    )}
                  >
                    {l.label}
                  </button>
                ))}
              </div>
              <span className="text-xs text-muted-foreground">
                {level ? levelNote[level] : 'Using what their stats say.'}
              </span>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

const HOW_STEPS = [
  { title: 'Ranked rating comes first', body: 'Once someone has 10 ranked games, their rating is all we use.' },
  {
    title: 'Win rate fills the gap',
    body: 'Before that, win rate from all games counts too, and less with each ranked game. Short records are pulled toward 50%.',
  },
  {
    title: 'No history? Group average',
    body: 'New players and guests count as this group’s average until you adjust them.',
  },
  {
    title: 'Even squads, unknowns spread out',
    body: 'Squads are matched on overall strength, and the players we know least about are split across squads.',
  },
]

function HowItWorksDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md rounded-[20px] p-5">
        <DialogHeader className="text-left">
          <DialogTitle className="font-display text-[19px] font-extrabold">How Auto Balanced works</DialogTitle>
        </DialogHeader>
        <ol className="flex flex-col gap-3.5">
          {HOW_STEPS.map((s, i) => (
            <li key={s.title} className="flex gap-3">
              <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-accent text-xs font-bold text-accent-foreground">
                {i + 1}
              </span>
              <div className="flex flex-col gap-0.5">
                <span className="text-sm font-semibold">{s.title}</span>
                <span className="text-[13px] leading-snug text-muted-foreground">{s.body}</span>
              </div>
            </li>
          ))}
        </ol>
        <Button variant="outline" className="h-11" onClick={() => onOpenChange(false)}>
          Got it
        </Button>
      </DialogContent>
    </Dialog>
  )
}
