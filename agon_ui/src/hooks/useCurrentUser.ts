import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type User = components['schemas']['User']

/**
 * The one fetcher behind the `['users-me']` cache entry: `GET /users/me`'s
 * whole response, private half included.
 *
 * Shared as a descriptor rather than a hook because its readers want
 * different slices (the id, the profile, the placements) and different
 * staleness, and `select` gives them that off one fetch. What they must not
 * do is disagree about the cached *shape* — several call sites used to spell
 * out `queryFn: () => data?.profile` and cache a `UserProfile` here, which
 * worked only as long as nobody needed a field that lives outside `profile`.
 * `rating_placements` is the first one that does.
 */
export const currentUserQuery = {
  queryKey: ['users-me'] as const,
  queryFn: async (): Promise<User | null> => {
    const { data } = await fetchClient.GET('/users/me')
    return data ?? null
  },
}
