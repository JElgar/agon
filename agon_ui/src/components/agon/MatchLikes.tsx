import { useState } from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { Flame } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { useIsDesktop } from '@/hooks/useMediaQuery'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { useToggleLike } from '@/hooks/useToggleLike'
import { UserCard } from '@/components/agon/UserCard'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'

type Match = components['schemas']['Match']
type UserPage = components['schemas']['UserPage']

/** Page size for the full likes list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20
/** How many names to name in the summary row before falling back to "and N others". */
const NAMED_IN_SUMMARY = 2

/** "Alex liked this" / "Alex and Sam liked this" / "Alex, Sam and 3 others liked this". */
function likeSummary(names: string[], count: number): string | undefined {
  if (count === 0) return undefined
  if (names.length === 0) return count === 1 ? '1 like' : `${count} likes`

  const shown = names.slice(0, NAMED_IN_SUMMARY)
  const others = count - shown.length
  const who =
    others > 0
      ? `${shown.join(', ')} and ${others} ${others === 1 ? 'other' : 'others'}`
      : shown.length === 1
        ? shown[0]
        : `${shown.slice(0, -1).join(', ')} and ${shown[shown.length - 1]}`

  return `${who} liked this`
}

/**
 * The likes row: a tappable flame that toggles the viewer's own like
 * (`POST`/`DELETE /matches/{id}/likes`), plus a summary of who else liked it
 * ("Alex, Sam and 3 others liked this") that opens the full list. Sits
 * alongside the comment count on the match details page.
 */
export function MatchLikesRow({ match }: { match: Pick<Match, 'id' | 'social'> }) {
  const [open, setOpen] = useState(false)
  const isDesktop = useIsDesktop()
  const currentUserId = useCurrentUserId()
  const toggleLike = useToggleLike(match)
  const { like_count: count, i_liked: liked } = match.social

  // A handful of names for the summary row — the full list is only fetched
  // once the sheet actually opens (see `useInfiniteQuery` below).
  const previewQuery = useQuery({
    queryKey: ['match-likes', match.id, 'preview'],
    enabled: count > 0,
    queryFn: async (): Promise<UserPage> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/likes', {
        params: { path: { match_id: match.id }, query: { limit: NAMED_IN_SUMMARY } },
      })
      if (error || !data) throw new Error('Failed to load likes')
      return data
    },
  })
  const summary = likeSummary(
    (previewQuery.data?.items ?? []).map((u) => u.name),
    count,
  )

  return (
    <div className="flex items-center gap-2">
      <button
        type="button"
        aria-pressed={liked}
        aria-label={liked ? 'Remove kudos' : 'Give kudos'}
        onClick={() => toggleLike.mutate(!liked)}
        className={cn(
          'flex size-9 shrink-0 items-center justify-center rounded-full border border-edge bg-background text-muted-foreground transition-colors hover:text-primary',
          liked && 'border-primary/30 text-primary',
        )}
      >
        <Flame className={cn('size-4', liked && 'fill-current')} />
      </button>

      {summary && (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="truncate text-left text-sm text-muted-foreground hover:text-primary"
        >
          {summary}
        </button>
      )}

      <MatchLikesSheet
        matchId={match.id}
        open={open}
        onOpenChange={setOpen}
        isDesktop={isDesktop}
        currentUserId={currentUserId}
      />
    </div>
  )
}

/** The full "Liked by" list, cursor-paginated via `GET /matches/{id}/likes`. */
function MatchLikesSheet({
  matchId,
  open,
  onOpenChange,
  isDesktop,
  currentUserId,
}: {
  matchId: string
  open: boolean
  onOpenChange: (open: boolean) => void
  isDesktop: boolean
  currentUserId?: string
}) {
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
    <Sheet open={open} onOpenChange={onOpenChange}>
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
  )
}
