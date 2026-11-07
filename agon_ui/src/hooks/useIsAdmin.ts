import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'

/**
 * Whether the signed-in account has admin-panel access. A separate query
 * (not `useCurrentUserId`'s `['users-me']` cache) because that hook caches
 * only the already-unwrapped `profile`, not `is_admin` — which lives on the
 * outer `/users/me` response alongside `email`.
 *
 * This only gates which UI renders; the server enforces the real check on
 * every `/admin/...` request regardless of what this returns.
 */
export function useIsAdmin(): boolean {
  const { data } = useQuery({
    queryKey: ['users-me-is-admin'],
    queryFn: async (): Promise<boolean> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.is_admin ?? false
    },
    staleTime: 5 * 60 * 1000,
  })
  return data ?? false
}
