import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { isoToDateTimeLocal } from '@/lib/datetime'
import { MultiImageUploadField } from '@/components/agon/MultiImageUploadField'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type UpdateMatchSideNameInput = components['schemas']['UpdateMatchSideNameInput']

/** Whether a side can be given its own custom name: an ad-hoc side (no team)
 *  always can; a team-linked side only when another side shares that same
 *  team (otherwise the team's own name is the source of truth) — mirrors the
 *  server's create/update validation. */
function isRenameable(side: MatchSide, sides: MatchSide[]): boolean {
  if (!side.team_id) return true
  return sides.some((other) => other.id !== side.id && other.team_id === side.team_id)
}

/**
 * Inline editor for a match's metadata — name, description, start time,
 * header photos, and side names — shown in place of the details card when a
 * participant taps "Edit". Saves via `PATCH /matches/{id}` (only the changed
 * fields are sent) and, on success, refreshes the match and feed then closes
 * back to the read-only card.
 *
 * Roster and result are edited elsewhere (invite control, result editor), so
 * this deliberately covers just the descriptive fields, photos, and side
 * names. The server has no id-level append/reorder for photos, only "replace
 * with this full list", so the photo field is seeded with the match's
 * *current* photos (by asset id) and the user's edits — reorder, remove, add
 * — are applied on top of that seed before being sent back as the complete
 * list on save.
 */
export function MatchDetailsEditor({
  match,
  onDone,
}: {
  match: Match
  onDone: () => void
}) {
  const queryClient = useQueryClient()
  const [name, setName] = useState(match.name)
  const [description, setDescription] = useState(match.description)
  // Local wall-clock for the datetime-local control, seeded from the stored UTC.
  const [startsAt, setStartsAt] = useState(isoToDateTimeLocal(match.starts_at))

  const existingHeaderAssetIds = match.header_photos.map((p) => p.asset_id!)
  const [headerAssetIds, setHeaderAssetIds] = useState<string[]>(existingHeaderAssetIds)

  // Seeded from each side's currently-*displayed* name (server-resolved —
  // could be a custom name, a team's name, or a computed fallback). Editing
  // and saving sets that side's custom name explicitly; clearing the field
  // back to empty reverts to whatever the server would otherwise resolve.
  const [sideNames, setSideNames] = useState<Record<string, string>>(
    Object.fromEntries(match.sides.map((s) => [s.id, s.name ?? ''])),
  )

  const nameError = name.trim().length === 0 ? 'A match needs a name' : null
  const timeError = Number.isNaN(new Date(startsAt).getTime())
    ? 'Pick a valid date and time'
    : null
  const valid = !nameError && !timeError

  const save = useMutation({
    mutationFn: async () => {
      // Send only fields that actually changed, so we don't rewrite untouched
      // values (and a no-op save stays a no-op).
      const body: components['schemas']['UpdateMatchInput'] = {}
      if (name.trim() !== match.name) body.name = name.trim()
      if (description !== match.description) body.description = description
      const newIso = new Date(startsAt).toISOString()
      if (newIso !== match.starts_at) body.starts_at = newIso
      if (JSON.stringify(headerAssetIds) !== JSON.stringify(existingHeaderAssetIds)) {
        body.header_photo_asset_ids = headerAssetIds
      }

      const sideNameUpdates: UpdateMatchSideNameInput[] = []
      for (const side of match.sides) {
        if (!isRenameable(side, match.sides)) continue
        const original = (side.name ?? '').trim()
        const next = (sideNames[side.id] ?? '').trim()
        if (next === original) continue
        // An empty field clears the custom name (omitting `name` — the
        // server treats a missing key the same as `null`), falling back to
        // the server-resolved default rather than sending an empty string.
        sideNameUpdates.push(next ? { side_id: side.id, name: next } : { side_id: side.id })
      }
      if (sideNameUpdates.length > 0) body.side_names = sideNameUpdates

      if (Object.keys(body).length === 0) return // nothing changed

      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (error) throw new Error('Failed to save changes')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      onDone()
    },
  })

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <div>
        <Label htmlFor="match-name" className="text-xs text-muted-foreground">
          Match name
        </Label>
        <Input
          id="match-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="mt-1"
          autoFocus
        />
        {nameError && (
          <p className="mt-1 text-xs text-destructive">{nameError}</p>
        )}
      </div>

      <div>
        <Label
          htmlFor="match-description"
          className="text-xs text-muted-foreground"
        >
          Description
        </Label>
        <Input
          id="match-description"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Optional"
          className="mt-1"
        />
      </div>

      <div>
        <Label htmlFor="match-starts" className="text-xs text-muted-foreground">
          When
        </Label>
        <Input
          id="match-starts"
          type="datetime-local"
          value={startsAt}
          onChange={(e) => setStartsAt(e.target.value)}
          className="mt-1"
        />
        {timeError && (
          <p className="mt-1 text-xs text-destructive">{timeError}</p>
        )}
      </div>

      <div className="flex flex-col gap-2">
        {match.sides.map((side, i) => {
          const renameable = isRenameable(side, match.sides)
          return (
            <div key={side.id}>
              <Label
                htmlFor={`side-name-${side.id}`}
                className="text-xs text-muted-foreground"
              >
                {i === 0 ? 'Side A name' : i === 1 ? 'Side B name' : `Side ${i + 1} name`}
              </Label>
              <Input
                id={`side-name-${side.id}`}
                value={sideNames[side.id] ?? ''}
                onChange={(e) =>
                  setSideNames((prev) => ({ ...prev, [side.id]: e.target.value }))
                }
                placeholder={renameable ? 'Optional' : undefined}
                disabled={!renameable}
                className="mt-1"
              />
              {!renameable && (
                <p className="mt-1 text-xs text-muted-foreground">
                  Linked to a team — rename the team instead.
                </p>
              )}
            </div>
          )
        })}
      </div>

      <div>
        <Label className="text-xs text-muted-foreground">Photos</Label>
        <MultiImageUploadField
          purpose="match_header"
          onChange={setHeaderAssetIds}
          initialItems={match.header_photos.map((p) => ({
            assetId: p.asset_id!,
            url: p.image_url,
          }))}
          className="mt-1"
        />
      </div>

      {save.isError && (
        <p className="text-xs text-destructive">
          Something went wrong. Please try again.
        </p>
      )}

      <div className="flex gap-2">
        <Button
          size="sm"
          disabled={!valid || save.isPending}
          onClick={() => save.mutate()}
        >
          {save.isPending ? 'Saving…' : 'Save'}
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
