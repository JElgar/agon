import { useQuery } from '@tanstack/react-query'
import { currentUserQuery } from './useCurrentUser'

/**
 * The signed-in user's Agon id (not the auth `sub`). Resolved from
 * `/users/me` and cached under the shared `['users-me']` key — see
 * `currentUserQuery` for why the id is derived with `select` instead of
 * being cached on its own. Returns `undefined` until it loads or if there's
 * no profile yet.
 */
export function useCurrentUserId(): string | undefined {
  const { data } = useQuery({
    ...currentUserQuery,
    select: (user) => user?.profile.id ?? null,
    staleTime: 5 * 60 * 1000,
  })
  return data ?? undefined
}
