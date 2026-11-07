import type { components } from '@/types/api'
import { myPendingInvitation } from '@/lib/members'

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
export function openJoinOptions(match: Match): {
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

/** Whether `OpenJoinAction` would render anything for this viewer — lets
 * `MatchDetailPage` decide, among several possible primary actions, whether
 * this one applies, without duplicating the eligibility check. */
export function openJoinEligible(match: Match, currentUserId?: string): boolean {
  const { sides, allowUnassigned } = openJoinOptions(match)
  const eligible = sides.length > 0 || allowUnassigned
  return eligible && !match.viewer_role && !myPendingInvitation(match, currentUserId)
}
