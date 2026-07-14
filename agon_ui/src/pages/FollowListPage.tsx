import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { UserCard } from '@/components/agon/UserCard'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type UserPage = components['schemas']['UserPage']

/** Page size for the list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

export type FollowListMode = 'followers' | 'following'

/**
 * A user's followers or following list (`GET /users/{id}/followers` /
 * `/following`), cursor-paginated with a "Load more" button and each row a
 * `UserCard` carrying an accurate follow button (the API now populates
 * `is_followed_by_me` for the viewer). Which list is shown is fixed by `mode`;
 * the subject user comes from the `:userId` route param.
 */
export function FollowListPage({ mode }: { mode: FollowListMode }) {
  const { userId } = useParams()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  // The subject's name, for the heading. Cheap and usually already cached from
  // the profile page the viewer came from.
  const nameQuery = useQuery({
    queryKey: ['profile', userId ?? 'me'],
    enabled: !!userId,
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/users/{user_id}', {
        params: { path: { user_id: userId! } },
      })
      if (error || !data) throw new Error('Failed to load profile')
      return data
    },
  })

  const list = useInfiniteQuery({
    queryKey: ['follow-list', mode, userId],
    enabled: !!userId,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<UserPage> => {
      const path = { user_id: userId! }
      const params = { path, query: { cursor: pageParam, limit: PAGE_SIZE } }
      const { data, error } =
        mode === 'followers'
          ? await fetchClient.GET('/users/{user_id}/followers', { params })
          : await fetchClient.GET('/users/{user_id}/following', { params })
      if (error || !data) throw new Error('Failed to load list')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
  })

  const title = mode === 'followers' ? 'Followers' : 'Following'
  const items = (list.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="icon"
          className="size-8"
          aria-label="Back"
          onClick={() => navigate(-1)}
        >
          <ChevronLeft className="size-4" />
        </Button>
        <h1 className="text-xl font-semibold">
          {title}
          {nameQuery.data && (
            <span className="ml-2 text-sm font-normal text-muted-foreground">
              {nameQuery.data.name}
            </span>
          )}
        </h1>
      </div>

      <ListBody
        list={list}
        items={items}
        mode={mode}
        currentUserId={currentUserId}
      />
    </div>
  )
}

interface ListBodyProps {
  list: ReturnType<typeof useInfiniteQuery<UserPage>>
  items: components['schemas']['UserProfile'][]
  mode: FollowListMode
  currentUserId?: string
}

function ListBody({ list, items, mode, currentUserId }: ListBodyProps) {
  if (list.isLoading) {
    return (
      <ul className="flex flex-col overflow-hidden rounded-xl border bg-card">
        {Array.from({ length: 6 }).map((_, i) => (
          <li key={i} className="flex items-center gap-3 border-b px-4 py-3 last:border-b-0">
            <div className="size-9 shrink-0 animate-pulse rounded-full bg-muted" />
            <div className="flex-1 space-y-2">
              <div className="h-3 w-1/3 animate-pulse rounded bg-muted" />
              <div className="h-2.5 w-1/4 animate-pulse rounded bg-muted" />
            </div>
          </li>
        ))}
      </ul>
    )
  }

  if (list.isError) {
    return (
      <div className="py-12 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load this list.</p>
        <Button variant="outline" size="sm" onClick={() => list.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        {mode === 'followers'
          ? 'No followers yet.'
          : 'Not following anyone yet.'}
      </p>
    )
  }

  return (
    <>
      <ul className="flex flex-col divide-y overflow-hidden rounded-xl border bg-card">
        {items.map((user) => (
          <li key={user.id}>
            <UserCard user={user} currentUserId={currentUserId} />
          </li>
        ))}
      </ul>

      {list.hasNextPage && (
        <Button
          variant="outline"
          disabled={list.isFetchingNextPage}
          onClick={() => list.fetchNextPage()}
        >
          {list.isFetchingNextPage ? 'Loading…' : 'Load more'}
        </Button>
      )}
    </>
  )
}
