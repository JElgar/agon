import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { isSetsSport } from '@/lib/sports'

type Match = components['schemas']['Match']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

/** One row of the sets editor: games won by each side in a single set. */
interface SetRow {
  a: string
  b: string
}

function sideLabel(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/**
 * Seed the sets editor from the match's confirmed score, if it's a sets score
 * keyed to these two sides. Falls back to two empty rows for a fresh result.
 */
function seedSets(match: Match, aId: string, bId: string): SetRow[] {
  const score = match.confirmed_score?.score
  if (score && score.type === 'Sets') {
    const a = score.entries.find((e) => e.side_id === aId)?.sets ?? []
    const b = score.entries.find((e) => e.side_id === bId)?.sets ?? []
    const rows = Math.max(a.length, b.length)
    if (rows > 0) {
      return Array.from({ length: rows }, (_, i) => ({
        a: a[i]?.toString() ?? '',
        b: b[i]?.toString() ?? '',
      }))
    }
  }
  return [
    { a: '', b: '' },
    { a: '', b: '' },
  ]
}

/** Seed the simple points editor from a confirmed simple score, else blanks. */
function seedPoints(match: Match, aId: string, bId: string): [string, string] {
  const score = match.confirmed_score?.score
  if (score && score.type === 'Simple') {
    const a = score.entries.find((e) => e.side_id === aId)?.points
    const b = score.entries.find((e) => e.side_id === bId)?.points
    return [a?.toString() ?? '', b?.toString() ?? '']
  }
  return ['', '']
}

/**
 * Inline editor for a match's result, opened from the detail card. Renders a
 * sets grid for racket sports and a single points pair otherwise (mirroring the
 * create flow), seeded from any existing confirmed score. On save it PATCHes the
 * score against the match's real side ids; a changed score re-enters the
 * confirmation flow server-side (the other side is asked to confirm), so we also
 * refresh so the pending-score prompt appears.
 */
export function MatchResultEditor({
  match,
  onDone,
}: {
  match: Match
  onDone: () => void
}) {
  const queryClient = useQueryClient()
  const [sideA, sideB] = match.sides
  const aId = sideA?.id ?? ''
  const bId = sideB?.id ?? ''
  const nameA = sideLabel(match, 0, 'Side A')
  const nameB = sideLabel(match, 1, 'Side B')

  const setsMode = isSetsSport(match.match_type)
  const [sets, setSets] = useState<SetRow[]>(() => seedSets(match, aId, bId))
  const [points, setPoints] = useState<[string, string]>(() =>
    seedPoints(match, aId, bId),
  )
  const [pointsA, pointsB] = points

  /** Build the score payload + derived winner, or null when incomplete. */
  const build = (): { score: Score; winner?: string } | null => {
    if (setsMode) {
      const rows = sets
        .map((r) => ({ a: Number(r.a), b: Number(r.b) }))
        .filter(
          (r) =>
            Number.isFinite(r.a) &&
            Number.isFinite(r.b) &&
            r.a >= 0 &&
            r.b >= 0 &&
            (r.a > 0 || r.b > 0),
        )
      if (rows.length === 0) return null
      let aSets = 0
      let bSets = 0
      for (const r of rows) {
        if (r.a > r.b) aSets += 1
        else if (r.b > r.a) bSets += 1
      }
      const score = {
        type: 'Sets',
        entries: [
          { side_id: aId, sets: rows.map((r) => r.a) },
          { side_id: bId, sets: rows.map((r) => r.b) },
        ],
      } as unknown as Score
      const winner = aSets === bSets ? undefined : aSets > bSets ? aId : bId
      return { score, winner }
    }

    const a = Number(pointsA)
    const b = Number(pointsB)
    if (
      pointsA === '' ||
      pointsB === '' ||
      !Number.isFinite(a) ||
      !Number.isFinite(b)
    )
      return null
    const score = {
      type: 'Simple',
      entries: [
        { side_id: aId, points: a },
        { side_id: bId, points: b },
      ],
    } as unknown as Score
    const winner = a === b ? undefined : a > b ? aId : bId
    return { score, winner }
  }

  const built = build()

  const save = useMutation({
    mutationFn: async () => {
      if (!built) throw new Error('Enter a score first')
      const body: UpdateMatchInput = {
        // The generated type is `Omit<Score,'type'>`; the server needs the
        // discriminator, which we injected when building `score`.
        score: built.score as UpdateMatchInput['score'],
      }
      if (built.winner) body.winner_side_id = built.winner
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (error) throw new Error('Failed to save the result')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      onDone()
    },
  })

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <p className="text-sm font-medium">Result</p>

      {setsMode ? (
        <div className="flex flex-col gap-2">
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2 text-center text-[11px] uppercase tracking-wider text-muted-foreground">
            <span className="truncate text-left">{nameA}</span>
            <span>Set</span>
            <span className="truncate text-right">{nameB}</span>
          </div>
          {sets.map((row, i) => (
            <div
              key={i}
              className="grid grid-cols-[1fr_auto_1fr] items-center gap-2"
            >
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={row.a}
                onChange={(e) =>
                  setSets((s) =>
                    s.map((r, j) => (j === i ? { ...r, a: e.target.value } : r)),
                  )
                }
                placeholder="0"
              />
              <span className="text-xs text-muted-foreground">Set {i + 1}</span>
              <Input
                type="number"
                min={0}
                inputMode="numeric"
                value={row.b}
                onChange={(e) =>
                  setSets((s) =>
                    s.map((r, j) => (j === i ? { ...r, b: e.target.value } : r)),
                  )
                }
                placeholder="0"
              />
            </div>
          ))}
          <div className="flex justify-between">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setSets((s) => [...s, { a: '', b: '' }])}
            >
              Add set
            </Button>
            {sets.length > 1 && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => setSets((s) => s.slice(0, -1))}
              >
                Remove set
              </Button>
            )}
          </div>
        </div>
      ) : (
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="flex flex-col gap-1">
            <span className="truncate text-center text-xs text-muted-foreground">
              {nameA}
            </span>
            <Input
              type="number"
              min={0}
              inputMode="numeric"
              value={pointsA}
              onChange={(e) => setPoints([e.target.value, pointsB])}
              placeholder="0"
            />
          </div>
          <span className="pt-5 text-muted-foreground">–</span>
          <div className="flex flex-col gap-1">
            <span className="truncate text-center text-xs text-muted-foreground">
              {nameB}
            </span>
            <Input
              type="number"
              min={0}
              inputMode="numeric"
              value={pointsB}
              onChange={(e) => setPoints([pointsA, e.target.value])}
              placeholder="0"
            />
          </div>
        </div>
      )}

      {save.isError && (
        <p className="text-xs text-destructive">
          {(save.error as Error).message}
        </p>
      )}

      <div className="flex gap-2">
        <Button
          size="sm"
          disabled={!built || save.isPending}
          onClick={() => save.mutate()}
        >
          {save.isPending ? 'Saving…' : 'Save result'}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={save.isPending}
          onClick={onDone}
        >
          Cancel
        </Button>
      </div>
    </div>
  )
}
