import { useEffect, useState } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { ArrowLeft, Search } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { sportIcon, sportLabel, type MatchType } from '@/lib/sports'

type SearchMatch = components['schemas']['SearchMatch']
type FeedMatch = components['schemas']['FeedMatch']
type ScheduledItem = SearchMatch | FeedMatch

/** The sports offered as filter chips, in the same order as `SportPicker`. */
const SPORTS: MatchType[] = [
  'tennis',
  'badminton',
  'squash',
  'table_tennis',
  'football',
  'cricket',
  'netball',
  'other',
]

/** Debounce for the search input, so we don't fire a request per keystroke. */
const DEBOUNCE_MS = 300

const PAGE_SIZE = 20

/**
 * "See all" from the feed's "Coming up" strip: every scheduled match the
 * viewer is playing in, searchable and filterable by sport and date. Off by
 * default, matches whose `starts_at` is more than 24h in the past are hidden
 * too (same as the feed strip, see `FeedPage`'s `upcomingCutoff`) — the
 * "Include past games" toggle lifts that so a stale/never-scored scheduled
 * match can still be found.
 *
 * "My games" is a `GET /matches` search (participant = the viewer), which
 * supports full-text search plus a soonest-first sort — it's fine for this to
 * search the whole app since it's scoped to the viewer's own matches anyway.
 * "Everyone's" deliberately does NOT search across every match in the app
 * (that's what `GET /matches` without `participant` would do) — it's meant to
 * be "the scheduled matches my feed would show", so it calls `GET /feed`
 * instead (the same follows-scoped audience `FeedPage` reads), filtered to
 * `status=scheduled`. `GET /feed` has no free-text search or sort-order
 * param of its own (see its doc comment), so the search box is hidden and
 * results are sorted by `starts_at` client-side, per loaded page.
 */
export function ScheduledMatchesPage() {
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')
  const [scope, setScope] = useState<'mine' | 'everyone'>('mine')
  const [sport, setSport] = useState<MatchType | null>(null)
  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')
  const [includePast, setIncludePast] = useState(false)

  useEffect(() => {
    const id = setTimeout(() => setDebounced(term.trim()), DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [term])

  // Explicit `fromDate` always wins; otherwise, unless "include past" is on,
  // scope to matches starting from today so old scheduled-but-never-played
  // games don't clutter the default view.
  const effectiveFrom = fromDate || (includePast ? undefined : startOfToday())
  const effectiveTo = toDate ? endOfDay(toDate) : undefined

  const query = useInfiniteQuery({
    queryKey: [
      'scheduled-matches',
      scope,
      scope === 'mine' ? currentUserId : null,
      scope === 'mine' ? debounced : '',
      sport,
      effectiveFrom,
      effectiveTo,
    ],
    enabled: !!currentUserId,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<{ items: ScheduledItem[]; next_cursor?: string | null }> => {
      if (scope === 'mine') {
        const { data, error } = await fetchClient.GET('/matches', {
          params: {
            query: {
              participant: currentUserId,
              status: 'scheduled',
              sort: 'asc',
              q: debounced || undefined,
              match_type: sport ?? undefined,
              from: effectiveFrom,
              to: effectiveTo,
              cursor: pageParam,
              limit: PAGE_SIZE,
            },
          },
        })
        if (error || !data) throw new Error('Failed to load scheduled matches')
        return data
      }

      const { data, error } = await fetchClient.GET('/feed', {
        params: {
          query: {
            status: 'scheduled',
            match_type: sport ?? undefined,
            from: effectiveFrom,
            to: effectiveTo,
            cursor: pageParam,
            limit: PAGE_SIZE,
          },
        },
      })
      if (error || !data) throw new Error('Failed to load scheduled matches')
      // `GET /feed` orders by fan-out time, not kickoff — resort each page so
      // "soonest first" still holds within what's been loaded so far.
      return {
        ...data,
        items: [...data.items].sort(
          (a, b) => new Date(a.starts_at).getTime() - new Date(b.starts_at).getTime(),
        ),
      }
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
  })

  const items: ScheduledItem[] = (query.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 md:max-w-2xl md:gap-6">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="icon"
          className="-ml-2"
          onClick={() => navigate(-1)}
          aria-label="Back"
        >
          <ArrowLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-3xl">
          Scheduled matches
        </h1>
      </div>

      {scope === 'mine' && (
        <div className="relative">
          <Search className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            placeholder="Search by match name…"
            className="h-12 rounded-2xl pl-10"
            aria-label="Search scheduled matches"
          />
        </div>
      )}

      <div className="inline-flex w-fit rounded-full border bg-card p-1">
        {(['mine', 'everyone'] as const).map((s) => (
          <button
            key={s}
            type="button"
            aria-pressed={scope === s}
            onClick={() => setScope(s)}
            className={cn(
              'h-8 rounded-full px-3.5 text-sm font-semibold transition-colors',
              scope === s
                ? 'bg-accent text-accent-foreground'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {s === 'mine' ? 'My games' : "Everyone's"}
          </button>
        ))}
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          aria-pressed={sport === null}
          onClick={() => setSport(null)}
          className={cn(
            'inline-flex h-9 items-center gap-2 rounded-full border px-3.5 text-sm font-semibold transition-colors',
            sport === null
              ? 'border-primary bg-accent text-accent-foreground'
              : 'border-input bg-card text-muted-foreground hover:bg-accent/50',
          )}
        >
          All sports
        </button>
        {SPORTS.map((s) => {
          const Icon = sportIcon(s)
          const selected = sport === s
          return (
            <button
              key={s}
              type="button"
              aria-pressed={selected}
              onClick={() => setSport(selected ? null : s)}
              className={cn(
                'inline-flex h-9 items-center gap-2 rounded-full border px-3.5 text-sm font-semibold transition-colors',
                selected
                  ? 'border-primary bg-accent text-accent-foreground'
                  : 'border-input bg-card text-muted-foreground hover:bg-accent/50',
              )}
            >
              <Icon className={cn('size-4', selected && 'text-primary')} />
              {sportLabel(s)}
            </button>
          )
        })}
      </div>

      <div className="flex flex-col gap-3 rounded-2xl border bg-card p-3.5">
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex flex-col gap-1">
            <Label htmlFor="scheduled-from" className="text-xs text-muted-foreground">
              From
            </Label>
            <Input
              id="scheduled-from"
              type="date"
              value={fromDate}
              onChange={(e) => setFromDate(e.target.value)}
              className="h-9 w-40"
            />
          </div>
          <div className="flex flex-col gap-1">
            <Label htmlFor="scheduled-to" className="text-xs text-muted-foreground">
              To
            </Label>
            <Input
              id="scheduled-to"
              type="date"
              value={toDate}
              onChange={(e) => setToDate(e.target.value)}
              className="h-9 w-40"
            />
          </div>
          {(fromDate || toDate) && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setFromDate('')
                setToDate('')
              }}
            >
              Clear dates
            </Button>
          )}
        </div>
        <div className="flex items-center justify-between gap-3 border-t pt-3">
          <Label htmlFor="include-past" className="text-sm font-medium">
            Include past games
          </Label>
          <Switch id="include-past" checked={includePast} onCheckedChange={setIncludePast} />
        </div>
      </div>

      <Results query={query} items={items} currentUserId={currentUserId} navigate={navigate} />
    </div>
  )
}

/** Midnight today, as an ISO instant, for the default "future only" filter. */
function startOfToday(): string {
  const d = new Date()
  d.setHours(0, 0, 0, 0)
  return d.toISOString()
}

/** End of the given "YYYY-MM-DD" day, as an ISO instant, inclusive. */
function endOfDay(dateStr: string): string {
  const d = new Date(dateStr)
  d.setHours(23, 59, 59, 999)
  return d.toISOString()
}

interface ResultsProps {
  query: ReturnType<typeof useInfiniteQuery<{ items: ScheduledItem[]; next_cursor?: string | null }>>
  items: ScheduledItem[]
  currentUserId?: string
  navigate: ReturnType<typeof useNavigate>
}

function Results({ query, items, currentUserId, navigate }: ResultsProps) {
  if (query.isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="h-32 animate-pulse rounded-2xl border bg-card" aria-hidden />
        ))}
      </div>
    )
  }

  if (query.isError) {
    return (
      <div className="py-12 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load scheduled matches.</p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        No scheduled matches match these filters.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-3">
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
