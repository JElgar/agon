import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Clock } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardTitle } from '@/components/ui/card'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

function sideName(side: MatchSide | undefined, fallback: string): string {
  return sideDisplayName(side) ?? fallback
}

/**
 * Who's queued for a spot on this match/side that wasn't free when they
 * tried to join or accept an invite onto it — `GET /matches/:id/waitlist`,
 * longest-waiting first. Renders nothing while empty; a match with no one
 * waiting shouldn't carry an always-there "no one waiting" placeholder.
 *
 * `canManage` gates "Move in" — admin-only, and only works once a spot is
 * actually free (the same typed conflict `join` returns). Removing an entry
 * ("Leave"/"Remove") is open to the waiting player themselves or a match
 * admin, same split the server enforces.
 */
export function WaitlistSection({
  match,
  currentUserId,
  canManage,
}: {
  match: Match
  currentUserId?: string
  canManage: boolean
}) {
  const queryClient = useQueryClient()
  const waitlistKey = ['waitlist', match.id]

  const query = useQuery({
    queryKey: waitlistKey,
    queryFn: async (): Promise<components['schemas']['WaitlistEntry'][]> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/waitlist', {
        params: { path: { match_id: match.id } },
      })
      if (error || !data) throw new Error('Failed to load the waitlist')
      return data
    },
  })

  const moveIn = useMutation({
    mutationFn: async (userId: string) => {
      const { error } = await fetchClient.POST(
        '/matches/{match_id}/waitlist/{user_id}/move-in',
        { params: { path: { match_id: match.id, user_id: userId } } },
      )
      if (error) throw new Error('Failed to move that player in')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: waitlistKey })
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })

  const leave = useMutation({
    mutationFn: async (userId: string) => {
      const { error } = await fetchClient.DELETE('/matches/{match_id}/waitlist/{user_id}', {
        params: { path: { match_id: match.id, user_id: userId } },
      })
      if (error) throw new Error('Failed to remove that waitlist entry')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: waitlistKey })
    },
  })

  const entries = query.data ?? []
  if (entries.length === 0) return null

  return (
    <Card>
      <CardContent className="p-4">
        <div className="mb-3 flex items-center gap-2">
          <Clock className="size-4 text-muted-foreground" />
          <CardTitle className="text-sm">Waiting list</CardTitle>
        </div>
        <div className="space-y-2">
          {entries.map((entry) => {
            const side = match.sides.find((s) => s.id === entry.side_id)
            const isMe = entry.user_id === currentUserId
            const canRemove = isMe || canManage
            return (
              <div
                key={entry.user_id}
                className="flex items-center justify-between gap-2 rounded-xl bg-accent/60 px-2.5 py-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  <Avatar name={entry.name} imageUrl={entry.avatar_url} size="sm" />
                  <div className="min-w-0">
                    <span className="block truncate text-sm font-medium">
                      {entry.name}
                      {isMe && ' (you)'}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      #{entry.position} · {sideName(side, 'Unassigned')}
                    </span>
                  </div>
                </div>
                <div className="flex shrink-0 gap-1.5">
                  {canManage && (
                    <Button
                      size="sm"
                      shape="pill"
                      disabled={moveIn.isPending}
                      onClick={() => moveIn.mutate(entry.user_id)}
                    >
                      Move in
                    </Button>
                  )}
                  {canRemove && (
                    <Button
                      size="sm"
                      shape="pill"
                      variant="outline"
                      disabled={leave.isPending}
                      onClick={() => leave.mutate(entry.user_id)}
                    >
                      {isMe ? 'Leave' : 'Remove'}
                    </Button>
                  )}
                </div>
              </div>
            )
          })}
        </div>
        {moveIn.isError && (
          <p className="mt-2 text-xs text-destructive">
            Couldn't move that player in — check there's actually a free spot.
          </p>
        )}
        {leave.isError && (
          <p className="mt-2 text-xs text-destructive">Something went wrong. Try again.</p>
        )}
      </CardContent>
    </Card>
  )
}
