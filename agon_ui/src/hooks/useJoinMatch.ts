import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { RosterConflict } from '@/lib/waitlist'

/** Self-serve join — team-roster self-join (`side_id`, no token) or a
 *  join-link token — shared by every place that calls `POST
 *  /matches/:match_id/join` directly (as opposed to accepting a named
 *  invitation, which goes through `respondToInvitation` instead): the
 *  scheduled-match invite screen's primary action and `TeamJoinBanner`
 *  (the live/finished match view's own team-self-join card). */
export function useJoinMatch(matchId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: {
      token?: string
      side_id?: string
    }): Promise<'joined' | RosterConflict> => {
      const { error, response } = await fetchClient.POST('/matches/{match_id}/join', {
        params: { path: { match_id: matchId } },
        body,
      })
      if (response.status === 409) return error as RosterConflict
      if (error) throw new Error('Failed to join')
      return 'joined'
    },
    onSuccess: (result) => {
      if (result !== 'joined') return
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })
}
