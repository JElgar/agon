import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type UserProfile = components['schemas']['UserProfile']

/**
 * The signed-in user's Agon id (not the auth `sub`). Resolved from `/users/me`
 * and cached under the shared `['users-me']` key. The cache holds the full
 * `UserProfile` (the same shape `LogMatchPage` and other consumers of this key
 * expect) — deriving the id via `select` keeps the cached value's shape
 * consistent across every reader, so a bare-id cache entry can't leak into a
 * consumer that expects the whole profile. Returns `undefined` until it loads
 * or if there's no profile yet.
 */
export function useCurrentUserId(): string | undefined {
  const { data } = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<UserProfile | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile ?? null
    },
    select: (profile) => profile?.id ?? null,
    staleTime: 5 * 60 * 1000,
  })
  return data ?? undefined
}
