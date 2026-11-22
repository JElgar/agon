import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { ratingLadderFor } from '@/lib/rating'
import type { MatchType } from '@/lib/sports'
import type { components } from '@/types/api'

export type RatingHistoryPoint = components['schemas']['RatingHistoryPoint']

/** Items per request — the endpoint's own cap (`page_limit`). */
const PAGE_SIZE = 50

/**
 * How many pages to walk before giving up, so one very long history can't
 * turn a card into an unbounded request loop. 250 rated matches in a single
 * sport is far beyond anything the app has, and the chart only draws the
 * recent end of it anyway.
 */
const MAX_PAGES = 5

/**
 * A player's rating over time on one sport's ladder, oldest first.
 *
 * `GET /users/:id/ratings/:ladder/history` is ascending-only and cursored,
 * which is the right shape for the collection (it is keyed on played order,
 * and the planned `RepairRatings` replay walks it forwards) but the wrong
 * shape for "show me the recent end": there is no way to seek to the last
 * page, so reaching it means walking there. This walks, capped at
 * `MAX_PAGES`.
 *
 * That is affordable because the walk is bounded by a player's rated matches
 * in **one sport**, which is tens, not thousands — and it collapses to a
 * single request for anybody under `PAGE_SIZE`, which today is everybody. If
 * that stops being true the fix belongs in the endpoint (a descending mode,
 * or a cursor for the last page), not in more paging here.
 *
 * Returns an empty list rather than an error for a sport with no ladder, a
 * rating still in placements, or a player who has never played it ranked —
 * the endpoint deliberately answers all three identically, because a
 * response that distinguished them would leak the fact that a number is
 * being withheld.
 */
export function useRatingHistory(userId: string | undefined, sport: MatchType) {
  const ladder = sport ? ratingLadderFor(sport) : null

  return useQuery({
    queryKey: ['rating-history', userId, ladder],
    enabled: !!userId && !!ladder,
    queryFn: async (): Promise<RatingHistoryPoint[]> => {
      const items: RatingHistoryPoint[] = []
      let cursor: string | undefined

      for (let page = 0; page < MAX_PAGES; page++) {
        const { data, error } = await fetchClient.GET(
          '/users/{user_id}/ratings/{ladder}/history',
          {
            params: {
              path: { user_id: userId!, ladder: ladder! },
              query: { cursor, limit: PAGE_SIZE },
            },
          },
        )
        if (error || !data) throw new Error('Failed to load rating history')
        items.push(...data.items)
        if (!data.next_cursor) break
        cursor = data.next_cursor
      }

      return items
    },
  })
}
