import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { sideDisplayName } from '@/lib/members'
import { useJoinMatch } from '@/hooks/useJoinMatch'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Sides open to an individual ringer right now, and whether joining one
 * requires the admin's approval — mirrors the backend's
 * `open_join_individual_scope`/`open_full_reason` (`agon_service/src/
 * main.rs`), since `Match` doesn't expose a single computed eligibility
 * field for this the way `viewer_team_join_side_ids` does for team
 * self-join. Doesn't replicate eligibility rules (`min_matches_played`) —
 * those are enforced server-side on the actual join/request call, same as
 * a full side being enforced there rather than hidden here. */
function openJoinOptions(match: Match): {
  sides: MatchSide[]
  allowUnassigned: boolean
  requiresApprovalFor: (sideId: string | undefined) => boolean
} {
  const scope = match.open_join
  if (!scope || scope.mode === 'per_side') {
    const sides = match.sides.filter((s) => s.open_join != null)
    return {
      sides,
      allowUnassigned: false,
      requiresApprovalFor: (sideId) =>
        sides.find((s) => s.id === sideId)?.open_join?.requires_approval ?? false,
    }
  }
  const audience = scope.policy?.audience
  if (audience !== 'individuals' && audience !== 'both') {
    return { sides: [], allowUnassigned: false, requiresApprovalFor: () => false }
  }
  return {
    sides: match.sides.filter((s) => s.open_join_enabled),
    allowUnassigned: match.allow_unassigned,
    requiresApprovalFor: () => scope.policy?.requires_approval ?? false,
  }
}

/**
 * "Join as a ringer" banner for a non-participant on a match open to
 * individuals (`Match.open_join`) — the open-join analogue of
 * `TeamJoinBanner`. Renders nothing once there's no eligible side (and no
 * unassigned option) to offer, or once the viewer is already playing.
 *
 * A side whose policy (or the match-wide one) requires approval submits a
 * `JoinRequest` instead of joining directly; `JoinRequestsCard` is where an
 * admin decides it.
 */
export function OpenJoinBanner({ match }: { match: Match }) {
  const queryClient = useQueryClient()
  const { sides, allowUnassigned, requiresApprovalFor } = openJoinOptions(match)
  const [sideId, setSideId] = useState<string | undefined>(sides[0]?.id)

  const join = useJoinMatch(match.id)
  const request = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/matches/{match_id}/join-requests', {
        params: { path: { match_id: match.id } },
        body: { side_id: sideId },
      })
      if (error) throw new Error('Failed to send the join request')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    },
  })

  if (match.viewer_role || (sides.length === 0 && !allowUnassigned)) return null

  const needsPick = sides.length > 0 && (sides.length > 1 || allowUnassigned)
  const effectiveSideId = sides.length === 1 && !allowUnassigned ? sides[0].id : sideId
  const requiresApproval = requiresApprovalFor(effectiveSideId)

  if (request.isSuccess) {
    return (
      <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
        <p className="text-sm font-medium">Request sent</p>
        <p className="text-xs text-muted-foreground">
          An admin needs to approve it before you're on the roster.
        </p>
      </div>
    )
  }

  const conflict = join.data && join.data !== 'joined' ? join.data : null

  return (
    <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
          <UserPlus className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">This match is open to ringers</p>
          <p className="text-xs text-muted-foreground">
            {requiresApproval
              ? 'Request to join — an admin will need to approve it.'
              : 'Join directly — no invite needed.'}
          </p>
          {needsPick && (
            <select
              className="mt-2 h-8 w-full rounded-md border bg-background px-2 text-sm"
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
          {conflict && <p className="mt-2 text-xs text-destructive">That side is full.</p>}
          {(join.isError || request.isError) && (
            <p className="mt-2 text-xs text-destructive">
              {((join.error ?? request.error) as Error).message}
            </p>
          )}
          <Button
            size="sm"
            className="mt-2"
            disabled={join.isPending || request.isPending}
            onClick={() => (requiresApproval ? request.mutate() : join.mutate({ side_id: effectiveSideId }))}
          >
            {requiresApproval ? 'Request to join' : 'Join'}
          </Button>
        </div>
      </div>
    </div>
  )
}
