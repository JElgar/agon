import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { Button, type ButtonProps } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface FollowButtonProps
  extends Omit<ButtonProps, 'onClick' | 'variant' | 'children'> {
  /** The user to follow/unfollow. */
  userId: string
  /** Whether the viewer already follows this user (the button's initial state). */
  isFollowing: boolean
  /** Notified after a successful toggle with the new follow state. */
  onToggled?: (following: boolean) => void
}

/**
 * Follow/unfollow toggle for a single user. Owns the `POST`/`DELETE
 * /users/{id}/follow` mutation and optimistically flips its own label so the
 * click feels instant, reverting if the request fails. On success it
 * invalidates the target's profile query (and the viewer's own, whose
 * following-count changed) so any mounted `ProfileHeader` counts refresh.
 *
 * Reusable across the profile page, search results, follower/following lists
 * and the follow-back action in notifications — each just passes the row's
 * `userId` + `isFollowing` (from `UserProfile.is_followed_by_me`).
 */
export function FollowButton({
  userId,
  isFollowing,
  onToggled,
  className,
  disabled,
  ...props
}: FollowButtonProps) {
  const queryClient = useQueryClient()
  // Local optimistic mirror of the prop; re-sync if the parent's value changes
  // (e.g. the query refetches) so we never drift from the server's truth.
  const [following, setFollowing] = useState(isFollowing)
  useEffect(() => setFollowing(isFollowing), [isFollowing])

  const mutation = useMutation({
    mutationFn: async (next: boolean) => {
      const options = { params: { path: { user_id: userId } } }
      const { error } = next
        ? await fetchClient.POST('/users/{user_id}/follow', options)
        : await fetchClient.DELETE('/users/{user_id}/follow', options)
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
      queryClient.invalidateQueries({ queryKey: ['profile', userId] })
      queryClient.invalidateQueries({ queryKey: ['profile', 'me'] })
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
