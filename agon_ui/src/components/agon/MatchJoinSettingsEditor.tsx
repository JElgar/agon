import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Pencil } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'

type Match = components['schemas']['Match']
type SideSelection = components['schemas']['SideSelection']

const SIDE_SELECTION_LABEL: Record<SideSelection, string> = {
  unassigned_only: 'Always unassigned — you place people on sides later',
  side_required: 'Must pick a side to join',
  side_optional: 'May pick a side, or join unassigned',
}

/** A side's draft `max_players`, as a form-friendly string (`''` = uncapped). */
type MaxPlayersDraft = Record<string, string>

function draftFromMatch(match: Match): MaxPlayersDraft {
  return Object.fromEntries(
    match.sides.map((side) => [side.id, side.max_players?.toString() ?? '']),
  )
}

/**
 * Editable "join settings" card — whether/how a self-serve joiner may pick a
 * side, and each side's player cap. Read-only for everyone; an admin
 * (`canManage`, i.e. `canManageMatchJoinSettings`) gets an "Edit" affordance,
 * same posture as `MatchFormatCard`/`MatchDetailsEditor` (whole-value
 * replace on save, not a diff — mirrors `MatchFormatCard`, which always
 * resubmits its full draft rather than tracking what changed).
 */
export function MatchJoinSettingsEditor({
  match,
  canManage,
}: {
  match: Match
  canManage: boolean
}) {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [sideSelection, setSideSelection] = useState<SideSelection>(
    match.join_policy.side_selection,
  )
  const [maxPlayers, setMaxPlayers] = useState<MaxPlayersDraft>(() => draftFromMatch(match))

  const save = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body: {
          join_policy: { side_selection: sideSelection },
          side_max_players: match.sides.map((side) => {
            const raw = maxPlayers[side.id]?.trim()
            return { side_id: side.id, max_players: raw ? Number(raw) : undefined }
          }),
        },
      })
      if (error) throw new Error('Failed to save join settings')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      setEditing(false)
    },
  })

  // `!= null` (not `!== undefined`): the server serializes a Rust
  // `Option::None` here as JSON `null`, not an absent key.
  const derivedCap =
    match.sides.length > 0 && match.sides.every((s) => s.max_players != null)
      ? match.sides.reduce((sum, s) => sum + (s.max_players ?? 0), 0)
      : undefined

  if (!editing) {
    return (
      <div className="flex items-center justify-between rounded-xl border bg-card p-4">
        <div>
          <p className="text-sm font-medium">Join settings</p>
          <p className="text-xs text-muted-foreground">
            {SIDE_SELECTION_LABEL[match.join_policy.side_selection]}
          </p>
          <p className="text-xs text-muted-foreground">
            {match.sides.every((s) => s.max_players == null)
              ? 'No player caps set'
              : `${match.sides
                  .map((s, i) => `${s.name?.trim() || `Side ${i + 1}`}: ${s.max_players ?? '∞'}`)
                  .join(' · ')}${derivedCap !== undefined ? ` (max ${derivedCap} total)` : ''}`}
          </p>
        </div>
        {canManage && (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 gap-1 px-2 text-xs text-muted-foreground"
            onClick={() => {
              setSideSelection(match.join_policy.side_selection)
              setMaxPlayers(draftFromMatch(match))
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
      <p className="mb-3 text-sm font-medium">Join settings</p>

      <div className="mb-3">
        <Label htmlFor="side-selection" className="text-xs text-muted-foreground">
          Who can pick a side?
        </Label>
        <select
          id="side-selection"
          value={sideSelection}
          onChange={(e) => setSideSelection(e.target.value as SideSelection)}
          className="mt-1 h-9 w-full rounded-md border bg-background px-3 text-sm"
        >
          {(Object.keys(SIDE_SELECTION_LABEL) as SideSelection[]).map((value) => (
            <option key={value} value={value}>
              {SIDE_SELECTION_LABEL[value]}
            </option>
          ))}
        </select>
      </div>

      <div className="flex flex-col gap-2">
        {match.sides.map((side, i) => (
          <div key={side.id} className="flex items-center gap-2">
            <Label htmlFor={`max-players-${side.id}`} className="flex-1 text-xs">
              {side.name?.trim() || `Side ${i + 1}`} — max players
            </Label>
            <input
              id={`max-players-${side.id}`}
              type="number"
              min={1}
              placeholder="No cap"
              value={maxPlayers[side.id] ?? ''}
              onChange={(e) =>
                setMaxPlayers((prev) => ({ ...prev, [side.id]: e.target.value }))
              }
              className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
            />
          </div>
        ))}
      </div>

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
