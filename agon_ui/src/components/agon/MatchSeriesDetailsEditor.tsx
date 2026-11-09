import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { MultiImageUploadField } from '@/components/agon/MultiImageUploadField'
import { LocationField, type LocationValue } from '@/components/agon/LocationField'
import { listFutureSeriesOccurrences } from '@/lib/matchSeries'

type Match = components['schemas']['Match']

/** "HH:mm" from an ISO instant, for the native `<input type="time">`. */
function isoToTime(iso: string): string {
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/**
 * Editor for the fields that describe a recurring series as a whole — name,
 * description, kickoff *time* (not date — each occurrence keeps its own),
 * location, and header photos — on `/matches/:id/edit` when the viewer picks
 * "the whole series" from `EditMatchDetailsFlow`'s choice.
 *
 * Deliberately a smaller field set than `MatchDetailsEditor`: a specific
 * date and side names/colours describe *this* occurrence, not the series,
 * so they're left out here entirely rather than editable-but-silently-only-
 * local, which is what made the single combined editor confusing enough
 * that this split exists.
 *
 * Saves this match (via `PATCH /matches/{id}`) then, from the same submit,
 * every future occurrence in the series (`GET
 * /match-series/{series_id}/occurrences`) — the same fields, plus the new
 * time of day applied to each occurrence's own date rather than overwriting
 * it outright.
 */
export function MatchSeriesDetailsEditor({
  match,
  onDone,
  onBack,
}: {
  match: Match
  onDone: () => void
  onBack: () => void
}) {
  const queryClient = useQueryClient()
  const [name, setName] = useState(match.name)
  const [description, setDescription] = useState(match.description)
  const [time, setTime] = useState(isoToTime(match.starts_at))

  const [location, setLocation] = useState<LocationValue | null>(
    match.location
      ? {
          text: match.location.text,
          latitude: match.location.latitude,
          longitude: match.location.longitude,
          place_id: match.location.place_id,
          address: match.location.address,
        }
      : null,
  )

  const existingHeaderAssetIds = match.header_photos.map((p) => p.asset_id!)
  const [headerAssetIds, setHeaderAssetIds] = useState<string[]>(existingHeaderAssetIds)

  const nameError = name.trim().length === 0 ? 'A match needs a name' : null
  const timeError = !/^\d{2}:\d{2}$/.test(time) ? 'Pick a valid time' : null
  const valid = !nameError && !timeError

  const save = useMutation({
    mutationFn: async () => {
      const body: components['schemas']['UpdateMatchInput'] = {}
      if (name.trim() !== match.name) body.name = name.trim()
      if (description !== match.description) body.description = description
      if (JSON.stringify(headerAssetIds) !== JSON.stringify(existingHeaderAssetIds)) {
        body.header_photo_asset_ids = headerAssetIds
      }
      if (JSON.stringify(location) !== JSON.stringify(match.location ?? null)) {
        if (location) body.location = location
      }

      const [hours, minutes] = time.split(':').map(Number)
      const timeChanged = time !== isoToTime(match.starts_at)
      if (timeChanged) {
        const thisDate = new Date(match.starts_at)
        thisDate.setHours(hours, minutes, 0, 0)
        body.starts_at = thisDate.toISOString()
      }

      if (Object.keys(body).length === 0) return // nothing changed

      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (error) throw new Error('Failed to save changes')

      const future = await listFutureSeriesOccurrences(match.series_id!, match.starts_at)
      const results = await Promise.all(
        future.map(async (occurrence) => {
          const occBody: components['schemas']['UpdateMatchInput'] = { ...body }
          if (timeChanged) {
            const occDate = new Date(occurrence.starts_at)
            occDate.setHours(hours, minutes, 0, 0)
            occBody.starts_at = occDate.toISOString()
          }
          if (Object.keys(occBody).length === 0) return { error: null }
          return fetchClient.PATCH('/matches/{match_id}', {
            params: { path: { match_id: occurrence.match_id } },
            body: occBody,
          })
        }),
      )
      if (results.some((r) => r.error)) {
        throw new Error('Saved this match, but some future matches in the series failed to update')
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['match-series', match.series_id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      onDone()
    },
  })

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <button
        type="button"
        onClick={onBack}
        className="-mb-1 self-start text-xs font-semibold text-muted-foreground hover:text-foreground"
      >
        ‹ Just this match, or the whole series?
      </button>

      <p className="text-xs text-muted-foreground">
        Editing the whole series. These changes apply to this and every future match — the date of
        each future match stays the same, only its time of day moves with this one.
      </p>

      <div>
        <Label htmlFor="series-match-name" className="text-xs text-muted-foreground">
          Match name
        </Label>
        <Input
          id="series-match-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="mt-1"
          autoFocus
        />
        {nameError && <p className="mt-1 text-xs text-destructive">{nameError}</p>}
      </div>

      <div>
        <Label htmlFor="series-match-description" className="text-xs text-muted-foreground">
          Description
        </Label>
        <Input
          id="series-match-description"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Optional"
          className="mt-1"
        />
      </div>

      <div>
        <Label htmlFor="series-match-time" className="text-xs text-muted-foreground">
          Kickoff time
        </Label>
        <Input
          id="series-match-time"
          type="time"
          value={time}
          onChange={(e) => setTime(e.target.value)}
          className="mt-1 w-[110px]"
        />
        {timeError && <p className="mt-1 text-xs text-destructive">{timeError}</p>}
      </div>

      <div>
        <Label htmlFor="series-match-location" className="text-xs text-muted-foreground">
          Where
        </Label>
        <LocationField
          id="series-match-location"
          value={location}
          onChange={setLocation}
          className="mt-1"
        />
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
          {save.error instanceof Error ? save.error.message : 'Something went wrong. Please try again.'}
        </p>
      )}

      <div className="flex gap-2">
        <Button size="sm" disabled={!valid || save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? 'Saving…' : 'Save to series'}
        </Button>
        <Button size="sm" variant="outline" disabled={save.isPending} onClick={onDone}>
          Cancel
        </Button>
      </div>
    </div>
  )
}
