import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'

/**
 * Live unread-notification count, shared by every nav surface (desktop
 * sidebar, mobile header). Shares the query key the notifications page
 * invalidates, so acting on notifications updates it everywhere at once.
 */
export function useUnreadNotificationsCount() {
  return useQuery({
    queryKey: ['notifications-unread-count'],
    queryFn: async (): Promise<number> => {
      const { data } = await fetchClient.GET('/notifications/unread-count')
      return data?.unread_count ?? 0
    },
  })
}
