import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Search } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Chip } from '@/components/agon/Chip'
import { StatTile } from '@/components/agon/StatTile'
import {
  MatchActivityChart,
  type MonthlyActivityPoint,
} from '@/components/agon/MatchActivityChart'
import { sportIcon, sportLabel, type MatchType } from '@/lib/sports'
import { formatWinRate, type BestFigure } from '@/lib/stats'
import { formatOvers } from '@/lib/cricketScore'
import { StatInfo } from '@/components/agon/StatInfo'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { cn } from '@/lib/utils'

type BestBowlingFigures = components['schemas']['BestBowlingFigures']
type UserProfile = components['schemas']['UserProfile']
type SearchMatch = components['schemas']['SearchMatch']
type MatchOutcome = components['schemas']['MatchOutcome']

/** Recent games listed below the chart. */
const RECENT_LIMIT = 10
/** Matches fetched to build the activity chart and recent-games list — the
 *  API caps at 50 either way. */
const FETCH_LIMIT = 50
/** Trailing months shown in the activity chart. */
const CHART_MONTHS = 6
/** Outcomes shown in the "recent form" row, most recent first. */
const FORM_LIMIT = 5

const MONTH_LABELS = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
] as const
const MONTH_FULL_LABELS = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
] as const

type ResultFilter = 'all' | 'won' | 'lost' | 'drawn'

/**
 * A single sport's full lifetime stats (`/profile/stats/:sport` or
 * `/users/:userId/stats/:sport`): the outcome breakdown, cricket/football's
 * sport-specific counters and personal bests, a matches-per-month activity
 * chart, and a searchable recent-games list — all driven off the same
 * profile + `GET /matches?participant&match_type` data the profile page's
 * summary row and "recent activity" already use. Restyled per the "Sport"
 * board of the "Agon redesign" canvas (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc)
 * — football is the board's own worked example; every other sport gets the
 * same shell with its own counters, same as the match-detail redesign.
 */
export function SportStatsPage() {
  const { userId, sport } = useParams<{ userId?: string; sport: string }>()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()
  const isOwnProfile = !userId
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ResultFilter>('all')

  const profileQuery = useQuery({
    queryKey: ['profile', userId ?? 'me'],
    queryFn: async (): Promise<UserProfile> => {
      if (userId) {
        const { data, error } = await fetchClient.GET('/users/{user_id}', {
          params: { path: { user_id: userId } },
        })
        if (error || !data) throw new Error('Failed to load profile')
        return data
      }
      const { data, error } = await fetchClient.GET('/users/me')
      if (error || !data) throw new Error('Failed to load profile')
      return data.profile
    },
  })

  const profileId = profileQuery.data?.id
  const matchType = sport as MatchType

  const matchesQuery = useQuery({
    queryKey: ['profile-sport-matches', profileId, sport],
    enabled: !!profileId && !!sport,
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: {
          query: { participant: profileId, match_type: matchType, limit: FETCH_LIMIT },
        },
      })
      if (error || !data) throw new Error('Failed to load matches')
      return data.items
    },
  })

  if (profileQuery.isLoading) {
    return <SportStatsSkeleton />
  }

  if (profileQuery.isError || !profileQuery.data || !sport) {
    return (
      <div className="mx-auto flex max-w-xl flex-col items-center gap-4 py-16 text-center">
        <p className="text-muted-foreground">Couldn't load these stats.</p>
        <Button variant="outline" onClick={() => profileQuery.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const profile = profileQuery.data
  const stats = profile.stats[matchType as keyof typeof profile.stats]
  const Icon = sportIcon(matchType)
  const matches = matchesQuery.data
  const recentByDate = sortedByDate(matches)
  const chartData = monthlyActivity(matches)
  const busiest = busiestMonth(chartData)

  // Football's desktop "Record" card folds the goals/assists numbers in
  // beside the win rate (see `DesktopSport.dc.html`) — no other sport has a
  // bespoke desktop board, so everyone else's banner just reflows via its
  // own responsive classes with no `extra` cluster.
  const bannerExtra =
    stats && 'goals' in stats ? (
      <>
        <PerMatchStat value={stats.goals} label="Goals" matches={stats.matches_played} />
        <PerMatchStat value={stats.assists} label="Assists" matches={stats.matches_played} />
      </>
    ) : undefined

  const breakdownEl = !stats ? null : 'runs' in stats ? (
    <section className="flex flex-col gap-3">
      <SectionHeading>Batting &amp; bowling</SectionHeading>
      <Card className="grid grid-cols-3 gap-3 p-[18px] sm:grid-cols-5">
        <StatTile value={stats.runs} label="Runs" />
        <StatTile value={stats.wickets} label="Wickets" />
        <StatTile value={stats.fours} label="Fours" />
        <StatTile value={stats.sixes} label="Sixes" />
        <StatTile value={stats.catches} label="Catches" />
      </Card>
      <Card className="grid grid-cols-3 gap-3 p-[18px]">
        <StatTileWithInfo
          value={stats.batting_average?.toFixed(1) ?? '-'}
          label="Average"
          info="Runs scored per dismissal — the traditional batting average. Shows “-” until you've been out at least once."
        />
        <StatTileWithInfo
          value={stats.strike_rate?.toFixed(0) ?? '-'}
          label="Strike rate"
          info="Runs scored per 100 balls faced."
        />
        <StatTileWithInfo
          value={stats.economy?.toFixed(1) ?? '-'}
          label="Economy"
          info="Runs conceded per over bowled — lower is better."
        />
      </Card>
    </section>
  ) : 'goals' in stats ? (
    <section className="flex flex-col gap-3 xl:hidden">
      <SectionHeading>Goals &amp; assists</SectionHeading>
      <Card className="grid grid-cols-2 gap-4 p-[18px]">
        <PerMatchStat value={stats.goals} label="Goals" matches={stats.matches_played} />
        <PerMatchStat value={stats.assists} label="Assists" matches={stats.matches_played} />
      </Card>
    </section>
  ) : null

  const personalBestsEl = !stats ? null : 'runs' in stats ? (
    <section className="flex flex-col gap-3">
      <SectionHeading>Personal bests</SectionHeading>
      <PersonalBestsCard
        rows={[{ label: 'High score', unit: 'runs', figure: stats.best_runs }]}
        bowlingFigure={stats.best_bowling}
        onOpen={(id) => navigate(`/matches/${id}`)}
      />
    </section>
  ) : 'goals' in stats ? (
    <section className="flex flex-col gap-3">
      <SectionHeading>Personal bests</SectionHeading>
      <PersonalBestsCard
        rows={[
          { label: 'Most goals in a match', unit: 'goals', figure: stats.best_goals },
          { label: 'Most goals + assists', sub: 'in one match', figure: stats.best_goal_contributions },
        ]}
        onOpen={(id) => navigate(`/matches/${id}`)}
      />
    </section>
  ) : null

  const chartEl = (
    <section className="flex flex-col gap-3">
      <SectionHeading>Matches per month</SectionHeading>
      <Card className="flex flex-col gap-3 p-[18px]">
        <MatchActivityChart data={chartData} />
        <span className="text-[13px] text-muted-foreground">
          {busiest
            ? (
              <>
                Busiest month: <b className="font-semibold text-foreground">{busiest.fullLabel.split(' ')[0]}</b> with{' '}
                {busiest.count} match{busiest.count === 1 ? '' : 'es'}
              </>
            )
            : `No matches in the last ${CHART_MONTHS} months`}
        </span>
      </Card>
    </section>
  )

  const recentMatchesEl = (
    <RecentMatches
      query={matchesQuery}
      matches={recentByDate}
      search={query}
      onSearch={setQuery}
      filter={filter}
      onFilter={setFilter}
      currentUserId={currentUserId}
      navigate={navigate}
    />
  )

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 xl:max-w-[1080px]">
      <header className="flex flex-col gap-3 pt-1">
        <Button
          variant="ghost"
          className="hidden h-11 w-fit gap-1.5 rounded-full pl-2 xl:flex"
          onClick={() => navigate(isOwnProfile ? '/profile' : `/users/${userId}`)}
        >
          <ChevronLeft className="size-5" />
          Profile
        </Button>
        <div className="flex items-center gap-2 xl:gap-3.5">
          <Button
            variant="ghost"
            size="icon"
            className="size-11 rounded-full xl:hidden"
            aria-label="Back"
            onClick={() => navigate(-1)}
          >
            <ChevronLeft className="size-5" />
          </Button>
          <span className="flex size-10 items-center justify-center rounded-full bg-accent xl:size-[52px]">
            <Icon className="size-[22px] text-accent-foreground xl:size-7" />
          </span>
          <div className="flex flex-col">
            <h1 className="font-display text-2xl leading-tight font-extrabold xl:text-[30px]">
              {sportLabel(matchType)}
            </h1>
            <span className="text-[13px] text-muted-foreground xl:text-sm">
              {isOwnProfile ? 'Your stats' : `${profile.name}'s stats`}
              {stats && <span className="hidden xl:inline"> · {stats.matches_played} matches</span>}
            </span>
          </div>
        </div>
      </header>

      {!stats ? (
        <Card className="p-6 text-center text-sm text-muted-foreground">
          No confirmed {sportLabel(matchType).toLowerCase()} matches yet.
        </Card>
      ) : (
        <>
          {/* Mobile / tablet: everything in a single column, in the order
              verified against the "Sport" board's mobile mock. */}
          <div className="flex flex-col gap-4 xl:hidden">
            <WinRateBanner stats={stats} recentOutcomes={recentByDate.map((m) => m.outcome)} />
            {breakdownEl}
            {personalBestsEl}
            {chartEl}
            {recentMatchesEl}
          </div>

          {/* Desktop (`xl`, ≥1280px): the "DesktopSport" board's two-column
              layout — win rate + breakdown + recent matches on the left,
              personal bests + activity chart in a fixed-width rail on the
              right. Grid placement (not DOM order) does the reordering, so
              this reuses the same subcomponents/state as the mobile column
              above rather than duplicating them. */}
          <div className="hidden xl:grid xl:grid-cols-[minmax(0,1fr)_420px] xl:items-start xl:gap-6">
            <div className="flex min-w-0 flex-col gap-4">
              <WinRateBanner
                stats={stats}
                recentOutcomes={recentByDate.map((m) => m.outcome)}
                extra={bannerExtra}
              />
              {'runs' in stats && breakdownEl}
              {recentMatchesEl}
            </div>
            <div className="flex flex-col gap-4">
              {personalBestsEl}
              {chartEl}
            </div>
          </div>
        </>
      )}
    </div>
  )
}

function sortedByDate(matches: SearchMatch[] | undefined): SearchMatch[] {
  return [...(matches ?? [])].sort(
    (a, b) => new Date(b.starts_at).getTime() - new Date(a.starts_at).getTime(),
  )
}

function SectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="px-1 font-display text-[19px] font-bold">{children}</h2>
  )
}

/** The hero card at the top of the page: win rate, the won/drawn/lost
 *  stacked bar and tiles, and a "recent form" row of the last few outcomes —
 *  the "Sport" board's stats card, generalised past football's W/D/L. */
function WinRateBanner({
  stats,
  recentOutcomes,
  extra,
}: {
  stats: { matches_played: number; wins: number; draws: number; losses: number; win_percentage?: number | null }
  recentOutcomes: (MatchOutcome | undefined)[]
  /** Extra stat tiles shown beside the win rate on desktop only (the
   *  "DesktopSport" board folds football's goals/assists into this card) —
   *  hidden below `xl` regardless of what's passed. */
  extra?: React.ReactNode
}) {
  const form = recentOutcomes.filter((o): o is MatchOutcome => !!o).slice(0, FORM_LIMIT)

  return (
    <Card className="flex flex-col gap-4 p-[18px] xl:gap-5 xl:p-6">
      <div className="flex items-end justify-between gap-6">
        <div className="flex flex-col gap-0.5">
          <span className="hidden text-[13px] font-semibold text-muted-foreground xl:block">Win rate</span>
          <span className="font-display text-5xl leading-none font-extrabold text-primary xl:text-7xl xl:tracking-tight">
            {formatWinRate(stats.win_percentage)}
          </span>
          <span className="text-sm text-muted-foreground">
            <span className="xl:hidden">win rate </span>
            from {stats.matches_played} match{stats.matches_played === 1 ? '' : 'es'}
          </span>
        </div>
        {extra && <div className="hidden items-end gap-7 xl:flex">{extra}</div>}
      </div>

      <div className="flex h-2.5 gap-[3px] overflow-hidden rounded-full">
        <span className="rounded-full bg-primary" style={{ flexGrow: stats.wins || 0.0001 }} />
        <span className="rounded-full bg-destructive" style={{ flexGrow: stats.losses || 0.0001 }} />
        <span className="rounded-full bg-muted" style={{ flexGrow: stats.draws || 0.0001 }} />
      </div>

      <div className="flex flex-col gap-3.5 xl:flex-row xl:items-center xl:gap-7">
        <div className="grid grid-cols-3 gap-2 xl:flex xl:gap-7">
          <StatTile value={stats.wins} label="Won" />
          <StatTile value={stats.draws} label="Drawn" />
          <StatTile value={stats.losses} label="Lost" />
        </div>

        {form.length > 0 && (
          <div className="flex items-center gap-3 border-t pt-3.5 xl:flex-grow xl:justify-end xl:border-t-0 xl:pt-0">
            <span className="text-sm font-semibold text-muted-foreground">Recent form</span>
            <div className="flex gap-1.5">
              {form.map((outcome, i) => <FormBadge key={i} outcome={outcome} />)}
            </div>
          </div>
        )}
      </div>
    </Card>
  )
}

function FormBadge({ outcome }: { outcome: MatchOutcome }) {
  const letter = outcome === 'won' ? 'W' : outcome === 'lost' ? 'L' : 'D'
  return (
    <span
      aria-label={outcome}
      className={cn(
        'flex size-[34px] items-center justify-center rounded-[10px] text-sm font-extrabold',
        outcome === 'won' && 'bg-primary text-primary-foreground',
        outcome === 'lost' && 'border-2 border-destructive bg-card text-destructive',
        outcome === 'draw' && 'bg-muted text-muted-foreground',
      )}
    >
      {letter}
    </span>
  )
}

/** A `StatTile` with a "?" explanation next to its label — for a derived
 *  cricket stat whose name alone doesn't say how it's computed. Composed
 *  locally rather than widening the shared `StatTile`'s `label` prop, which
 *  every other caller passes a plain string. */
function StatTileWithInfo({ value, label, info }: { value: React.ReactNode; label: string; info: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-display text-3xl leading-none font-extrabold">{value}</span>
      <span className="flex items-center gap-1 text-xs text-muted-foreground">
        {label}
        <StatInfo>{info}</StatInfo>
      </span>
    </div>
  )
}

function PerMatchStat({ value, label, matches }: { value: number; label: string; matches: number }) {
  const perMatch = matches > 0 ? (value / matches).toFixed(1) : '0.0'
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-display text-4xl leading-none font-extrabold">{value}</span>
      <span className="text-sm font-semibold">{label}</span>
      <span className="text-[13px] text-muted-foreground">{perMatch} per match</span>
    </div>
  )
}

interface PersonalBestRow {
  label: React.ReactNode
  sub?: string
  unit?: string
  figure: BestFigure | undefined
}

/** "Personal bests" list card — a row per counter with a match to link to,
 *  plus (for cricket) the best bowling spell, which needs its own richer row
 *  (wickets/runs/overs, not just a bare number). Renders nothing if every
 *  figure passed in is still unset. */
function PersonalBestsCard({
  rows,
  bowlingFigure,
  onOpen,
}: {
  rows: PersonalBestRow[]
  bowlingFigure?: BestBowlingFigures
  onOpen: (matchId: string) => void
}) {
  const populated = rows.filter((r) => r.figure)
  if (populated.length === 0 && !bowlingFigure) return null

  return (
    <Card className="flex flex-col divide-y overflow-hidden p-0">
      {populated.map((row, i) => (
        <button
          key={i}
          type="button"
          onClick={() => row.figure && onOpen(row.figure.match_id)}
          className="flex min-h-16 items-center gap-3.5 px-4 py-3.5 text-left"
        >
          <span className="w-12 font-display text-3xl leading-none font-extrabold">{row.figure!.value}</span>
          <span className="flex flex-grow flex-col gap-0.5">
            <span className="text-[15px] font-bold">{row.label}</span>
            <span className="text-[13px] text-muted-foreground">{row.sub ?? row.unit}</span>
          </span>
          <span className="text-sm font-bold text-primary">View match</span>
        </button>
      ))}
      {bowlingFigure && (
        <button
          type="button"
          onClick={() => onOpen(bowlingFigure.match_id)}
          className="flex min-h-16 items-center gap-3.5 px-4 py-3.5 text-left"
        >
          <span className="w-12 font-display text-2xl leading-none font-extrabold">
            {bowlingFigure.wickets}/{bowlingFigure.runs_conceded}
          </span>
          <span className="flex flex-grow flex-col gap-0.5">
            <span className="text-[15px] font-bold">Best bowling</span>
            <span className="text-[13px] text-muted-foreground">{formatOvers(bowlingFigure.overs)} overs</span>
          </span>
          <span className="text-sm font-bold text-primary">View match</span>
        </button>
      )}
    </Card>
  )
}

/** "26 Aug" style short date, matching the mock's recent-matches subtitle. */
function shortDate(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })
}

function outcomeBadge(match: SearchMatch): { label: string; bg: string; fg: string; small?: boolean } {
  if (match.status !== 'completed' || !match.outcome) {
    return { label: 'TBC', bg: 'bg-muted', fg: 'text-muted-foreground', small: true }
  }
  if (match.outcome === 'won') return { label: 'W', bg: 'bg-accent', fg: 'text-accent-foreground' }
  if (match.outcome === 'lost') return { label: 'L', bg: 'bg-destructive/15', fg: 'text-destructive' }
  return { label: 'D', bg: 'bg-muted', fg: 'text-muted-foreground' }
}

function matchScoreLabel(match: SearchMatch): string | undefined {
  const score = match.confirmed_score?.score ?? match.pending_score?.score
  if (!score) return undefined
  if (score.type === 'Simple' || score.type === 'Football' || score.type === 'Netball') {
    const values = Object.values(score.type === 'Simple' ? score.entries : score.score)
    if (values.length < 2) return undefined
    return values.join('–')
  }
  return undefined
}

interface RecentMatchesProps {
  query: ReturnType<typeof useQuery<SearchMatch[]>>
  matches: SearchMatch[]
  search: string
  onSearch: (q: string) => void
  filter: ResultFilter
  onFilter: (f: ResultFilter) => void
  currentUserId?: string
  navigate: ReturnType<typeof useNavigate>
}

const FILTERS: { id: ResultFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'won', label: 'Won' },
  { id: 'lost', label: 'Lost' },
  { id: 'drawn', label: 'Drawn' },
]

function RecentMatches({ query, matches, search, onSearch, filter, onFilter, navigate }: RecentMatchesProps) {
  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase()
    return matches.filter((m) => {
      if (filter === 'won' && m.outcome !== 'won') return false
      if (filter === 'lost' && m.outcome !== 'lost') return false
      if (filter === 'drawn' && m.outcome !== 'draw') return false
      if (needle) {
        const haystack = [m.name, m.description, ...m.sides.map((s) => s.name ?? '')]
          .join(' ')
          .toLowerCase()
        if (!haystack.includes(needle)) return false
      }
      return true
    })
  }, [matches, search, filter])

  const shown = filtered.slice(0, RECENT_LIMIT)
  const narrowed = !!search.trim() || filter !== 'all'

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-baseline justify-between px-1">
        <SectionHeading>Recent matches</SectionHeading>
        <span className="text-[13px] text-muted-foreground">
          {narrowed ? `Showing ${filtered.length} of ${matches.length}` : `${matches.length} match${matches.length === 1 ? '' : 'es'}`}
        </span>
      </div>

      <label className="flex h-12 items-center gap-2.5 rounded-2xl border bg-card px-3.5 text-muted-foreground">
        <Search className="size-5 shrink-0" />
        <input
          type="search"
          aria-label="Search matches"
          placeholder="Search by match or team"
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          className="min-w-0 flex-grow border-none bg-transparent text-[15px] font-medium text-foreground outline-none placeholder:text-muted-foreground"
        />
      </label>

      <div role="group" aria-label="Filter matches" className="flex flex-wrap gap-2">
        {FILTERS.map((f) => (
          <Chip key={f.id} pressed={filter === f.id} onClick={() => onFilter(f.id)}>
            {f.label}
          </Chip>
        ))}
      </div>

      {query.isLoading ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 2 }).map((_, i) => (
            <div key={i} className="h-16 animate-pulse rounded-2xl border bg-card" aria-hidden />
          ))}
        </div>
      ) : query.isError ? (
        <Card className="flex flex-col items-center gap-3 p-6 text-center">
          <p className="text-sm text-muted-foreground">Couldn't load recent matches.</p>
          <Button variant="outline" size="sm" onClick={() => query.refetch()}>
            Retry
          </Button>
        </Card>
      ) : (
        <Card className="flex flex-col divide-y overflow-hidden p-0">
          {shown.map((match) => {
            const badge = outcomeBadge(match)
            const [sideA, sideB] = match.sides
            const opponent = [sideA?.name, sideB?.name].filter(Boolean).join(' vs ')
            const score = matchScoreLabel(match)
            return (
              <button
                key={match.id}
                type="button"
                onClick={() => navigate(`/matches/${match.id}`)}
                className="flex min-h-16 items-center gap-3 px-4 py-3 text-left"
              >
                <span
                  className={cn(
                    'flex size-8 shrink-0 items-center justify-center rounded-[10px] font-extrabold tracking-wide',
                    badge.bg,
                    badge.fg,
                    badge.small ? 'text-[10px]' : 'text-[13px]',
                  )}
                >
                  {badge.label}
                </span>
                <span className="flex min-w-0 flex-grow flex-col gap-0.5">
                  <span className="truncate text-[15px] font-semibold">{match.name}</span>
                  <span className="truncate text-[13px] text-muted-foreground">
                    {opponent}
                    {opponent && ' · '}
                    {shortDate(match.starts_at)}
                    {match.status !== 'completed' && ' · awaiting confirmation'}
                  </span>
                </span>
                {score && <span className="font-display text-lg font-extrabold">{score}</span>}
              </button>
            )
          })}
          {shown.length === 0 && (
            <div className="flex flex-col items-center gap-2 px-5 py-7 text-center">
              <span className="text-[15px] font-bold">No matches found</span>
              <button
                type="button"
                onClick={() => {
                  onSearch('')
                  onFilter('all')
                }}
                className="flex h-10 items-center rounded-full bg-accent px-4 text-sm font-bold text-accent-foreground"
              >
                Clear search and filters
              </button>
            </div>
          )}
        </Card>
      )}
    </section>
  )
}

/** Bucket matches into the last `CHART_MONTHS` trailing calendar months
 *  (oldest first), zero-filling months with no matches so gaps are visible
 *  rather than skipped. */
function monthlyActivity(matches: SearchMatch[] | undefined): MonthlyActivityPoint[] {
  const now = new Date()
  const buckets: MonthlyActivityPoint[] = []
  for (let i = CHART_MONTHS - 1; i >= 0; i--) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1)
    buckets.push({
      label: MONTH_LABELS[d.getMonth()],
      fullLabel: `${MONTH_FULL_LABELS[d.getMonth()]} ${d.getFullYear()}`,
      count: 0,
    })
  }
  const oldestBucketStart = new Date(now.getFullYear(), now.getMonth() - (CHART_MONTHS - 1), 1)

  for (const match of matches ?? []) {
    const started = new Date(match.starts_at)
    if (Number.isNaN(started.getTime()) || started < oldestBucketStart) continue
    const monthsAgo =
      (now.getFullYear() - started.getFullYear()) * 12 + (now.getMonth() - started.getMonth())
    const index = CHART_MONTHS - 1 - monthsAgo
    if (index >= 0 && index < buckets.length) {
      buckets[index].count += 1
    }
  }
  return buckets
}

function busiestMonth(data: MonthlyActivityPoint[]): MonthlyActivityPoint | null {
  let best: MonthlyActivityPoint | null = null
  for (const d of data) {
    if (d.count > 0 && (!best || d.count > best.count)) best = d
  }
  return best
}

/** Placeholder while the profile loads. */
function SportStatsSkeleton() {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="h-10 w-40 animate-pulse rounded-full bg-card" aria-hidden />
      <div className="h-56 animate-pulse rounded-2xl border bg-card" aria-hidden />
      <div className="h-40 animate-pulse rounded-2xl border bg-card" aria-hidden />
    </div>
  )
}
