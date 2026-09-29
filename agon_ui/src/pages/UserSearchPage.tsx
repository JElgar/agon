import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Search } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { UserCard } from '@/components/agon/UserCard'
import { Card } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type UserProfile = components['schemas']['UserProfile']

/** Debounce for the search input, so we don't fire a request per keystroke. */
const DEBOUNCE_MS = 300

/**
 * Find-people page: a search box over `GET /users/search`, results rendered as
 * `UserCard`s with an inline follow button. The query is debounced and only
 * runs for a non-empty term; the viewer's own row still lists but without a
 * follow button (handled by `UserCard`).
 */
export function UserSearchPage() {
  const currentUserId = useCurrentUserId()
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const id = setTimeout(() => setDebounced(term.trim()), DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [term])

  const query = useQuery({
    queryKey: ['user-search', debounced],
    enabled: debounced.length > 0,
    queryFn: async (): Promise<UserProfile[]> => {
      const { data, error } = await fetchClient.GET('/users/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Failed to search users')
      return data
    },
  })

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <h1 className="font-display text-xl font-extrabold">Find people</h1>

      <div className="relative">
        <Search className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          placeholder="Search by name…"
          className="h-12 rounded-2xl pl-10"
          autoFocus
          aria-label="Search people"
        />
      </div>

      <Results
        query={query}
        term={debounced}
        currentUserId={currentUserId}
      />
    </div>
  )
}

interface ResultsProps {
  query: ReturnType<typeof useQuery<UserProfile[]>>
  term: string
  currentUserId?: string
}

/** The results region: prompt / loading / error / empty / list states. */
function Results({ query, term, currentUserId }: ResultsProps) {
  if (term.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        Search for people to follow.
      </p>
    )
  }

  if (query.isLoading) {
    return (
      <Card className="flex flex-col overflow-hidden">
        {Array.from({ length: 5 }).map((_, i) => (
          <div
            key={i}
            className="flex items-center gap-3 border-b px-4 py-3.5 last:border-b-0"
          >
            <div className="size-9 shrink-0 animate-pulse rounded-full bg-muted" />
            <div className="flex-1 space-y-2">
              <div className="h-3 w-1/3 animate-pulse rounded bg-muted" />
              <div className="h-2.5 w-1/4 animate-pulse rounded bg-muted" />
            </div>
          </div>
        ))}
      </Card>
    )
  }

  if (query.isError) {
    return (
      <div className="py-12 text-center">
        <p className="mb-3 text-sm text-muted-foreground">
          Couldn't run that search.
        </p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const users = query.data ?? []

  if (users.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        No people match “{term}”.
      </p>
    )
  }

  return (
    <Card className="overflow-hidden">
      <ul className="flex flex-col divide-y">
        {users.map((user) => (
          <li key={user.id}>
            <UserCard user={user} currentUserId={currentUserId} />
          </li>
        ))}
      </ul>
    </Card>
  )
}
