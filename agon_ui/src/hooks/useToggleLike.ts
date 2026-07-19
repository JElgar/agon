import { useMutation, useQueryClient } from '@tanstack/react-query'
import type { InfiniteData } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type Match = components['schemas']['Match']
type FeedPage = components['schemas']['FeedPage']

/**
 * Apply `i_liked` + `like_count` to whichever cached match(es) match `matchId`,
 * across the shapes a match is cached in: a single `Match`, a `FeedPage`
 * infinite query, and the `Match[]` profile-activity lists. Anything else is
 * returned untouched, so this is safe to run over every cache entry.
 */
function patchLiked(matchId: string, liked: boolean) {
  const applyToMatch = <T extends Match>(m: T): T => {
    if (m.id !== matchId) return m
    // Guard the count so a double-fire can't drift it: only move it when the
    // liked state actually flips relative to what's cached.
    if (m.social.i_liked === liked) return m
    return {
      ...m,
      social: {
        ...m.social,
        i_liked: liked,
        like_count: Math.max(0, m.social.like_count + (liked ? 1 : -1)),
      },
    }
  }

  return (data: unknown): unknown => {
    if (!data || typeof data !== 'object') return data
    // Single match.
    if ('id' in data && 'social' in data) {
      return applyToMatch(data as Match)
    }
    // Feed: an infinite query of FeedPage. Feed items extend Match.
    if ('pages' in data) {
      const inf = data as InfiniteData<FeedPage>
      return {
        ...inf,
        pages: inf.pages.map((page) => ({
          ...page,
          items: page.items.map((item) => applyToMatch(item)),
        })),
      }
    }
    // Profile activity: a bare array of matches.
    if (Array.isArray(data)) {
      return (data as Match[]).map((m) => applyToMatch(m))
    }
    return data
  }
}

/**
 * Like/unlike a match with an optimistic toggle. `POST /likes` and
 * `DELETE /likes` are idempotent (both 204), so the mutation just picks the verb
 * from the desired next state. Optimistically patches every cache the match
 * appears in (detail, feed, profile activity) so the flame + count update the
 * instant the button is pressed, then reconciles on settle.
 */
export function useToggleLike(match: Pick<Match, 'id' | 'social'>) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (nextLiked: boolean) => {
      const { error } = nextLiked
        ? await fetchClient.POST('/matches/{match_id}/likes', {
            params: { path: { match_id: match.id } },
          })
        : await fetchClient.DELETE('/matches/{match_id}/likes', {
            params: { path: { match_id: match.id } },
          })
      if (error) throw new Error('Failed to update like')
    },
    onMutate: async (nextLiked) => {
      // Patch every query in the cache — patchLiked no-ops on non-match data.
      const patch = patchLiked(match.id, nextLiked)
      const snapshot = queryClient.getQueriesData({})
      queryClient.setQueriesData({}, patch)
      return { snapshot }
    },
    onError: (_err, _nextLiked, context) => {
      // Restore every snapshot we took, rolling back the optimistic patch.
      context?.snapshot.forEach(([key, data]) => {
        queryClient.setQueryData(key, data)
      })
    },
    onSettled: () => {
      // The like/comment counts live on the match; refresh the detail view. The
      // feed/profile lists reconcile on their own next fetch — the optimistic
      // patch already keeps them visually correct.
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    },
  })
}
