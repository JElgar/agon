import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'

/** Promote/demote a player between `player` and `admin` — shared by every
 *  place a match admin can manage roles (the scheduled-match "who's going"
 *  list and the live/finished Players tabs' roster rows), open at any match
 *  stage since admin rights are about managing the match, not about whether
 *  it's been scored yet. */
export function useSetMatchPlayerRole(matchId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ playerId, role }: { playerId: string; role: 'admin' | 'player' }) => {
      const { error } = await fetchClient.POST('/matches/{match_id}/players/{player_id}/role', {
        params: { path: { match_id: matchId, player_id: playerId } },
        body: { role },
      })
      if (error) throw new Error('Failed to update role')
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['match', matchId] }),
  })
}
