import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'

type Match = components['schemas']['Match']

/**
 * "Join this side directly" banner for a non-participant who's an accepted
 * member of a team linked to one (or, for an intra-squad match, more) of the
 * match's sides — `Match.viewer_team_join_side_ids`, server-computed (see
 * its doc comment) so this never has to re-derive eligibility from the
 * viewer's own team list itself. `null`/empty renders nothing, same
 * "banner disappears once it doesn't apply" posture as `InviteBanner`.
 *
 * One eligible side auto-assigns on click (mirrors a single-side join
 * link's own auto-assign). Two or more (both sides the same team) shows a
 * picker — the same choice a multi-side join link offers, including
 * "unassigned" when the match's own `allow_unassigned` allows it.
 */
export function TeamJoinBanner({ match }: { match: Match }) {
  const queryClient = useQueryClient()
  const [sideId, setSideId] = useState<string | undefined>(undefined)

  const eligibleSideIds = match.viewer_team_join_side_ids
  const pickableSides = match.sides.filter((s) => eligibleSideIds?.includes(s.id))
  const forcedSideId = eligibleSideIds?.length === 1 ? eligibleSideIds[0] : undefined
  const needsPick = (eligibleSideIds?.length ?? 0) > 1
  const effectiveSideId = forcedSideId ?? sideId
  const canSubmit = needsPick ? match.allow_unassigned || !!effectiveSideId : true

  const join = useMutation({
    mutationFn: async (): Promise<'joined' | 'conflict'> => {
      const { error, response } = await fetchClient.POST('/matches/{match_id}/join', {
        params: { path: { match_id: match.id } },
        body: { side_id: effectiveSideId },
      })
      if (response.status === 409) return 'conflict'
      if (error) throw new Error('Failed to join')
      return 'joined'
    },
    onSuccess: (result) => {
      if (result === 'conflict') return
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })

  if (!eligibleSideIds || eligibleSideIds.length === 0) return null

  return (
    <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
          <Users className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">
            {needsPick
              ? "You're on a team playing in this game"
              : `You're on ${pickableSides[0]?.name?.trim() || 'a side'} playing in this game`}
          </p>
          <p className="text-xs text-muted-foreground">
            {needsPick
              ? 'Pick a side to join it directly — no invite needed.'
              : 'Join it directly — no invite needed.'}
          </p>

          {needsPick && (
            <select
              value={sideId ?? ''}
              onChange={(e) => setSideId(e.target.value || undefined)}
              className="mt-2 h-9 w-full max-w-56 rounded-md border bg-background px-3 text-sm"
            >
              {match.allow_unassigned ? (
                <option value="">Unassigned — pick a side later</option>
              ) : (
                <option value="" disabled>
                  Choose a side…
                </option>
              )}
              {pickableSides.map((side, i) => (
                <option key={side.id} value={side.id}>
                  {side.name?.trim() || `Side ${i + 1}`}
                </option>
              ))}
            </select>
          )}

          {join.isError && (
            <p className="mt-2 text-xs text-destructive">Something went wrong. Try again.</p>
          )}
          {join.data === 'conflict' && (
            <p className="mt-2 text-xs text-destructive">
              This game (or side) is full, or you're already on the roster.
            </p>
          )}

          <div className="mt-3">
            <Button
              size="sm"
              disabled={join.isPending || !canSubmit}
              onClick={() => join.mutate()}
            >
              {join.isPending ? 'Joining…' : 'Join'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
