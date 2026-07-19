import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { isoToDateTimeLocal } from '@/lib/datetime'

type Match = components['schemas']['Match']

/**
 * Inline editor for a match's metadata — name, description, and start time —
 * shown in place of the details card when a participant taps "Edit". Saves via
 * `PATCH /matches/{id}` (only the changed fields are sent) and, on success,
 * refreshes the match and feed then closes back to the read-only card.
 *
 * Roster and result are edited elsewhere (invite control, result editor), so
 * this deliberately covers just the descriptive fields.
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
