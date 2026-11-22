import { useQuery } from '@tanstack/react-query'
import type { components } from '@/types/api'
import { currentUserQuery } from './useCurrentUser'

type UserProfile = components['schemas']['UserProfile']

/**
 * The signed-in user's public profile (name, image, per-sport stats, published
 * ratings). Shares the `['users-me']` cache entry with `useCurrentUserId` and
 * every other reader of `currentUserQuery` — react-query keys on the query
 * key, not the `select`, so they all read the same fetch.
 */
export function useCurrentUserProfile(): UserProfile | undefined {
  const { data } = useQuery({
    ...currentUserQuery,
    select: (user) => user?.profile ?? null,
    staleTime: 5 * 60 * 1000,
  })
  return data ?? undefined
}
