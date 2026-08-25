import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { Button, type ButtonProps } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface TeamFollowButtonProps
  extends Omit<ButtonProps, 'onClick' | 'variant' | 'children'> {
  /** The team to follow/unfollow. */
  teamId: string
  /** Whether the viewer already follows this team (the button's initial state). */
  isFollowing: boolean
  /** Notified after a successful toggle with the new follow state. */
  onToggled?: (following: boolean) => void
}

/**
 * Follow/unfollow toggle for a single team — the team-side counterpart of
 * `FollowButton`. Owns the `POST`/`DELETE /teams/{id}/follow` mutation and
 * optimistically flips its own label, reverting if the request fails. On
 * success it invalidates the team's own query (its `follower_count` changed)
 * and `['my-teams']` (a followed team isn't necessarily one you're a member
 * of, but the list still reads `is_followed_by_me` per row).
 */
export function TeamFollowButton({
  teamId,
  isFollowing,
  onToggled,
  className,
  disabled,
  ...props
}: TeamFollowButtonProps) {
  const queryClient = useQueryClient()
  // Local optimistic mirror of the prop; re-sync if the parent's value changes
  // (e.g. the query refetches) so we never drift from the server's truth.
  const [following, setFollowing] = useState(isFollowing)
  useEffect(() => setFollowing(isFollowing), [isFollowing])

  const mutation = useMutation({
    mutationFn: async (next: boolean) => {
      const options = { params: { path: { team_id: teamId } } }
      const { error } = next
        ? await fetchClient.POST('/teams/{team_id}/follow', options)
        : await fetchClient.DELETE('/teams/{team_id}/follow', options)
      if (error) throw new Error('Failed to update follow')
    },
    onMutate: (next: boolean) => {
      const previous = following
      setFollowing(next)
      return { previous }
    },
    onError: (_err, _next, context) => {
      // Revert the optimistic flip.
      if (context) setFollowing(context.previous)
    },
    onSuccess: (_data, next) => {
      queryClient.invalidateQueries({ queryKey: ['team', teamId] })
      queryClient.invalidateQueries({ queryKey: ['my-teams'] })
      onToggled?.(next)
    },
  })

  return (
    <Button
      variant={following ? 'outline' : 'default'}
      disabled={disabled || mutation.isPending}
      onClick={() => mutation.mutate(!following)}
      className={cn(className)}
      {...props}
    >
      {following ? 'Following' : 'Follow'}
    </Button>
  )
}
