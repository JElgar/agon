import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'

/** Everyone the viewer follows, walked page by page (the API caps a page at 50). */
export function useViewerFollowing(userId: string | undefined) {
  return useQuery({
    queryKey: ['following-ids', userId],
    enabled: !!userId,
    queryFn: async () => {
      const ids = new Set<string>()
      let cursor: string | undefined
      for (let page = 0; page < 10; page++) {
        const { data, error } = await fetchClient.GET('/users/{user_id}/following', {
          params: { path: { user_id: userId! }, query: { limit: 50, cursor } },
        })
        if (error || !data) break
        data.items.forEach((u) => ids.add(u.id))
        cursor = data.next_cursor ?? undefined
        if (!cursor) break
      }
      return ids
    },
  })
}
