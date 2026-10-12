import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type UserProfile = components['schemas']['UserProfile']

/**
 * The signed-in user's full profile (name, image, per-sport stats). Shares
 * the `['users-me']` cache entry with `useCurrentUserId` — react-query keys
 * on the query key, not the `select`, so both hooks read the same fetch.
 */
export function useCurrentUserProfile(): UserProfile | undefined {
  const { data } = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<UserProfile | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile ?? null
    },
    staleTime: 5 * 60 * 1000,
  })
  return data ?? undefined
}
