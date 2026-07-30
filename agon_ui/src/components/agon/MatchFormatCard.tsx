import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Pencil } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { CricketFields, FootballFields } from './MatchFormatEditor'
import {
  cricketFormat,
  footballFormat,
  oversLimitLabel,
  type CricketFormat,
  type FootballFormat,
} from '@/lib/matchFormat'

type Match = components['schemas']['Match']

/** Read-only summary line for whatever format is in effect (the match's own
 *  if set, else the app default — so this always shows real numbers, never
 *  "not configured"). */
function FormatSummary({ match }: { match: Match }) {
  if (match.match_type === 'cricket') {
    const fmt = cricketFormat(match.format)
    return (
      <p className="text-xs text-muted-foreground">
        {oversLimitLabel(fmt)} · {fmt.innings_per_side === 1 ? 'single' : 'two'} innings ·
        no-ball {fmt.no_ball_penalty_runs} run{fmt.no_ball_penalty_runs === 1 ? '' : 's'}
        {!fmt.no_ball_is_extra_ball && ' (no extra ball)'}
        {fmt.free_hit_after_no_ball && ' + free hit'}
        {!fmt.wide_is_extra_ball && ' · wide: no extra ball'}
      </p>
    )
  }
  if (match.match_type === 'football') {
    const fmt = footballFormat(match.format)
    return (
      <p className="text-xs text-muted-foreground">
        {fmt.num_halves} × {fmt.half_length_minutes}min
        {fmt.extra_time && ' · extra time'}
        {fmt.penalties && ' · penalties'}
      </p>
    )
  }
  return null
}

/**
 * Editable "match format" card — half length/overs limit/penalty runs, shown
 * on the match detail page for football/cricket. Read-only for everyone;
 * participants get an "Edit" affordance, same posture as `MatchDetailsEditor`.
 * Always edits a concrete value (seeded from the match's own format, or the
 * app default) — there's no "unset" option here, only at creation time (see
 * `MatchFormatEditor`'s doc comment for why).
 */
export function MatchFormatCard({
  match,
  canEdit,
}: {
  match: Match
  canEdit: boolean
}) {
  const queryClient = useQueryClient()
  // The server locks the format once the match has left `scheduled` (scoring
  // started, or it's complete/cancelled) — hide the affordance to match, so
  // there's no dead "Edit" button that just 400s.
  const canEditFormat = canEdit && match.status === 'scheduled'
  const [editing, setEditing] = useState(false)
  const [footballDraft, setFootballDraft] = useState<FootballFormat>(() =>
    footballFormat(match.format),
  )
  const [cricketDraft, setCricketDraft] = useState<CricketFormat>(() =>
    cricketFormat(match.format),
  )

  const save = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body: {
          format:
            match.match_type === 'cricket'
              ? { sport: 'Cricket', ...cricketDraft }
              : { sport: 'Football', ...footballDraft },
        },
      })
      if (error) throw new Error('Failed to save the match format')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      setEditing(false)
    },
  })

  if (match.match_type !== 'football' && match.match_type !== 'cricket') return null

  if (!editing) {
    return (
      <div className="flex items-center justify-between rounded-xl border bg-card p-4">
        <div>
          <p className="text-sm font-medium">Format</p>
          <FormatSummary match={match} />
        </div>
        {canEditFormat && (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 gap-1 px-2 text-xs text-muted-foreground"
            onClick={() => {
              setFootballDraft(footballFormat(match.format))
              setCricketDraft(cricketFormat(match.format))
              setEditing(true)
            }}
          >
            <Pencil className="size-3" /> Edit
          </Button>
        )}
      </div>
    )
  }

  return (
    <div className="rounded-xl border bg-card p-4">
      <p className="mb-3 text-sm font-medium">Format</p>
      {match.match_type === 'cricket' ? (
        <CricketFields value={cricketDraft} onChange={setCricketDraft} />
      ) : (
        <FootballFields value={footballDraft} onChange={setFootballDraft} />
      )}
      {save.isError && (
        <p className="mt-2 text-xs text-destructive">{(save.error as Error).message}</p>
      )}
      <div className="mt-3 flex gap-2">
        <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? 'Saving…' : 'Save'}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={save.isPending}
          onClick={() => setEditing(false)}
        >
          Cancel
        </Button>
      </div>
    </div>
  )
}
