import { useInfiniteQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useNavigate } from 'react-router-dom'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type FeedPageData = components['schemas']['FeedPage']

/** Page size for the feed. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

/**
 * The feed: the viewer's fanned-out matches, newest first, with cursor-based
 * infinite scroll. Reads `GET /feed` via the authenticated fetch client, so it
 * relies on a signed-in session (the app shell only mounts this once auth +
 * profile gates pass).
 */
export function FeedPage() {
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const query = useInfiniteQuery({
    queryKey: ['feed'],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<FeedPageData> => {
      const { data, error } = await fetchClient.GET('/feed', {
        params: {
          query: { cursor: pageParam, limit: PAGE_SIZE },
        },
      })
      if (error || !data) throw new Error('Failed to load feed')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor,
  })

  if (query.isLoading) {
    return <FeedSkeleton />
  }

  if (query.isError) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load your feed.</p>
        <Button variant="outline" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const items = (query.data?.pages ?? []).flatMap((page) => page.items)

  if (items.length === 0) {
    return (
      <div className="py-16 text-center">
        <h2 className="mb-1 text-lg font-medium">Your feed is empty</h2>
        <p className="mb-4 text-sm text-muted-foreground">
          Matches you play and the people you follow show up here.
        </p>
        <Button onClick={() => navigate('/matches/new')}>Log a match</Button>
      </div>
    )
  }

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-3">
      {items.map((item) => (
        <MatchCard
          key={item.id}
          match={item}
          currentUserId={currentUserId}
          onOpen={() => navigate(`/matches/${item.id}`)}
        />
      ))}

      {query.hasNextPage && (
        <Button
          variant="outline"
          className="mt-2"
          disabled={query.isFetchingNextPage}
          onClick={() => query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more'}
        </Button>
      )}
    </div>
  )
}

/** Placeholder cards while the first page loads. */
function FeedSkeleton() {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-3">
      {Array.from({ length: 3 }).map((_, i) => (
        <div
          key={i}
          className="h-48 animate-pulse rounded-xl border bg-card"
          aria-hidden
        />
      ))}
    </div>
  )
}
