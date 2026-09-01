import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Pencil } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'

type Match = components['schemas']['Match']

/** A side's draft `max_players`/`team_join_enabled`. */
interface SideDraft {
  maxPlayers: string // '' = uncapped
  teamJoinEnabled: boolean
}
type SideDrafts = Record<string, SideDraft>

function draftsFromMatch(match: Match): SideDrafts {
  return Object.fromEntries(
    match.sides.map((side) => [
      side.id,
      {
        maxPlayers: side.max_players?.toString() ?? '',
        teamJoinEnabled: side.team_join_enabled,
      },
    ]),
  )
}

/**
 * Editable "join settings" card — whether a self-serve joiner may ever land
 * unassigned (a match-wide ceiling every join link's own preference is
 * capped by — see the backend's `Match.allow_unassigned` doc comment), each
 * side's player cap, and (for a side linked to a team) whether that team's
 * members may join it directly (see `TeamJoinBanner`, the entry point a
 * viewer eligible via this actually sees). Read-only for everyone; an admin
 * (`canManage`, i.e. `canManageMatchJoinSettings`) gets
 * an "Edit" affordance, same posture as `MatchFormatCard`/
 * `MatchDetailsEditor` (whole-value replace on save, not a diff — mirrors
 * `MatchFormatCard`, which always resubmits its full draft rather than
 * tracking what changed).
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
  const [allowUnassigned, setAllowUnassigned] = useState(match.allow_unassigned)
  const [sides, setSides] = useState<SideDrafts>(() => draftsFromMatch(match))

  const save = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body: {
          allow_unassigned: allowUnassigned,
          side_join_settings: match.sides.map((side) => {
            const draft = sides[side.id]
            const raw = draft?.maxPlayers.trim()
            return {
              side_id: side.id,
              max_players: raw ? Number(raw) : undefined,
              team_join_enabled: draft?.teamJoinEnabled ?? false,
            }
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
            {match.allow_unassigned
              ? 'A join link may let people join without a side'
              : 'Everyone must join with a side — never unassigned'}
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
              setAllowUnassigned(match.allow_unassigned)
              setSides(draftsFromMatch(match))
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

      <label className="mb-3 flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          className="mt-0.5"
          checked={allowUnassigned}
          onChange={(e) => setAllowUnassigned(e.target.checked)}
        />
        <span>
          Allow joining without a side
          <span className="block text-xs text-muted-foreground">
            A ceiling, not a default — a join link can still require a side even when this is
            on, but no link can offer "unassigned" when this is off.
          </span>
        </span>
      </label>

      <div className="flex flex-col gap-3">
        {match.sides.map((side, i) => {
          const draft = sides[side.id]
          return (
            <div key={side.id} className="flex flex-col gap-1.5 rounded-lg border p-2">
              <div className="flex items-center gap-2">
                <Label htmlFor={`max-players-${side.id}`} className="flex-1 text-xs">
                  {side.name?.trim() || `Side ${i + 1}`} — max players
                </Label>
                <input
                  id={`max-players-${side.id}`}
                  type="number"
                  min={1}
                  placeholder="No cap"
                  value={draft?.maxPlayers ?? ''}
                  onChange={(e) =>
                    setSides((prev) => ({
                      ...prev,
                      [side.id]: { ...prev[side.id], maxPlayers: e.target.value },
                    }))
                  }
                  className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
                />
              </div>
              {side.team_id && (
                <label className="flex items-center gap-2 text-xs text-muted-foreground">
                  <input
                    type="checkbox"
                    checked={draft?.teamJoinEnabled ?? false}
                    onChange={(e) =>
                      setSides((prev) => ({
                        ...prev,
                        [side.id]: { ...prev[side.id], teamJoinEnabled: e.target.checked },
                      }))
                    }
                  />
                  Let this side's team members join directly
                </label>
              )}
            </div>
          )
        })}
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
