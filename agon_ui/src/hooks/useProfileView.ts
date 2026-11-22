import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import type { LadderPlacement } from '@/lib/rating'

type UserProfile = components['schemas']['UserProfile']

export interface ProfileView {
  /** The public profile — name, image, per-sport stats, published ratings. */
  profile: UserProfile
  /**
   * Ladders this viewer is still placing on, from `GET /users/me`'s private
   * half. Present only for the viewer's own profile: nothing exposes anybody
   * else's placements, so this is `undefined` whenever `userId` was given.
   *
   * That makes "no placements" and "not my profile" the same value on
   * purpose. Both mean the same thing to a caller — there is no placements
   * chip to show — and keeping them distinguishable would only invite a
   * component to reveal the difference.
   */
  placements?: LadderPlacement[]
}

/**
 * One user's profile: the viewer's own (`GET /users/me`) when `userId` is
 * undefined, somebody else's (`GET /users/{user_id}`) when it isn't.
 *
 * Shared by the profile page, a sport's stats page and the follower lists
 * under the single `['profile', <id>]` cache key, so moving between them
 * doesn't refetch. Also the reason this is a hook rather than three copies of
 * the same `useQuery`: they all key the same entry, so they have to agree on
 * its shape, and the copy that forgot `rating_placements` would silently
 * decide whether the others could see them too.
 */
export function useProfileView(userId?: string): UseQueryResult<ProfileView> {
  return useQuery({
    queryKey: ['profile', userId ?? 'me'],
    queryFn: async (): Promise<ProfileView> => {
      if (userId) {
        const { data, error } = await fetchClient.GET('/users/{user_id}', {
          params: { path: { user_id: userId } },
        })
        if (error || !data) throw new Error('Failed to load profile')
        return { profile: data }
      }
      const { data, error } = await fetchClient.GET('/users/me')
      if (error || !data) throw new Error('Failed to load profile')
      return { profile: data.profile, placements: data.rating_placements }
    },
  })
}
