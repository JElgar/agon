import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { isSetsSport } from '@/lib/sports'
import { FootballScoreFields } from '@/components/agon/FootballScoreFields'
import { CricketScoreFields } from '@/components/agon/CricketScoreFields'
import { displayScore, headlineBySide } from '@/lib/score'
import { cricketProgressFromScore, matchTotalsBySide } from '@/lib/cricketScore'

type Match = components['schemas']['Match']
type UpdateMatchInput = components['schemas']['UpdateMatchInput']
type Score = components['schemas']['Score']

/** A short "X–Y" summary of a score for the two given sides — used to show
 *  what the live/recorded score actually is when it disagrees with what's
 *  being saved. Cricket has no single headline number (see `headlineBySide`),
 *  so it sums each side's runs across every innings played so far instead. */
function scoreSummary(score: Score, aId: string, bId: string): string {
  const totals =
    score.type === 'Cricket' ? matchTotalsBySide(cricketProgressFromScore(score)) : headlineBySide(score)
  return `${totals[aId] ?? 0}–${totals[bId] ?? 0}`
}

/** One row of the sets editor: games won by each side in a single set. */
interface SetRow {
  a: string
  b: string
}

function sideLabel(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/**
 * Seed the sets editor from the match's current score (confirmed, or pending
 * if not yet confirmed — see `displayScore`), if it's a sets score keyed to
 * these two sides. Falls back to two empty rows for a fresh result.
 */
function seedSets(match: Match, aId: string, bId: string): SetRow[] {
  const score = displayScore(match)?.score
  if (score && score.type === 'Sets') {
    const a = score.entries[aId] ?? []
    const b = score.entries[bId] ?? []
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

/** Seed the simple points editor from the match's current simple score
 *  (confirmed, or pending if not yet confirmed), else blanks. */
function seedPoints(match: Match, aId: string, bId: string): [string, string] {
  const score = displayScore(match)?.score
  if (score && score.type === 'Simple') {
    const a = score.entries[aId]
    const b = score.entries[bId]
    return [a?.toString() ?? '', b?.toString() ?? '']
  }
  return ['', '']
}

/**
 * Inline editor for a match's result, opened from the detail card. Renders a
 * sets grid for racket sports, the football/cricket editors (goal-by-goal /
 * innings detail, optional) for those sports, and a single points pair
 * otherwise (mirroring the create flow), seeded from the match's current
 * result — confirmed if there is one, else a still-pending submission (see
 * `displayScore`). On save it PATCHes the score against the match's real side ids; a
 * changed score re-enters the confirmation flow server-side (the other side
 * is asked to confirm), so we also refresh so the pending-score prompt
 * appears.
 *
 * For a live-scored football/cricket match, a save that disagrees with the
 * server's own live-derived score comes back as a 409 rather than being
 * silently accepted — unlike `finishMatch` (which just asks the user to
 * refresh and try again), this editor is specifically a correction tool, so
 * it offers a way through: fetch and show what the live/recorded score
 * actually is, then let a second tap resubmit with `override_live_score` set.
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

  const isFootball = match.match_type === 'football'
  const isCricket = match.match_type === 'cricket'
  const setsMode = isSetsSport(match.match_type)
  // The result to prepopulate the form with: confirmed if there is one, else
  // a still-pending submission awaiting the other side's confirmation.
  const currentScore = displayScore(match)?.score
  const [sets, setSets] = useState<SetRow[]>(() => seedSets(match, aId, bId))
  const [points, setPoints] = useState<[string, string]>(() => seedPoints(match, aId, bId))
  const [pointsA, pointsB] = points
  const [detailBuilt, setDetailBuilt] = useState<{ score: Score; winnerSideId?: string } | null>(null)

  /** Build the score payload + derived winner, or null when incomplete. */
  const build = (): { score: Score; winner?: string } | null => {
    if (isFootball || isCricket) {
      return detailBuilt ? { score: detailBuilt.score, winner: detailBuilt.winnerSideId } : null
    }

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
      const score: Score = {
        type: 'Sets',
        entries: {
          [aId]: rows.map((r) => r.a),
          [bId]: rows.map((r) => r.b),
        },
      }
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
    const score: Score = {
      type: 'Simple',
      entries: { [aId]: a, [bId]: b },
    }
    const winner = a === b ? undefined : a > b ? aId : bId
    return { score, winner }
  }

  const built = build()

  // Set when a save is rejected (409) because it disagrees with the match's
  // live/recorded score — `null` while there's nothing to warn about. Holds
  // that live score itself, so the prompt can show exactly what it is
  // instead of just "something doesn't match". Cleared whenever the input
  // changes so a fresh edit doesn't carry a stale warning around, and an
  // edit that resolves the disagreement goes back to a normal save.
  const [conflict, setConflict] = useState<Score | null>(null)
  useEffect(() => {
    setConflict(null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [built ? JSON.stringify(built.score) : null])

  const save = useMutation({
    mutationFn: async (override: boolean): Promise<'saved' | 'conflict'> => {
      if (!built) throw new Error('Enter a score first')
      const body: UpdateMatchInput = {
        score: built.score,
      }
      if (built.winner) body.winner_side_id = built.winner
      if (override) body.override_live_score = true
      const { error, response } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (response.status === 409) {
        const { data } = await fetchClient.GET('/matches/{match_id}/score', {
          params: { path: { match_id: match.id } },
        })
        setConflict(data ?? null)
        return 'conflict'
      }
      if (error) throw new Error('Failed to save the result')
      return 'saved'
    },
    onSuccess: (result) => {
      if (result === 'conflict') return
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      onDone()
    },
  })

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <p className="text-sm font-medium">Result</p>

      {isFootball && sideA && sideB ? (
        <FootballScoreFields
          sideA={sideA}
          sideB={sideB}
          players={match.players}
          initial={currentScore}
          onChange={setDetailBuilt}
        />
      ) : isCricket && sideA && sideB ? (
        <CricketScoreFields
          sideA={sideA}
          sideB={sideB}
          players={match.players}
          initial={currentScore}
          onChange={setDetailBuilt}
        />
      ) : setsMode ? (
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

      {conflict && (
        <div className="rounded-lg border border-warning/30 bg-warning/10 p-3">
          <p className="text-xs font-medium text-foreground">
            This doesn't match the live/recorded score ({scoreSummary(conflict, aId, bId)}). Saving
            will overwrite it.
          </p>
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
          onClick={() => save.mutate(!!conflict)}
        >
          {save.isPending ? 'Saving…' : conflict ? 'Save anyway' : 'Save result'}
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
