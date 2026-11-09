import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { sideDisplayName } from '@/lib/members'
import { openJoinOptions } from '@/lib/openJoin'
import { useJoinMatch } from '@/hooks/useJoinMatch'
import { primaryActionClass } from '@/components/agon/football/FootballMatchView'
import { cn } from '@/lib/utils'

type Match = components['schemas']['Match']

/**
 * "Join as a ringer" control for the pinned action bar's primary slot —
 * James, 2026-10-23: no separate join banner, a viewer who can join for any
 * reason gets "Join" in the bar instead. The open-join analogue of
 * `TeamJoinBanner`'s eligibility, but rendered as this bar's primary
 * action rather than its own card.
 *
 * Only rendered once `openJoinEligible` (`@/lib/openJoin`) says so —
 * `MatchDetailPage` checks that before picking this as the primary action.
 *
 * A side whose policy (or the match-wide one) requires approval submits a
 * `JoinRequest` instead of joining directly; `JoinRequestsCard` is where an
 * admin decides it.
 */
export function OpenJoinAction({ match }: { match: Match }) {
  const queryClient = useQueryClient()
  const { sides, allowUnassigned, requiresApprovalFor } = openJoinOptions(match)
  const [sideId, setSideId] = useState<string | undefined>(sides[0]?.id)

  const join = useJoinMatch(match.id)
  const request = useMutation({
    mutationFn: async (requestSideId: string | undefined) => {
      const { error } = await fetchClient.POST('/matches/{match_id}/join-requests', {
        params: { path: { match_id: match.id } },
        body: { side_id: requestSideId },
      })
      if (error) throw new Error('Failed to send the join request')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    },
  })

  const needsPick = sides.length > 1 || (sides.length > 0 && allowUnassigned)
  const effectiveSideId = sides.length === 1 && !allowUnassigned ? sides[0].id : sideId
  const requiresApproval = requiresApprovalFor(effectiveSideId)

  if (request.isSuccess) {
    return (
      <div className={cn(primaryActionClass, 'pointer-events-none bg-muted text-muted-foreground')}>
        Request sent
      </div>
    )
  }

  const conflict = join.data && join.data !== 'joined' ? join.data : null
  const errorMessage = conflict
    ? 'That side is full'
    : ((join.error ?? request.error) as Error | null)?.message

  return (
    <div className="flex flex-1 flex-col gap-1">
      {errorMessage && <p className="text-center text-xs text-destructive">{errorMessage}</p>}
      {needsPick && (
        <select
          className="h-8 rounded-md border bg-background px-2 text-sm"
          value={sideId ?? ''}
          onChange={(e) => setSideId(e.target.value || undefined)}
        >
          {sides.map((side, i) => (
            <option key={side.id} value={side.id}>
              {sideDisplayName(side) ?? `Side ${i + 1}`}
            </option>
          ))}
          {allowUnassigned && <option value="">Unassigned</option>}
        </select>
      )}
      <button
        type="button"
        className={primaryActionClass}
        disabled={join.isPending || request.isPending}
        onClick={() =>
          requiresApproval ? request.mutate(effectiveSideId) : join.mutate({ side_id: effectiveSideId })
        }
      >
        {requiresApproval ? 'Request to join' : 'Join'}
      </button>
    </div>
  )
}
