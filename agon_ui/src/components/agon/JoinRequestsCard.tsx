import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Check, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Avatar } from '@/components/agon/Avatar'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']

/**
 * Pending join requests awaiting an admin's decision — only relevant once
 * `Match.open_join`'s policy has `requires_approval` set (see
 * `MatchOpenJoinEditor`); someone can still have a request sitting here from
 * before an admin turned approval off, so this doesn't gate on that flag.
 * Fetches `GET /matches/:id/join-requests` only while the viewer can manage
 * the match, since the endpoint is match-admin only.
 */
export function JoinRequestsCard({ match, canManage }: { match: Match; canManage: boolean }) {
  const queryClient = useQueryClient()
  const requests = useQuery({
    queryKey: ['match', match.id, 'join-requests'],
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/join-requests', {
        params: { path: { match_id: match.id } },
      })
      if (error) throw new Error('Failed to load join requests')
      return data
    },
    enabled: canManage,
  })

  const decide = useMutation({
    mutationFn: async ({ userId, approve }: { userId: string; approve: boolean }) => {
      const { error } = await fetchClient.POST(
        approve
          ? '/matches/{match_id}/join-requests/{user_id}/approve'
          : '/matches/{match_id}/join-requests/{user_id}/decline',
        { params: { path: { match_id: match.id, user_id: userId } } },
      )
      if (error) throw new Error(`Failed to ${approve ? 'approve' : 'decline'} the request`)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id, 'join-requests'] })
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    },
  })

  if (!canManage) return null
  const pending = (requests.data ?? []).filter((r) => r.status === 'pending')
  if (requests.isLoading || pending.length === 0) return null

  return (
    <div className="rounded-xl border bg-card p-4">
      <p className="mb-3 text-sm font-medium">Join requests</p>
      <div className="flex flex-col gap-2">
        {pending.map((request) => {
          const side = request.side_id ? match.sides.find((s) => s.id === request.side_id) : undefined
          const busy = decide.isPending && decide.variables?.userId === request.user_id
          return (
            <div key={request.user_id} className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <Avatar name={request.name} imageUrl={request.avatar_url} size="md" />
                <div>
                  <p className="text-sm">{request.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {side ? (sideDisplayName(side) ?? 'A side') : 'Unassigned'}
                  </p>
                </div>
              </div>
              <div className="flex gap-1">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 gap-1 px-2 text-xs"
                  disabled={busy}
                  onClick={() => decide.mutate({ userId: request.user_id, approve: true })}
                >
                  <Check className="size-3" /> Approve
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 gap-1 px-2 text-xs text-muted-foreground"
                  disabled={busy}
                  onClick={() => decide.mutate({ userId: request.user_id, approve: false })}
                >
                  <X className="size-3" /> Decline
                </Button>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
