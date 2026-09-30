import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { DateTimePicker } from '@/components/ui/date-time-picker'
import { isoToDateTimeLocal } from '@/lib/datetime'
import { MultiImageUploadField } from '@/components/agon/MultiImageUploadField'
import { LocationField, type LocationValue } from '@/components/agon/LocationField'
import { SIDE_COLOURS, defaultSideColour } from '@/lib/sideColours'
import { cn } from '@/lib/utils'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type UpdateMatchSideNameInput = components['schemas']['UpdateMatchSideNameInput']
type UpdateMatchSideColourInput = components['schemas']['UpdateMatchSideColourInput']

/** Whether a side can be given its own custom name/colour: an ad-hoc side (no
 *  team) always can; a team-linked side only when another side shares that
 *  same team (otherwise the team's own identity is the source of truth) —
 *  mirrors the server's create/update validation for both `name` and
 *  `colour`. */
function isRenameable(side: MatchSide, sides: MatchSide[]): boolean {
  if (!side.team_id) return true
  return sides.some((other) => other.id !== side.id && other.team_id === side.team_id)
}

/**
 * Inline editor for a match's metadata — name, description, start time,
 * header photos, and side names — shown in place of the details card when a
 * match admin taps "Edit". Saves via `PATCH /matches/{id}` (only the changed
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

  const [location, setLocation] = useState<LocationValue | null>(
    match.location
      ? {
          text: match.location.text,
          latitude: match.location.latitude,
          longitude: match.location.longitude,
          place_id: match.location.place_id,
        }
      : null,
  )

  const existingHeaderAssetIds = match.header_photos.map((p) => p.asset_id!)
  const [headerAssetIds, setHeaderAssetIds] = useState<string[]>(existingHeaderAssetIds)

  // Seeded from each side's currently-*displayed* name (server-resolved —
  // could be a custom name, a team's name, or a computed fallback). Editing
  // and saving sets that side's custom name explicitly; clearing the field
  // back to empty reverts to whatever the server would otherwise resolve.
  const [sideNames, setSideNames] = useState<Record<string, string>>(
    Object.fromEntries(match.sides.map((s) => [s.id, s.name ?? ''])),
  )

  // Seeded from each side's current colour. A renameable ad-hoc side (no
  // team) that has none yet — only possible for a match created before this
  // field existed — is seeded with a real default rather than left blank,
  // since the server requires one for a team-less side; a renameable derby
  // side (team shared with another side) is left unset if it has none,
  // since a colour there is optional. Defaults are picked one side at a
  // time so two sides both missing a colour don't default to the same one.
  const [sideColours, setSideColours] = useState<Record<string, string | undefined>>(() => {
    const seeded: Record<string, string | undefined> = {}
    for (const s of match.sides) {
      seeded[s.id] =
        s.colour ??
        (isRenameable(s, match.sides) && !s.team_id
          ? defaultSideColour(...Object.values(seeded))
          : undefined)
    }
    return seeded
  })

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
      if (JSON.stringify(location) !== JSON.stringify(match.location ?? null)) {
        if (location) body.location = location
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

      const sideColourUpdates: UpdateMatchSideColourInput[] = []
      for (const side of match.sides) {
        if (!isRenameable(side, match.sides)) continue
        const original = side.colour ?? undefined
        const next = sideColours[side.id]
        if (next === original) continue
        // Omitting `colour` clears it (only valid for a derby side — a
        // team-less one is never left unset, see the picker below).
        sideColourUpdates.push(next ? { side_id: side.id, colour: next } : { side_id: side.id })
      }
      if (sideColourUpdates.length > 0) body.side_colours = sideColourUpdates

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
        <DateTimePicker
          id="match-starts"
          value={startsAt}
          onChange={setStartsAt}
          className="mt-1"
        />
        {timeError && (
          <p className="mt-1 text-xs text-destructive">{timeError}</p>
        )}
      </div>

      <div>
        <Label htmlFor="match-location" className="text-xs text-muted-foreground">
          Where
        </Label>
        <LocationField
          id="match-location"
          value={location}
          onChange={setLocation}
          className="mt-1"
        />
      </div>

      <div className="flex flex-col gap-3">
        {match.sides.map((side, i) => {
          const renameable = isRenameable(side, match.sides)
          // A derby side (team shared with another side) can clear its
          // colour back to unset; a team-less side can't — the server
          // always requires one there.
          const canClearColour = renameable && !!side.team_id
          const label =
            i === 0 ? 'Side A' : i === 1 ? 'Side B' : `Side ${i + 1}`
          return (
            <div key={side.id}>
              <Label
                htmlFor={`side-name-${side.id}`}
                className="text-xs text-muted-foreground"
              >
                {label} name
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
              {renameable && (
                <div className="mt-2">
                  <Label className="text-xs text-muted-foreground">{label} colour</Label>
                  <div
                    className="mt-1 flex items-center gap-1.5"
                    role="radiogroup"
                    aria-label={`${label} colour`}
                  >
                    {SIDE_COLOURS.map((c) => {
                      // Greyed out when another side already has this colour
                      // — the server rejects two sides sharing one.
                      const disabled = match.sides.some(
                        (other) => other.id !== side.id && sideColours[other.id] === c.hex,
                      )
                      return (
                        <button
                          key={c.hex}
                          type="button"
                          role="radio"
                          aria-checked={sideColours[side.id] === c.hex}
                          aria-label={c.label}
                          disabled={disabled}
                          title={disabled ? 'Already used by another side' : undefined}
                          onClick={() =>
                            setSideColours((prev) => ({
                              ...prev,
                              [side.id]:
                                canClearColour && prev[side.id] === c.hex ? undefined : c.hex,
                            }))
                          }
                          className={cn(
                            'size-6 shrink-0 rounded-full border transition-shadow',
                            disabled
                              ? 'cursor-not-allowed opacity-25'
                              : sideColours[side.id] === c.hex
                                ? 'ring-2 ring-primary ring-offset-1 ring-offset-card'
                                : 'border-border/60',
                          )}
                          style={{ backgroundColor: c.hex }}
                        />
                      )
                    })}
                  </div>
                </div>
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
