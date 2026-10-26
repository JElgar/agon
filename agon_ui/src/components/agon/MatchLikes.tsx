import { useState } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { Flame } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { useIsDesktop } from '@/hooks/useMediaQuery'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { UserCard } from '@/components/agon/UserCard'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'

type UserPage = components['schemas']['UserPage']

/** Page size for the likes list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

/**
 * The like count (flame icon), styled to sit alongside the comment count.
 * Clicking it opens a sheet listing who liked the match, via
 * `GET /matches/{match_id}/likes`.
 */
export function MatchLikesButton({
  matchId,
  count,
  liked,
}: {
  matchId: string
  count: number
  liked: boolean
}) {
  const [open, setOpen] = useState(false)
  const isDesktop = useIsDesktop()
  const currentUserId = useCurrentUserId()

  const query = useInfiniteQuery({
    queryKey: ['match-likes', matchId],
    enabled: open,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<UserPage> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/likes', {
        params: {
          path: { match_id: matchId },
          query: { cursor: pageParam, limit: PAGE_SIZE },
        },
      })
      if (error || !data) throw new Error('Failed to load likes')
      return data
    },
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  })

  const users = (query.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <>
      <button
        type="button"
        disabled={count === 0}
        onClick={() => setOpen(true)}
        aria-label={`Liked by ${count}`}
        className={cn(
          'flex items-center gap-1.5 text-sm font-semibold text-muted-foreground transition-colors',
          count > 0 && 'hover:text-primary',
          liked && 'text-primary',
        )}
      >
        <Flame className={cn('size-4', liked && 'fill-current')} />
        {count}
      </button>

      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent
          side={isDesktop ? 'right' : 'bottom'}
          className={cn(
            'overflow-y-auto bg-background p-4',
            isDesktop ? 'w-[420px] sm:max-w-[420px]' : 'max-h-[85vh] rounded-t-[20px]',
          )}
        >
          <SheetHeader className="p-0">
            <SheetTitle className="font-display text-xl">Liked by</SheetTitle>
          </SheetHeader>

          {query.isLoading ? (
            <p className="mt-4 text-sm text-muted-foreground">Loading…</p>
          ) : query.isError ? (
            <div className="mt-4 text-sm text-muted-foreground">
              <p className="mb-2">Couldn't load likes.</p>
              <Button variant="outline" shape="pill" size="sm" onClick={() => query.refetch()}>
                Retry
              </Button>
            </div>
          ) : users.length === 0 ? (
            <p className="mt-4 text-sm text-muted-foreground">No kudos yet.</p>
          ) : (
            <div className="mt-2 flex flex-col divide-y">
              {users.map((user) => (
                <UserCard key={user.id} user={user} currentUserId={currentUserId} />
              ))}
            </div>
          )}

          {query.hasNextPage && (
            <Button
              variant="ghost"
              shape="pill"
              size="sm"
              className="mt-3"
              disabled={query.isFetchingNextPage}
              onClick={() => query.fetchNextPage()}
            >
              {query.isFetchingNextPage ? 'Loading…' : 'Load more'}
            </Button>
          )}
        </SheetContent>
      </Sheet>
    </>
  )
}
