import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link2 } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { joinChoiceFor, sidesFor } from '@/lib/joinLink'
import { forgetJoinLink, getRememberedJoinLink } from '@/lib/joinLinkMemory'
import { sidePlayerCountLabel } from '@/lib/members'

type Match = components['schemas']['Match']
type JoinLinkPreview = components['schemas']['JoinLinkPreview']

/**
 * "Join with the link you opened earlier" banner for a non-participant who
 * previewed a join link (`/join/:token`) for this exact match — see
 * `JoinMatchPage` — but left via "View match" instead of joining right away.
 * The token was remembered against this match id (`lib/joinLinkMemory`), so
 * it's still usable here, mirroring how a real invitation stays actionable
 * on this page too (`InviteBanner`).
 *
 * Renders nothing when there's no remembered token, the viewer is already a
 * participant, or the link no longer resolves (revoked since it was seen) —
 * in the last case the stale token is forgotten so this stops re-checking it.
 */
export function JoinLinkBanner({ match }: { match: Match }) {
  const queryClient = useQueryClient()
  const [sideId, setSideId] = useState<string | undefined>(undefined)
  const token = match.viewer_role == null ? getRememberedJoinLink(match.id) : null

  const preview = useQuery({
    queryKey: ['join-link-by-token', token],
    enabled: !!token,
    retry: false,
    queryFn: async (): Promise<JoinLinkPreview> => {
      const { data, error } = await fetchClient.GET('/join-links/by-token/{token}', {
        params: { path: { token: token! } },
      })
      if (error || !data) throw new Error('join-link-not-found')
      return data
    },
  })

  useEffect(() => {
    if (token && preview.isError) forgetJoinLink(match.id)
  }, [token, preview.isError, match.id])

  const choice = token && preview.data ? joinChoiceFor(preview.data, match) : undefined
  const pickableSides = choice ? sidesFor(choice, match) : []
  const forcedSideId =
    choice?.allowedSideIds && choice.allowedSideIds.length === 1 ? choice.allowedSideIds[0] : undefined
  const needsPick = choice ? choice.allowedSideIds === null || choice.allowedSideIds.length > 1 : false
  const effectiveSideId = forcedSideId ?? sideId
  const canSubmit = !choice ? false : needsPick ? !!effectiveSideId || choice.allowUnassigned : true

  const join = useMutation({
    mutationFn: async (): Promise<'joined' | 'conflict'> => {
      const { error, response } = await fetchClient.POST('/matches/{match_id}/join', {
        params: { path: { match_id: match.id } },
        body: { token: token!, side_id: effectiveSideId },
      })
      if (response.status === 409) return 'conflict'
      if (error) throw new Error('Failed to join')
      return 'joined'
    },
    onSuccess: (result) => {
      if (result === 'conflict') return
      forgetJoinLink(match.id)
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })

  if (!token || !choice) return null

  return (
    <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
          <Link2 className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">You have a join link for this game</p>
          <p className="text-xs text-muted-foreground">
            Join directly with the link you opened earlier — no invite needed.
          </p>

          {needsPick && (
            <select
              value={sideId ?? ''}
              onChange={(e) => setSideId(e.target.value || undefined)}
              className="mt-2 h-9 w-full max-w-56 rounded-md border bg-background px-3 text-sm"
            >
              {choice.allowUnassigned ? (
                <option value="">Unassigned — pick a side later</option>
              ) : (
                <option value="" disabled>
                  Choose a side…
                </option>
              )}
              {pickableSides.map((side, i) => (
                <option key={side.id} value={side.id}>
                  {side.name?.trim() || `Side ${i + 1}`} · {sidePlayerCountLabel(side)}
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
