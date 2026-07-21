import { useEffect } from 'react'
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useNavigate } from 'react-router-dom'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { dayLabel } from '@/lib/datetime'
import {
  usePendingMatches,
  prunePendingMatches,
} from '@/hooks/usePendingMatches'

type FeedPageData = components['schemas']['FeedPage']
type FeedItem = components['schemas']['FeedItem']

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
  const queryClient = useQueryClient()

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

  // Matches the viewer just created, shown on top of the fetched feed until the
  // async fan-out lands them in `GET /feed` (see usePendingMatches).
  const pending = usePendingMatches()
  const serverItems = (query.data?.pages ?? []).flatMap((page) => page.items)

  // Once the server feed contains a pending match, drop it from the overlay so
  // it isn't rendered twice. Done in an effect — it mutates the query cache.
  const serverIdsKey = serverItems.map((i) => i.id).join(',')
  useEffect(() => {
    if (pending.length === 0) return
    const serverIds = new Set(serverItems.map((i) => i.id))
    prunePendingMatches(queryClient, serverIds)
    // serverIdsKey captures the set of ids; re-run only when it changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverIdsKey, pending.length, queryClient])

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

  // Merge the pending overlay ahead of the server feed, deduped by id (a match
  // that's in both — mid-reconciliation — renders once, from the server copy).
  const serverIds = new Set(serverItems.map((i) => i.id))
  const pendingItems: FeedItem[] = pending
    .filter((m) => !serverIds.has(m.id))
    .map((m) => ({ ...m, type: 'Match' }) as FeedItem)
  const items = [...pendingItems, ...serverItems]

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

  const sections = groupByDay(items)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-6">
      {sections.map((section) => (
        <div key={section.label} className="flex flex-col gap-3">
          <h2 className="font-serif text-lg italic text-muted-foreground">
            {section.label}
          </h2>
          {section.items.map((item) => (
            <MatchCard
              key={item.id}
              match={item}
              currentUserId={currentUserId}
              onOpen={() => navigate(`/matches/${item.id}`)}
            />
          ))}
        </div>
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

/**
 * Bucket feed items into day-labelled sections ("Today", "Yesterday", …),
 * preserving the feed's existing newest-first order — items already arrive
 * sorted by `starts_at`, so this only needs to notice when the label changes,
 * not re-sort anything.
 */
function groupByDay(items: FeedItem[]): { label: string; items: FeedItem[] }[] {
  const sections: { label: string; items: FeedItem[] }[] = []
  for (const item of items) {
    const label = dayLabel(item.starts_at)
    const last = sections[sections.length - 1]
    if (last && last.label === label) {
      last.items.push(item)
    } else {
      sections.push({ label, items: [item] })
    }
  }
  return sections
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
