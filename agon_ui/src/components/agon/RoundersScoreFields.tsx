import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SidePicker, PlayerPicker } from '@/components/agon/live/Pickers'
import { playersOnSide } from '@/lib/members'
import { outKindLabel, type RoundersOutKind } from '@/lib/roundersScore'

type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type RoundersScoreInnings = components['schemas']['RoundersScoreInnings']
type RoundersBattingEntry = components['schemas']['RoundersBattingEntry']
type RoundersBowlingEntry = components['schemas']['RoundersBowlingEntry']

const OUT_KINDS: RoundersOutKind[] = [
  'caught',
  'stumped_at_post',
  'overtook_runner',
  'obstruction',
  'failed_to_run',
]

interface BattingRowDraft {
  playerId: string
  halfRounders: string
  balls: string
  out: boolean
  outKind: RoundersOutKind
  fielderId: string
}

interface BowlingRowDraft {
  playerId: string
  ballsBowled: string
  noBalls: string
  outs: string
  halfRoundersConceded: string
}

interface InningsDraft {
  battingSideId: string
  fieldingSideId: string
  halfRounders: string
  outs: string
  goodBalls: string
  byes: string
  detailOpen: boolean
  batting: BattingRowDraft[]
  bowling: BowlingRowDraft[]
}

function newInnings(battingSideId: string, fieldingSideId: string): InningsDraft {
  return {
    battingSideId,
    fieldingSideId,
    halfRounders: '',
    outs: '',
    goodBalls: '',
    byes: '',
    detailOpen: false,
    batting: [],
    bowling: [],
  }
}

function seedInnings(initial: Score | undefined, aId: string, bId: string): InningsDraft[] {
  if (initial?.type !== 'Rounders' || initial.innings.length === 0) {
    return [newInnings(aId, bId)]
  }
  return initial.innings.map((inn) => ({
    battingSideId: inn.batting_side_id,
    fieldingSideId: inn.fielding_side_id,
    halfRounders: String(inn.half_rounders),
    outs: String(inn.outs),
    goodBalls: String(inn.good_balls_bowled),
    byes: String(inn.byes),
    detailOpen: !!inn.batting?.length || !!inn.bowling?.length,
    batting: (inn.batting ?? []).map((b) => ({
      playerId: b.player_id,
      halfRounders: String(b.half_rounders),
      balls: String(b.balls_faced),
      out: !!b.out,
      outKind: b.out?.kind ?? 'caught',
      fielderId: b.out?.fielder_player_id ?? '',
    })),
    bowling: (inn.bowling ?? []).map((b) => ({
      playerId: b.player_id,
      ballsBowled: String(b.balls_bowled),
      noBalls: String(b.no_balls),
      outs: String(b.outs),
      halfRoundersConceded: String(b.half_rounders_conceded),
    })),
  }))
}

function num(v: string): number {
  const n = Number(v)
  return v !== '' && Number.isFinite(n) && n >= 0 ? n : 0
}

function buildInnings(draft: InningsDraft): RoundersScoreInnings | null {
  if (draft.halfRounders === '' && draft.outs === '' && draft.goodBalls === '') return null
  const batting: RoundersBattingEntry[] | undefined =
    draft.detailOpen && draft.batting.length > 0
      ? draft.batting
          .filter((b) => b.playerId)
          .map((b) => ({
            player_id: b.playerId,
            half_rounders: num(b.halfRounders),
            balls_faced: num(b.balls),
            out: b.out
              ? { kind: b.outKind, fielder_player_id: b.fielderId || undefined }
              : undefined,
          }))
      : undefined
  const bowling: RoundersBowlingEntry[] | undefined =
    draft.detailOpen && draft.bowling.length > 0
      ? draft.bowling
          .filter((b) => b.playerId)
          .map((b) => ({
            player_id: b.playerId,
            balls_bowled: num(b.ballsBowled),
            no_balls: num(b.noBalls),
            outs: num(b.outs),
            half_rounders_conceded: num(b.halfRoundersConceded),
          }))
      : undefined
  return {
    batting_side_id: draft.battingSideId,
    fielding_side_id: draft.fieldingSideId,
    half_rounders: num(draft.halfRounders),
    outs: num(draft.outs),
    good_balls_bowled: num(draft.goodBalls),
    byes: num(draft.byes),
    batting,
    bowling,
  }
}

/**
 * Rounders' result entry — per-innings totals (half-rounders/outs/good
 * balls/byes), plus an optional batting/bowling card per innings. Mirrors
 * `CricketScoreFields`' shape closely (same innings-based sport), with
 * `fall_of_outs` left uncollected here for the same reason cricket omits
 * `fall_of_wickets`: ordering outs against a running score is real extra UI
 * for the lowest-value field.
 */
export function RoundersScoreFields({
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
    const built = innings.map(buildInnings).filter((i): i is RoundersScoreInnings => i !== null)
    if (built.length === 0) {
      onChange(null)
      return
    }
    const totals: Record<string, number> = {}
    for (const inn of built)
      totals[inn.batting_side_id] = (totals[inn.batting_side_id] ?? 0) + inn.half_rounders
    const a = totals[sideA.id] ?? 0
    const b = totals[sideB.id] ?? 0
    const winnerSideId = a === b ? undefined : a > b ? sideA.id : sideB.id
    // `players` (the score's resolved-name map) is server-only — a manually
    // built score has nothing to put here, same as cricket/football.
    onChange({ score: { type: 'Rounders', innings: built, players: {} }, winnerSideId })
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
  const fieldingSide = [sideA, sideB].find((s) => s.id === draft.fieldingSideId)
  const battingRoster = battingSide ? playersOnSide(players, battingSide) : []
  const fieldingRoster = fieldingSide ? playersOnSide(players, fieldingSide) : []

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
            fieldingSideId: sideId === sideA.id ? sideB.id : sideA.id,
          })
        }
      />

      <div className="mt-3 grid grid-cols-4 gap-2">
        <Field label="½ rounders" value={draft.halfRounders} onChange={(v) => onChange({ halfRounders: v })} />
        <Field label="Outs" value={draft.outs} onChange={(v) => onChange({ outs: v })} />
        <Field label="Good balls" value={draft.goodBalls} onChange={(v) => onChange({ goodBalls: v })} />
        <Field label="Byes" value={draft.byes} onChange={(v) => onChange({ byes: v })} />
      </div>

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
                fieldingRoster={fieldingRoster}
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
                      halfRounders: '',
                      balls: '',
                      out: false,
                      outKind: 'caught',
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
                roster={fieldingRoster}
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
                    { playerId: '', ballsBowled: '', noBalls: '', outs: '', halfRoundersConceded: '' },
                  ],
                })
              }
            >
              <Plus className="size-3.5" /> Add bowler
            </Button>
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
  fieldingRoster,
  onChange,
  onRemove,
}: {
  draft: BattingRowDraft
  roster: MatchPlayer[]
  fieldingRoster: MatchPlayer[]
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
      <div className="grid grid-cols-2 gap-1.5">
        <Field label="½ rounders" value={draft.halfRounders} onChange={(v) => onChange({ halfRounders: v })} />
        <Field label="Balls faced" value={draft.balls} onChange={(v) => onChange({ balls: v })} />
      </div>
      <label className="mt-1.5 flex items-center gap-1.5 text-xs">
        <input type="checkbox" checked={draft.out} onChange={(e) => onChange({ out: e.target.checked })} />
        Out
      </label>
      {draft.out && (
        <div className="mt-1.5 flex flex-col gap-1.5">
          <div className="grid grid-cols-2 gap-1">
            {OUT_KINDS.map((k) => (
              <button
                key={k}
                type="button"
                aria-pressed={draft.outKind === k}
                onClick={() => onChange({ outKind: k })}
                className={`rounded border px-1.5 py-1 text-[10px] ${draft.outKind === k ? 'border-primary bg-accent' : 'text-muted-foreground'}`}
              >
                {outKindLabel(k)}
              </button>
            ))}
          </div>
          <PlayerPicker
            players={fieldingRoster}
            value={draft.fielderId || undefined}
            emptyLabel="Fielder (optional)"
            onChange={(id) => onChange({ fielderId: draft.fielderId === id ? '' : id })}
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
      <div className="grid grid-cols-2 gap-1.5">
        <Field label="Balls" value={draft.ballsBowled} onChange={(v) => onChange({ ballsBowled: v })} />
        <Field label="No balls" value={draft.noBalls} onChange={(v) => onChange({ noBalls: v })} />
      </div>
      <div className="mt-1.5 grid grid-cols-2 gap-1.5">
        <Field label="Outs" value={draft.outs} onChange={(v) => onChange({ outs: v })} />
        <Field label="½ rounders conceded" value={draft.halfRoundersConceded} onChange={(v) => onChange({ halfRoundersConceded: v })} />
      </div>
    </div>
  )
}
