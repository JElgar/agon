import { useInfiniteQuery } from '@tanstack/react-query'
import { Plus, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { CreateTeamDialog } from '@/components/agon/CreateTeamDialog'
import { TeamCard } from '@/components/agon/TeamCard'
import { Button } from '@/components/ui/button'

type TeamPage = components['schemas']['TeamPage']

/** Page size for the list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

/**
 * "My teams" (`GET /users/me/teams`), cursor-paginated with a "Load more"
 * button, plus the entry point for creating a new team. Reached from the
 * sidebar (desktop) and the profile page's Account section (mobile) — see
 * `AppSidebar` / `ProfilePage`.
 */
export function TeamsPage() {
  const list = useInfiniteQuery({
    queryKey: ['my-teams'],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<TeamPage> => {
      const { data, error } = await fetchClient.GET('/users/me/teams', {
        params: { query: { cursor: pageParam, limit: PAGE_SIZE } },
      })
      if (error || !data) throw new Error('Failed to load teams')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
  })

  const items = (list.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h1 className="text-xl font-semibold">Teams</h1>
        <CreateTeamDialog>
          <Button size="sm" className="gap-2">
            <Plus className="size-4" />
            Create team
          </Button>
        </CreateTeamDialog>
      </div>

      <ListBody list={list} items={items} />
    </div>
  )
}

interface ListBodyProps {
  list: ReturnType<typeof useInfiniteQuery<TeamPage>>
  items: components['schemas']['TeamListItem'][]
}

/** The list region: loading / error / empty / list states, plus "Load more". */
function ListBody({ list, items }: ListBodyProps) {
  if (list.isLoading) {
    return (
      <ul className="flex flex-col overflow-hidden rounded-xl border bg-card">
        {Array.from({ length: 4 }).map((_, i) => (
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
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load your teams.</p>
        <Button variant="outline" size="sm" onClick={() => list.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 rounded-xl border bg-card py-16 text-center">
        <Users className="size-8 text-muted-foreground" />
        <p className="text-sm text-muted-foreground">
          You're not on a team yet.
        </p>
      </div>
    )
  }

  return (
    <>
      <ul className="flex flex-col divide-y overflow-hidden rounded-xl border bg-card">
        {items.map((team) => (
          <li key={team.id}>
            <TeamCard team={team} />
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
