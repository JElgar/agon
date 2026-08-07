import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SidePicker, PlayerPicker } from '@/components/agon/live/Pickers'
import { playersOnSide } from '@/lib/members'
import { dismissalLabel, type CricketDismissalKind } from '@/lib/cricketScore'

type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type CricketScoreInnings = components['schemas']['CricketScoreInnings']
type CricketBattingEntry = components['schemas']['CricketBattingEntry']
type CricketBowlingEntry = components['schemas']['CricketBowlingEntry']

const DISMISSAL_KINDS: CricketDismissalKind[] = [
  'bowled',
  'caught',
  'leg_before_wicket',
  'run_out',
  'stumped',
  'hit_wicket',
]

interface BattingRowDraft {
  playerId: string
  runs: string
  balls: string
  fours: string
  sixes: string
  dismissed: boolean
  dismissalKind: CricketDismissalKind
  bowlerId: string
  fielderId: string
}

interface BowlingRowDraft {
  playerId: string
  overs: string
  balls: string
  maidens: string
  runsConceded: string
  wickets: string
  wides: string
  noBalls: string
}

interface InningsDraft {
  battingSideId: string
  bowlingSideId: string
  runs: string
  wickets: string
  overs: string
  balls: string
  declared: boolean
  detailOpen: boolean
  batting: BattingRowDraft[]
  bowling: BowlingRowDraft[]
  extras: { byes: string; legByes: string; wides: string; noBalls: string; penalty: string }
}

function newInnings(battingSideId: string, bowlingSideId: string): InningsDraft {
  return {
    battingSideId,
    bowlingSideId,
    runs: '',
    wickets: '',
    overs: '',
    balls: '',
    declared: false,
    detailOpen: false,
    batting: [],
    bowling: [],
    extras: { byes: '', legByes: '', wides: '', noBalls: '', penalty: '' },
  }
}

function seedInnings(initial: Score | undefined, aId: string, bId: string): InningsDraft[] {
  if (initial?.type !== 'Cricket' || initial.innings.length === 0) {
    return [newInnings(aId, bId)]
  }
  return initial.innings.map((inn) => ({
    battingSideId: inn.batting_side_id,
    bowlingSideId: inn.bowling_side_id,
    runs: String(inn.runs),
    wickets: String(inn.wickets),
    overs: String(inn.overs.overs),
    balls: String(inn.overs.balls),
    declared: inn.declared,
    detailOpen: !!inn.batting?.length || !!inn.bowling?.length,
    batting: (inn.batting ?? []).map((b) => ({
      playerId: b.player_id,
      runs: String(b.runs),
      balls: String(b.balls_faced),
      fours: String(b.fours),
      sixes: String(b.sixes),
      dismissed: !!b.dismissal,
      dismissalKind: b.dismissal?.kind ?? 'bowled',
      bowlerId: b.dismissal?.bowler_player_id ?? '',
      fielderId: b.dismissal?.fielder_player_id ?? '',
    })),
    bowling: (inn.bowling ?? []).map((b) => ({
      playerId: b.player_id,
      overs: String(b.overs.overs),
      balls: String(b.overs.balls),
      maidens: String(b.maidens),
      runsConceded: String(b.runs_conceded),
      wickets: String(b.wickets),
      wides: String(b.wides),
      noBalls: String(b.no_balls),
    })),
    extras: {
      byes: String(inn.extras?.byes ?? ''),
      legByes: String(inn.extras?.leg_byes ?? ''),
      wides: String(inn.extras?.wides ?? ''),
      noBalls: String(inn.extras?.no_balls ?? ''),
      penalty: String(inn.extras?.penalty ?? ''),
    },
  }))
}

function num(v: string): number {
  const n = Number(v)
  return v !== '' && Number.isFinite(n) && n >= 0 ? n : 0
}

function buildInnings(draft: InningsDraft): CricketScoreInnings | null {
  if (draft.runs === '' && draft.wickets === '' && draft.overs === '') return null
  const batting: CricketBattingEntry[] | undefined =
    draft.detailOpen && draft.batting.length > 0
      ? draft.batting
          .filter((b) => b.playerId)
          .map((b) => ({
            player_id: b.playerId,
            runs: num(b.runs),
            balls_faced: num(b.balls),
            fours: num(b.fours),
            sixes: num(b.sixes),
            dismissal: b.dismissed
              ? {
                  kind: b.dismissalKind,
                  bowler_player_id: b.bowlerId || undefined,
                  fielder_player_id: b.fielderId || undefined,
                }
              : undefined,
          }))
      : undefined
  const bowling: CricketBowlingEntry[] | undefined =
    draft.detailOpen && draft.bowling.length > 0
      ? draft.bowling
          .filter((b) => b.playerId)
          .map((b) => ({
            player_id: b.playerId,
            overs: { overs: num(b.overs), balls: num(b.balls) },
            maidens: num(b.maidens),
            runs_conceded: num(b.runsConceded),
            wickets: num(b.wickets),
            wides: num(b.wides),
            no_balls: num(b.noBalls),
          }))
      : undefined
  const hasExtras = Object.values(draft.extras).some((v) => v !== '')
  return {
    batting_side_id: draft.battingSideId,
    bowling_side_id: draft.bowlingSideId,
    runs: num(draft.runs),
    wickets: num(draft.wickets),
    overs: { overs: num(draft.overs), balls: num(draft.balls) },
    declared: draft.declared,
    batting,
    bowling,
    extras:
      draft.detailOpen && hasExtras
        ? {
            byes: num(draft.extras.byes),
            leg_byes: num(draft.extras.legByes),
            wides: num(draft.extras.wides),
            no_balls: num(draft.extras.noBalls),
            penalty: num(draft.extras.penalty),
          }
        : undefined,
  }
}

/**
 * Cricket's result entry: per-innings totals (runs/wickets/overs/declared —
 * there's no prior cricket UI to build on, so this replaces what used to be
 * a meaningless "points" fallback), plus an optional batting/bowling card
 * per innings. `fall_of_wickets` isn't collected here — ordering wickets
 * against a running score is real extra UI for the lowest-value field; a
 * manually-entered result just omits it, same as any other field left blank.
 */
export function CricketScoreFields({
  sideA,
  sideB,
  players,
  initial,
  onChange,
}: {
  sideA: MatchSide
  sideB: MatchSide
  players: MatchPlayer[]
  initial?: Score
  onChange: (built: { score: Score; winnerSideId?: string } | null) => void
}) {
  const [innings, setInnings] = useState<InningsDraft[]>(() => seedInnings(initial, sideA.id, sideB.id))

  useEffect(() => {
    const built = innings.map(buildInnings).filter((i): i is CricketScoreInnings => i !== null)
    if (built.length === 0) {
      onChange(null)
      return
    }
    const totals: Record<string, number> = {}
    for (const inn of built) totals[inn.batting_side_id] = (totals[inn.batting_side_id] ?? 0) + inn.runs
    const a = totals[sideA.id] ?? 0
    const b = totals[sideB.id] ?? 0
    const winnerSideId = a === b ? undefined : a > b ? sideA.id : sideB.id
    // `players` (the score's resolved-name map) is server-only — the backend
    // never persists it (see `CricketScore.players`'s doc comment) — so a
    // manually-built score has nothing to put here.
    onChange({ score: { type: 'Cricket', innings: built, players: {} }, winnerSideId })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [innings, sideA.id, sideB.id])

  const update = (i: number, patch: Partial<InningsDraft>) =>
    setInnings((rows) => rows.map((r, j) => (j === i ? { ...r, ...patch } : r)))

  return (
    <div className="flex flex-col gap-3">
      {innings.map((inn, i) => (
        <InningsRow
          key={i}
          index={i}
          draft={inn}
          sideA={sideA}
          sideB={sideB}
          players={players}
          onChange={(patch) => update(i, patch)}
          onRemove={innings.length > 1 ? () => setInnings((rows) => rows.filter((_, j) => j !== i)) : undefined}
        />
      ))}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="self-start"
        onClick={() => setInnings((rows) => [...rows, newInnings(sideB.id, sideA.id)])}
      >
        <Plus className="size-3.5" /> Add innings
      </Button>
    </div>
  )
}

function InningsRow({
  index,
  draft,
  sideA,
  sideB,
  players,
  onChange,
  onRemove,
}: {
  index: number
  draft: InningsDraft
  sideA: MatchSide
  sideB: MatchSide
  players: MatchPlayer[]
  onChange: (patch: Partial<InningsDraft>) => void
  onRemove?: () => void
}) {
  const battingSide = [sideA, sideB].find((s) => s.id === draft.battingSideId)
  const bowlingSide = [sideA, sideB].find((s) => s.id === draft.bowlingSideId)
  const battingRoster = battingSide ? playersOnSide(players, battingSide) : []
  const bowlingRoster = bowlingSide ? playersOnSide(players, bowlingSide) : []

  return (
    <div className="rounded-lg border p-3">
      <div className="mb-2 flex items-center justify-between">
        <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Innings {index + 1}
        </p>
        {onRemove && (
          <Button type="button" variant="ghost" size="sm" className="h-6 px-1.5 text-xs" onClick={onRemove}>
            Remove
          </Button>
        )}
      </div>

      <p className="mb-1.5 text-xs text-muted-foreground">Batting side</p>
      <SidePicker
        sides={[sideA, sideB]}
        value={draft.battingSideId}
        onChange={(sideId) =>
          onChange({
            battingSideId: sideId,
            bowlingSideId: sideId === sideA.id ? sideB.id : sideA.id,
          })
        }
      />

      <div className="mt-3 grid grid-cols-4 gap-2">
        <Field label="Runs" value={draft.runs} onChange={(v) => onChange({ runs: v })} />
        <Field label="Wickets" value={draft.wickets} onChange={(v) => onChange({ wickets: v })} />
        <Field label="Overs" value={draft.overs} onChange={(v) => onChange({ overs: v })} />
        <Field label="Balls" value={draft.balls} onChange={(v) => onChange({ balls: v })} />
      </div>
      <label className="mt-2 flex items-center gap-1.5 text-xs">
        <input
          type="checkbox"
          checked={draft.declared}
          onChange={(e) => onChange({ declared: e.target.checked })}
        />
        Declared
      </label>

      {!draft.detailOpen ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="mt-2"
          onClick={() => onChange({ detailOpen: true })}
        >
          <Plus className="size-3.5" /> Add batting/bowling card
        </Button>
      ) : (
        <div className="mt-3 flex flex-col gap-3 border-t pt-3">
          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Batting
            </p>
            {draft.batting.map((b, i) => (
              <BattingRow
                key={i}
                draft={b}
                roster={battingRoster}
                bowlingRoster={bowlingRoster}
                onChange={(patch) =>
                  onChange({ batting: draft.batting.map((r, j) => (j === i ? { ...r, ...patch } : r)) })
                }
                onRemove={() => onChange({ batting: draft.batting.filter((_, j) => j !== i) })}
              />
            ))}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() =>
                onChange({
                  batting: [
                    ...draft.batting,
                    {
                      playerId: '',
                      runs: '',
                      balls: '',
                      fours: '',
                      sixes: '',
                      dismissed: false,
                      dismissalKind: 'bowled',
                      bowlerId: '',
                      fielderId: '',
                    },
                  ],
                })
              }
            >
              <Plus className="size-3.5" /> Add batter
            </Button>
          </div>

          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Bowling
            </p>
            {draft.bowling.map((b, i) => (
              <BowlingRow
                key={i}
                draft={b}
                roster={bowlingRoster}
                onChange={(patch) =>
                  onChange({ bowling: draft.bowling.map((r, j) => (j === i ? { ...r, ...patch } : r)) })
                }
                onRemove={() => onChange({ bowling: draft.bowling.filter((_, j) => j !== i) })}
              />
            ))}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() =>
                onChange({
                  bowling: [
                    ...draft.bowling,
                    {
                      playerId: '',
                      overs: '',
                      balls: '',
                      maidens: '',
                      runsConceded: '',
                      wickets: '',
                      wides: '',
                      noBalls: '',
                    },
                  ],
                })
              }
            >
              <Plus className="size-3.5" /> Add bowler
            </Button>
          </div>

          <div>
            <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-muted-foreground">
              Extras
            </p>
            <div className="grid grid-cols-5 gap-1.5">
              <Field label="B" value={draft.extras.byes} onChange={(v) => onChange({ extras: { ...draft.extras, byes: v } })} />
              <Field label="LB" value={draft.extras.legByes} onChange={(v) => onChange({ extras: { ...draft.extras, legByes: v } })} />
              <Field label="WD" value={draft.extras.wides} onChange={(v) => onChange({ extras: { ...draft.extras, wides: v } })} />
              <Field label="NB" value={draft.extras.noBalls} onChange={(v) => onChange({ extras: { ...draft.extras, noBalls: v } })} />
              <Field label="P" value={draft.extras.penalty} onChange={(v) => onChange({ extras: { ...draft.extras, penalty: v } })} />
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function Field({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-center text-[10px] uppercase text-muted-foreground">{label}</span>
      <Input
        type="number"
        min={0}
        inputMode="numeric"
        className="h-8"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="0"
      />
    </div>
  )
}

function BattingRow({
  draft,
  roster,
  bowlingRoster,
  onChange,
  onRemove,
}: {
  draft: BattingRowDraft
  roster: MatchPlayer[]
  bowlingRoster: MatchPlayer[]
  onChange: (patch: Partial<BattingRowDraft>) => void
  onRemove: () => void
}) {
  return (
    <div className="mb-2 rounded-md border p-2">
      <div className="mb-1.5 flex items-center justify-between">
        <PlayerPicker players={roster} value={draft.playerId || undefined} onChange={(id) => onChange({ playerId: id })} />
        <Button type="button" variant="ghost" size="sm" className="h-6 px-1.5 text-xs" onClick={onRemove}>
          Remove
        </Button>
      </div>
      <div className="grid grid-cols-4 gap-1.5">
        <Field label="R" value={draft.runs} onChange={(v) => onChange({ runs: v })} />
        <Field label="B" value={draft.balls} onChange={(v) => onChange({ balls: v })} />
        <Field label="4s" value={draft.fours} onChange={(v) => onChange({ fours: v })} />
        <Field label="6s" value={draft.sixes} onChange={(v) => onChange({ sixes: v })} />
      </div>
      <label className="mt-1.5 flex items-center gap-1.5 text-xs">
        <input type="checkbox" checked={draft.dismissed} onChange={(e) => onChange({ dismissed: e.target.checked })} />
        Dismissed
      </label>
      {draft.dismissed && (
        <div className="mt-1.5 flex flex-col gap-1.5">
          <div className="grid grid-cols-3 gap-1">
            {DISMISSAL_KINDS.map((k) => (
              <button
                key={k}
                type="button"
                aria-pressed={draft.dismissalKind === k}
                onClick={() => onChange({ dismissalKind: k })}
                className={`rounded border px-1.5 py-1 text-[10px] ${draft.dismissalKind === k ? 'border-primary bg-accent' : 'text-muted-foreground'}`}
              >
                {dismissalLabel(k)}
              </button>
            ))}
          </div>
          <PlayerPicker
            players={bowlingRoster}
            value={draft.bowlerId || undefined}
            emptyLabel="Bowler (optional)"
            onChange={(id) => onChange({ bowlerId: draft.bowlerId === id ? '' : id })}
          />
        </div>
      )}
    </div>
  )
}

function BowlingRow({
  draft,
  roster,
  onChange,
  onRemove,
}: {
  draft: BowlingRowDraft
  roster: MatchPlayer[]
  onChange: (patch: Partial<BowlingRowDraft>) => void
  onRemove: () => void
}) {
  return (
    <div className="mb-2 rounded-md border p-2">
      <div className="mb-1.5 flex items-center justify-between">
        <PlayerPicker players={roster} value={draft.playerId || undefined} onChange={(id) => onChange({ playerId: id })} />
        <Button type="button" variant="ghost" size="sm" className="h-6 px-1.5 text-xs" onClick={onRemove}>
          Remove
        </Button>
      </div>
      <div className="grid grid-cols-4 gap-1.5">
        <Field label="Ov" value={draft.overs} onChange={(v) => onChange({ overs: v })} />
        <Field label="Bl" value={draft.balls} onChange={(v) => onChange({ balls: v })} />
        <Field label="Md" value={draft.maidens} onChange={(v) => onChange({ maidens: v })} />
        <Field label="R" value={draft.runsConceded} onChange={(v) => onChange({ runsConceded: v })} />
      </div>
      <div className="mt-1.5 grid grid-cols-3 gap-1.5">
        <Field label="W" value={draft.wickets} onChange={(v) => onChange({ wickets: v })} />
        <Field label="Wd" value={draft.wides} onChange={(v) => onChange({ wides: v })} />
        <Field label="Nb" value={draft.noBalls} onChange={(v) => onChange({ noBalls: v })} />
      </div>
    </div>
  )
}
