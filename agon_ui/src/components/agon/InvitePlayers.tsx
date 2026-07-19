import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  PlayerSideEditor,
  type TaggedPlayer,
} from '@/components/agon/PlayerSideEditor'

type Match = components['schemas']['Match']

function sideLabel(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/**
 * "Invite players" panel on the match detail page (participants only). Collects
 * people to invite via the shared `PlayerSideEditor` — registered users or
 * typed-in guests — plus which side they're joining, then POSTs them to
 * `/matches/{id}/invitations`. The server creates each invitee's roster slot +
 * invitation, so on success we refresh the match (they appear as "invited") and
 * close the panel.
 *
 * Users already on the match are excluded from search so they can't be
 * double-invited.
 */
export function InvitePlayers({
  match,
  onDone,
}: {
  match: Match
  onDone: () => void
}) {
  const queryClient = useQueryClient()
  const [players, setPlayers] = useState<TaggedPlayer[]>([])
  // Default to the first side; the invite endpoint takes one side per call.
  const [sideId, setSideId] = useState<string>(match.sides[0]?.id ?? '')

  // Everyone already linked to this match, so we don't offer to invite them again.
  const existingUserIds = match.players.flatMap((p) =>
    p.member.type === 'User' ? [p.member.user_id] : [],
  )

  const invite = useMutation({
    mutationFn: async () => {
      const invited_user_ids = players
        .filter((p) => p.kind === 'user')
        .map((p) => (p as Extract<TaggedPlayer, { kind: 'user' }>).id)
      const invited_external_names = players
        .filter((p) => p.kind === 'external')
        .map((p) => p.name)
      if (invited_user_ids.length === 0 && invited_external_names.length === 0)
        return

      const { error } = await fetchClient.POST(
        '/matches/{match_id}/invitations',
        {
          params: { path: { match_id: match.id } },
          body: {
            invited_user_ids,
            invited_external_names,
            side_id: sideId || undefined,
          },
        },
      )
      if (error) throw new Error('Failed to send invitations')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      onDone()
    },
  })

  const hasPeople = players.length > 0

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <p className="text-sm font-medium">Invite players</p>

      <PlayerSideEditor
        title="People to invite"
        searchPlaceholder="Search people, or type a guest's name…"
        players={players}
        onChange={setPlayers}
        excludeUserIds={existingUserIds}
      />

      <div>
        <Label htmlFor="invite-side" className="text-xs text-muted-foreground">
          Which side?
        </Label>
        <select
          id="invite-side"
          value={sideId}
          onChange={(e) => setSideId(e.target.value)}
          className="mt-1 h-9 w-full rounded-md border bg-background px-3 text-sm"
        >
          {match.sides.map((side, i) => (
            <option key={side.id} value={side.id}>
              {sideLabel(match, i, `Side ${i + 1}`)}
            </option>
          ))}
        </select>
      </div>

      {invite.isError && (
        <p className="text-xs text-destructive">
          Something went wrong. Please try again.
        </p>
      )}

      <div className="flex gap-2">
        <Button
          size="sm"
          disabled={!hasPeople || invite.isPending}
          onClick={() => invite.mutate()}
        >
          {invite.isPending ? 'Inviting…' : 'Send invites'}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={invite.isPending}
          onClick={onDone}
        >
          Cancel
        </Button>
      </div>
    </div>
  )
}
