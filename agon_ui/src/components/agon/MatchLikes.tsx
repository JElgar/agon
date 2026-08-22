import { useState } from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { UserCard } from './UserCard'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type UserPage = components['schemas']['UserPage']

/** Page size for the full likers list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20
/** Names shown inline before collapsing the rest into "+N" — just enough to
 *  fetch for the summary line, so it doesn't pull a full page just to show
 *  two names. */
const SUMMARY_NAMES = 2

/**
 * "Liked by A, B +N" summary text for `names` (already capped to at most
 * `SUMMARY_NAMES`) out of `total` likes. Assumes `total > 0` — callers hide
 * the line entirely when there are no likes.
 */
function formatLikedBy(names: string[], total: number): string {
  if (names.length === 0) return `${total} ${total === 1 ? 'like' : 'likes'}`
  if (total <= names.length) {
    return total === 1
      ? `Liked by ${names[0]}`
      : `Liked by ${names.join(' and ')}`
  }
  const extra = total - names.length
  return `Liked by ${names.join(', ')} +${extra}`
}

/**
 * The match detail page's "Liked by A, B +N" line, driven by
 * `GET /matches/{id}/likes`. Clicking it opens `MatchLikesDialog` with the
 * full, paginated list of likers. Renders nothing when there are no likes.
 */
export function LikedByLine({
  matchId,
  likeCount,
}: {
  matchId: string
  likeCount: number
}) {
  const [open, setOpen] = useState(false)

  const summary = useQuery({
    queryKey: ['match-likes-summary', matchId],
    enabled: likeCount > 0,
    queryFn: async (): Promise<UserPage> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/likes', {
        params: {
          path: { match_id: matchId },
          query: { limit: SUMMARY_NAMES },
        },
      })
      if (error || !data) throw new Error('Failed to load likes')
      return data
    },
  })

  if (likeCount === 0) return null

  const names = (summary.data?.items ?? []).map((u) => u.name)

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="truncate text-left transition-colors hover:text-primary hover:underline"
      >
        {formatLikedBy(names, likeCount)}
      </button>
      <MatchLikesDialog matchId={matchId} open={open} onOpenChange={setOpen} />
    </>
  )
}

/** The full, paginated "who liked this match" list, opened from `LikedByLine`. */
function MatchLikesDialog({
  matchId,
  open,
  onOpenChange,
}: {
  matchId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const currentUserId = useCurrentUserId()

  const list = useInfiniteQuery({
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
    getNextPageParam: (last) => last.next_cursor,
  })

  const items = (list.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Likes</DialogTitle>
        </DialogHeader>

        {list.isLoading ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            Loading…
          </p>
        ) : list.isError ? (
          <div className="py-6 text-center">
            <p className="mb-2 text-sm text-muted-foreground">
              Couldn't load likes.
            </p>
            <Button variant="outline" size="sm" onClick={() => list.refetch()}>
              Retry
            </Button>
          </div>
        ) : items.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            No likes yet.
          </p>
        ) : (
          <ul className="-mx-6 flex flex-col divide-y">
            {items.map((user) => (
              <li key={user.id}>
                <UserCard user={user} currentUserId={currentUserId} />
              </li>
            ))}
          </ul>
        )}

        {list.hasNextPage && (
          <Button
            variant="outline"
            size="sm"
            disabled={list.isFetchingNextPage}
            onClick={() => list.fetchNextPage()}
          >
            {list.isFetchingNextPage ? 'Loading…' : 'Load more'}
          </Button>
        )}
      </DialogContent>
    </Dialog>
  )
}
