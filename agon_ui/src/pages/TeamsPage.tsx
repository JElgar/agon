import { useInfiniteQuery } from '@tanstack/react-query'
import { Plus, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { CreateTeamDialog } from '@/components/agon/CreateTeamDialog'
import { TeamCard } from '@/components/agon/TeamCard'
import { Card } from '@/components/ui/card'
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
    <div className="mx-auto flex max-w-xl flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h1 className="font-display text-2xl font-extrabold">Teams</h1>
        <CreateTeamDialog>
          <Button shape="pill" className="gap-1.5">
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
      <Card className="flex flex-col overflow-hidden">
        {Array.from({ length: 4 }).map((_, i) => (
          <div
            key={i}
            className="flex items-center gap-3.5 border-b px-4 py-3.5 last:border-b-0"
          >
            <div className="size-12 shrink-0 animate-pulse rounded-2xl bg-muted" />
            <div className="flex-1 space-y-2">
              <div className="h-3.5 w-1/3 animate-pulse rounded bg-muted" />
              <div className="h-3 w-1/4 animate-pulse rounded bg-muted" />
            </div>
          </div>
        ))}
      </Card>
    )
  }

  if (list.isError) {
    return (
      <div className="py-12 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load your teams.</p>
        <Button variant="outline" shape="pill" size="sm" onClick={() => list.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <Card className="flex flex-col items-center gap-2 py-16 text-center">
        <Users className="size-8 text-muted-foreground" />
        <p className="text-sm text-muted-foreground">
          You're not on a team yet.
        </p>
      </Card>
    )
  }

  return (
    <div className="flex flex-col gap-2.5">
      <Card className="flex flex-col divide-y overflow-hidden">
        {items.map((team) => (
          <TeamCard key={team.id} team={team} />
        ))}
      </Card>

      {list.hasNextPage && (
        <Button
          variant="outline"
          shape="pill"
          className="h-11"
          disabled={list.isFetchingNextPage}
          onClick={() => list.fetchNextPage()}
        >
          {list.isFetchingNextPage ? 'Loading…' : 'Load more'}
        </Button>
      )}
    </div>
  )
}
