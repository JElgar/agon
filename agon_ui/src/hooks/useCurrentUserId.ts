import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'

/**
 * The signed-in user's Agon id (not the auth `sub`). Resolved once from
 * `/users/me` and cached under a shared query key, so every consumer (feed card,
 * match detail, log-match) reads the same value. Returns `undefined` until it
 * loads or if there's no profile yet.
 */
export function useCurrentUserId(): string | undefined {
  const { data } = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<string | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile.id ?? null
    },
    staleTime: 5 * 60 * 1000,
  })
  return data ?? undefined
}
