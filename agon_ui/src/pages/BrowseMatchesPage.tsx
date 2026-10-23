import { useEffect, useMemo, useState } from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { ArrowLeft, ChevronDown, Search, X } from 'lucide-react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { MatchCard } from '@/components/agon/MatchCard'
import { Avatar } from '@/components/agon/Avatar'
import { Chip } from '@/components/agon/Chip'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Combobox,
  ComboboxCollection,
  ComboboxContent,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxList,
} from '@/components/ui/combobox'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { sportIcon, sportLabel, type MatchType } from '@/lib/sports'
import { cn } from '@/lib/utils'

type SearchMatch = components['schemas']['SearchMatch']
type FeedMatch = components['schemas']['FeedMatch']
type UserProfile = components['schemas']['UserProfile']
type TeamListItem = components['schemas']['TeamListItem']
type MatchOutcome = components['schemas']['MatchOutcome']

/** Either tab's result item — the "All" tab reads `GET /matches` (always
 *  `SearchMatch`), the "Scheduled" tab's "Everyone's" scope reads `GET /feed`
 *  instead (see that tab's query below), which returns `FeedMatch`. */
type BrowseItem = SearchMatch | FeedMatch

/** One top-level tab of this page. Carried in the `tab` URL search param so
 *  `FeedPage`'s "See all scheduled games" link can deep-link straight into
 *  the scheduled tab instead of needing its own route. */
type Tab = 'all' | 'scheduled'

/** The sports offered in the "All" tab's multi-select filter and the
 *  "Scheduled" tab's single-select chips, in the same order. */
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

const RESULTS: { id: MatchOutcome; label: string }[] = [
  { id: 'won', label: 'Won' },
  { id: 'lost', label: 'Lost' },
  { id: 'draw', label: 'Drawn' },
]

/** Debounce for free-text search inputs. */
const DEBOUNCE_MS = 300

const PAGE_SIZE = 20

/** Either half of the player/team filter's single selection. */
type Subject =
  | { kind: 'user'; id: string; name: string; imageUrl?: string }
  | { kind: 'team'; id: string; name: string; imageUrl?: string }

/** One row offered by the player/team combobox. */
type SubjectItem = { kind: 'user'; user: UserProfile } | { kind: 'team'; team: TeamListItem }

/** One group of the player/team combobox's grouped `items`. */
interface SubjectGroup {
  value: string
  items: SubjectItem[]
}

/**
 * Browse every match in the app, either with compound filters ("All" tab:
 * free text, any combination of sports, and — once a player or team is
 * picked — that player/team's result) or scoped to what's still to come
 * ("Scheduled" tab: the viewer's own games or everyone's, by sport and date).
 *
 * The two tabs used to be separate pages/buttons from the feed ("Browse all
 * matches" and "See all scheduled games") — folded into one page here so the
 * feed only needs a single entry point. "Scheduled" keeps its own query
 * logic rather than reusing "All"'s: "My games" hits `GET /matches` with
 * `status=scheduled` (full-text search, soonest-first sort); "Everyone's"
 * deliberately does NOT search across every match in the app (that's what
 * `GET /matches` without `participant` would do) — it's meant to be "the
 * scheduled matches my feed would show", so it calls `GET /feed` instead
 * (the same follows-scoped audience `FeedPage` reads), filtered to
 * `status=scheduled`. `GET /feed` has no free-text search or sort-order
 * param of its own, so the search box is hidden there and results are
 * sorted by `starts_at` client-side, per loaded page.
 *
 * "Result" (All tab) only has meaning relative to a single person — the
 * server resolves `outcome` off whichever id is passed as `participant` —
 * so it stays hidden until a *player* (not a team) is selected, and is
 * applied client-side over each loaded page.
 */
export function BrowseMatchesPage() {
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()
  const [searchParams, setSearchParams] = useSearchParams()

  const tab: Tab = searchParams.get('tab') === 'scheduled' ? 'scheduled' : 'all'
  const setTab = (next: Tab) => {
    setSearchParams(
      (prev) => {
        const params = new URLSearchParams(prev)
        if (next === 'all') params.delete('tab')
        else params.set('tab', next)
        return params
      },
      { replace: true },
    )
  }

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
        <h1 className="font-display text-xl font-extrabold md:text-3xl">Browse matches</h1>
      </div>

      <div className="inline-flex w-fit rounded-full border bg-card p-1">
        {(['all', 'scheduled'] as const).map((t) => (
          <button
            key={t}
            type="button"
            aria-pressed={tab === t}
            onClick={() => setTab(t)}
            className={cn(
              'h-8 rounded-full px-3.5 text-sm font-semibold transition-colors',
              tab === t
                ? 'bg-accent text-accent-foreground'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {t === 'all' ? 'All matches' : 'Scheduled'}
          </button>
        ))}
      </div>

      {tab === 'all' ? (
        <AllMatchesTab currentUserId={currentUserId} navigate={navigate} />
      ) : (
        <ScheduledTab currentUserId={currentUserId} navigate={navigate} />
      )}
    </div>
  )
}

interface TabProps {
  currentUserId?: string
  navigate: ReturnType<typeof useNavigate>
}

function AllMatchesTab({ currentUserId, navigate }: TabProps) {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')
  const [sports, setSports] = useState<MatchType[]>([])
  const [subject, setSubject] = useState<Subject | null>(null)
  const [subjectTerm, setSubjectTerm] = useState('')
  const [subjectDebounced, setSubjectDebounced] = useState('')
  const [result, setResult] = useState<MatchOutcome | null>(null)

  useEffect(() => {
    const id = setTimeout(() => setDebounced(term.trim()), DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [term])

  useEffect(() => {
    const id = setTimeout(() => setSubjectDebounced(subjectTerm.trim()), DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [subjectTerm])

  const participant = subject?.kind === 'user' ? subject.id : undefined
  const teamId = subject?.kind === 'team' ? subject.id : undefined
  // Result only has a server-resolvable meaning relative to one player.
  const resultEnabled = subject?.kind === 'user'

  const query = useInfiniteQuery({
    queryKey: ['browse-matches', debounced, sports, participant, teamId],
    enabled: !!currentUserId,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<{ items: SearchMatch[]; next_cursor?: string | null }> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: {
          query: {
            q: debounced || undefined,
            match_type: sports.length ? sports : undefined,
            participant,
            team_id: teamId ? [teamId] : undefined,
            cursor: pageParam,
            limit: PAGE_SIZE,
          },
        },
      })
      if (error || !data) throw new Error('Failed to load matches')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
  })

  const allItems: SearchMatch[] = (query.data?.pages ?? []).flatMap((page) => page.items)
  const items = resultEnabled && result ? allItems.filter((m) => m.outcome === result) : allItems

  return (
    <>
      <div className="relative">
        <Search className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          placeholder="Search by match or team…"
          className="h-12 rounded-2xl pl-10"
          aria-label="Search matches"
        />
      </div>

      <div className="flex flex-wrap items-start gap-2">
        <SportFilter sports={sports} onChange={setSports} />

        <SubjectFilter
          subject={subject}
          term={subjectTerm}
          debounced={subjectDebounced}
          onTermChange={setSubjectTerm}
          onChange={(next) => {
            setSubject(next)
            setResult(null)
            setSubjectTerm('')
            setSubjectDebounced('')
          }}
        />

        {resultEnabled &&
          RESULTS.map((r) => (
            <Chip key={r.id} pressed={result === r.id} onClick={() => setResult(result === r.id ? null : r.id)}>
              {r.label}
            </Chip>
          ))}
      </div>

      <Results query={query} items={items} currentUserId={currentUserId} navigate={navigate} />
    </>
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

function ScheduledTab({ currentUserId, navigate }: TabProps) {
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
    queryFn: async ({ pageParam }): Promise<{ items: BrowseItem[]; next_cursor?: string | null }> => {
      if (scope === 'mine') {
        const { data, error } = await fetchClient.GET('/matches', {
          params: {
            query: {
              participant: currentUserId,
              status: 'scheduled',
              sort: 'asc',
              q: debounced || undefined,
              match_type: sport ? [sport] : undefined,
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

  const items: BrowseItem[] = (query.data?.pages ?? []).flatMap((page) => page.items)

  return (
    <>
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
    </>
  )
}

interface SportFilterProps {
  sports: MatchType[]
  onChange: (sports: MatchType[]) => void
}

/** Sport pill that opens a multi-select checklist — any number can be picked. */
function SportFilter({ sports, onChange }: SportFilterProps) {
  const label =
    sports.length === 0
      ? 'Sport'
      : sports.length <= 2
        ? sports.map(sportLabel).join(', ')
        : `${sports.length} sports`

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className={cn(
            'inline-flex h-9 items-center gap-1.5 rounded-full border px-3.5 text-sm font-semibold transition-colors',
            sports.length > 0
              ? 'border-primary bg-accent text-accent-foreground'
              : 'border-input bg-card text-muted-foreground hover:bg-accent/50',
          )}
        >
          {label}
          <ChevronDown className="size-3.5" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {SPORTS.map((s) => {
          const Icon = sportIcon(s)
          const checked = sports.includes(s)
          return (
            <DropdownMenuCheckboxItem
              key={s}
              checked={checked}
              onCheckedChange={(next) =>
                onChange(next ? [...sports, s] : sports.filter((x) => x !== s))
              }
              onSelect={(e) => e.preventDefault()}
            >
              <Icon className="mr-2 size-4" />
              {sportLabel(s)}
            </DropdownMenuCheckboxItem>
          )
        })}
        {sports.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <button
              type="button"
              className="w-full rounded-[9px] px-2.5 py-[9px] text-left text-sm font-semibold text-primary hover:bg-accent"
              onClick={() => onChange([])}
            >
              Clear
            </button>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

interface SubjectFilterProps {
  subject: Subject | null
  term: string
  debounced: string
  onTermChange: (term: string) => void
  onChange: (subject: Subject | null) => void
}

/** How many of the user's own teams to hold client-side, same as `TeamPicker`. */
const MY_TEAMS_LIMIT = 100
const MY_TEAMS_PAGE_LIMIT = 50

/**
 * Player-or-team pill: once picked, collapses to a removable chip (same
 * pattern as `TeamPicker`). Searches `/users/search` and `/teams/search`
 * together, grouped, once 2+ characters are typed.
 */
function SubjectFilter({ subject, term, debounced, onTermChange, onChange }: SubjectFilterProps) {
  const searching = debounced.length >= 2

  const myTeams = useQuery({
    queryKey: ['my-teams-browse-filter'],
    enabled: !subject,
    queryFn: async (): Promise<TeamListItem[]> => {
      const items: TeamListItem[] = []
      let cursor: string | undefined
      while (items.length < MY_TEAMS_LIMIT) {
        const { data, error } = await fetchClient.GET('/users/me/teams', {
          params: { query: { cursor, limit: MY_TEAMS_PAGE_LIMIT } },
        })
        if (error || !data) throw new Error('Failed to load teams')
        items.push(...data.items)
        if (!data.next_cursor) break
        cursor = data.next_cursor
      }
      return items
    },
  })

  const userSearch = useQuery({
    queryKey: ['users-search-browse-filter', debounced],
    enabled: searching && !subject,
    queryFn: async (): Promise<UserProfile[]> => {
      const { data, error } = await fetchClient.GET('/users/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Search failed')
      return data
    },
  })

  const teamSearch = useQuery({
    queryKey: ['teams-search-browse-filter', debounced],
    enabled: searching && !subject,
    queryFn: async (): Promise<TeamListItem[]> => {
      const { data, error } = await fetchClient.GET('/teams/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Search failed')
      return data.items
    },
  })

  const myTeamIds = useMemo(() => new Set((myTeams.data ?? []).map((t) => t.id)), [myTeams.data])
  const teamResults = useMemo(
    () => (searching ? (teamSearch.data ?? []).filter((t) => !myTeamIds.has(t.id)) : []),
    [searching, teamSearch.data, myTeamIds],
  )
  const termLower = term.trim().toLowerCase()
  const myTeamMatches = useMemo(
    () => (myTeams.data ?? []).filter((t) => (termLower ? t.name.toLowerCase().includes(termLower) : true)),
    [myTeams.data, termLower],
  )

  const groups: SubjectGroup[] = []
  if (searching) {
    groups.push({
      value: 'Players',
      items: (userSearch.data ?? []).map((user): SubjectItem => ({ kind: 'user', user })),
    })
  }
  if (myTeamMatches.length > 0) {
    groups.push({
      value: 'Your teams',
      items: myTeamMatches.map((team): SubjectItem => ({ kind: 'team', team })),
    })
  }
  if (searching) {
    groups.push({
      value: 'Other teams',
      items: teamResults.map((team): SubjectItem => ({ kind: 'team', team })),
    })
  }

  const isLoading = searching && (userSearch.isLoading || teamSearch.isLoading)
  const nothingFound = searching && !isLoading && groups.every((g) => g.items.length === 0)

  if (subject) {
    return (
      <div className="inline-flex h-9 items-center gap-2 rounded-full border border-primary bg-accent px-2 pr-3 text-sm font-semibold text-accent-foreground">
        <Avatar name={subject.name} imageUrl={subject.imageUrl} size="sm" />
        <span className="max-w-32 truncate">{subject.name}</span>
        <button
          type="button"
          onClick={() => onChange(null)}
          aria-label={`Clear ${subject.name}`}
          className="text-muted-foreground transition-colors hover:text-foreground"
        >
          <X className="size-3.5" />
        </button>
      </div>
    )
  }

  return (
    <Combobox
      items={groups}
      filter={null}
      inputValue={term}
      onInputValueChange={onTermChange}
      onValueChange={(next) => {
        if (!next) return
        const item = next as SubjectItem
        if (item.kind === 'user') {
          onChange({
            kind: 'user',
            id: item.user.id,
            name: item.user.name,
            imageUrl: item.user.profile_image?.image_url,
          })
        } else {
          onChange({
            kind: 'team',
            id: item.team.id,
            name: item.team.name,
            imageUrl: item.team.logo?.image_url,
          })
        }
      }}
    >
      <ComboboxInput
        placeholder="Player or team"
        className="h-9 w-40 rounded-full border-input bg-card text-sm"
      />
      <ComboboxContent>
        {isLoading && <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>}
        {nothingFound && <p className="px-3 py-2 text-xs text-muted-foreground">No matches.</p>}
        <ComboboxList>
          {(group: SubjectGroup) => (
            <ComboboxGroup key={group.value} items={group.items}>
              <ComboboxLabel>{group.value}</ComboboxLabel>
              <ComboboxCollection>
                {(item: SubjectItem) => {
                  const name = item.kind === 'user' ? item.user.name : item.team.name
                  const imageUrl =
                    item.kind === 'user' ? item.user.profile_image?.image_url : item.team.logo?.image_url
                  const key = item.kind === 'user' ? item.user.id : item.team.id
                  return (
                    <ComboboxItem key={key} value={item}>
                      <Avatar name={name} imageUrl={imageUrl} size="md" />
                      <span className="flex-1 truncate">{name}</span>
                    </ComboboxItem>
                  )
                }}
              </ComboboxCollection>
            </ComboboxGroup>
          )}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}

interface ResultsProps<T extends BrowseItem> {
  query: ReturnType<typeof useInfiniteQuery<{ items: T[]; next_cursor?: string | null }>>
  items: T[]
  currentUserId?: string
  navigate: ReturnType<typeof useNavigate>
}

function Results<T extends BrowseItem>({ query, items, currentUserId, navigate }: ResultsProps<T>) {
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
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load matches.</p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        No matches match these filters.
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
