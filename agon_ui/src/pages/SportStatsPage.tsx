import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { MatchCard } from '@/components/agon/MatchCard'
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

/** Recent games listed below the chart. */
const RECENT_LIMIT = 10
/** Matches fetched to build the activity chart and recent-games list — the
 *  API caps at 50 either way. */
const FETCH_LIMIT = 50
/** Trailing months shown in the activity chart. */
const CHART_MONTHS = 6

const MONTH_LABELS = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
] as const
const MONTH_FULL_LABELS = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
] as const

/**
 * A single sport's full lifetime stats (`/profile/stats/:sport` or
 * `/users/:userId/stats/:sport`): the outcome breakdown, cricket/football's
 * sport-specific counters and personal bests, a matches-per-month activity
 * chart, and a recent-games list — all driven off the same profile +
 * `GET /matches?participant&match_type` data the profile page's summary row
 * and "recent activity" already use.
 */
export function SportStatsPage() {
  const { userId, sport } = useParams<{ userId?: string; sport: string }>()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()
  const isOwnProfile = !userId

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
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load these stats.</p>
        <Button variant="outline" onClick={() => profileQuery.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const profile = profileQuery.data
  const stats = profile.stats[matchType as keyof typeof profile.stats]
  const Icon = sportIcon(matchType)

  const chartData = monthlyActivity(matchesQuery.data)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-6">
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
        <span className="flex size-8 items-center justify-center rounded-lg bg-accent">
          <Icon className="size-4 text-accent-foreground" />
        </span>
        <h1 className="text-xl font-semibold">
          {sportLabel(matchType)}
          <span className="ml-2 text-sm font-normal text-muted-foreground">
            {isOwnProfile ? 'Your stats' : `${profile.name}'s stats`}
          </span>
        </h1>
      </div>

      {!stats ? (
        <div className="rounded-xl border bg-card p-6 text-center text-sm text-muted-foreground">
          No confirmed {sportLabel(matchType).toLowerCase()} matches yet.
        </div>
      ) : (
        <>
          <section className="grid grid-cols-3 gap-2 sm:grid-cols-5">
            <StatTile value={stats.matches_played} label="Matches" />
            <StatTile value={stats.wins} label="Wins" />
            <StatTile value={stats.draws} label="Draws" />
            <StatTile value={stats.losses} label="Losses" />
            <StatTile value={formatWinRate(stats.win_percentage)} label="Win rate" />
          </section>

          {'runs' in stats && (
            <section className="flex flex-col gap-2">
              <SectionHeading>Batting &amp; bowling</SectionHeading>
              <div className="grid grid-cols-3 gap-2 sm:grid-cols-5">
                <StatTile value={stats.runs} label="Runs" />
                <StatTile value={stats.wickets} label="Wickets" />
                <StatTile value={stats.fours} label="Fours" />
                <StatTile value={stats.sixes} label="Sixes" />
                <StatTile value={stats.catches} label="Catches" />
              </div>
              <div className="grid grid-cols-3 gap-2">
                <StatTile
                  value={stats.batting_average?.toFixed(1) ?? '-'}
                  label="Average"
                  info="Runs scored per dismissal — the traditional batting average. Shows “-” until you've been out at least once."
                />
                <StatTile
                  value={stats.strike_rate?.toFixed(0) ?? '-'}
                  label="Strike rate"
                  info="Runs scored per 100 balls faced."
                />
                <StatTile
                  value={stats.economy?.toFixed(1) ?? '-'}
                  label="Economy"
                  info="Runs conceded per over bowled — lower is better."
                />
              </div>
              <div className="grid grid-cols-2 gap-2">
                <BestFigureCard
                  label="High score"
                  figure={stats.best_runs}
                  onOpen={(id) => navigate(`/matches/${id}`)}
                />
                <BestBowlingCard
                  figure={stats.best_bowling}
                  onOpen={(id) => navigate(`/matches/${id}`)}
                />
              </div>
            </section>
          )}

          {'goals' in stats && (
            <section className="flex flex-col gap-2">
              <SectionHeading>Goals &amp; assists</SectionHeading>
              <div className="grid grid-cols-2 gap-2">
                <StatTile value={stats.goals} label="Goals" />
                <StatTile value={stats.assists} label="Assists" />
              </div>
              <div className="grid grid-cols-2 gap-2">
                <BestFigureCard
                  label="Best game (goals)"
                  figure={stats.best_goals}
                  unit="goals"
                  onOpen={(id) => navigate(`/matches/${id}`)}
                />
                <BestFigureCard
                  label="Best game (goal involvements)"
                  info="Goals scored plus assists, added together, in the same match."
                  figure={stats.best_goal_contributions}
                  onOpen={(id) => navigate(`/matches/${id}`)}
                />
              </div>
            </section>
          )}

          <section className="flex flex-col gap-2">
            <SectionHeading>Activity</SectionHeading>
            <div className="rounded-xl border bg-card p-4 pt-6">
              <MatchActivityChart data={chartData} />
            </div>
            <p className="text-xs text-muted-foreground">
              Matches per month, last {CHART_MONTHS} months
              {matchesQuery.data && matchesQuery.data.length >= FETCH_LIMIT
                ? ` (most recent ${FETCH_LIMIT} matches)`
                : ''}
              .
            </p>
          </section>

          <section className="flex flex-col gap-2">
            <SectionHeading>Recent games</SectionHeading>
            <RecentGames query={matchesQuery} currentUserId={currentUserId} navigate={navigate} />
          </section>
        </>
      )}
    </div>
  )
}

function SectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
      {children}
    </h2>
  )
}

function StatTile({
  value,
  label,
  info,
}: {
  value: string | number
  label: string
  /** Explanation shown behind a "?" next to the label, for a stat whose
   *  name alone doesn't say how it's computed (e.g. "Runs conceded per
   *  over bowled" for Economy). Omitted for self-explanatory stats. */
  info?: React.ReactNode
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-0.5 rounded-xl border bg-card px-2 py-3 text-center">
      <div className="text-lg font-medium">{value}</div>
      <div className="flex items-center gap-1 text-[10px] text-muted-foreground">
        {label}
        {info && <StatInfo>{info}</StatInfo>}
      </div>
    </div>
  )
}

function BestFigureCard({
  label,
  info,
  figure,
  unit,
  onOpen,
}: {
  label: string
  /** Explanation shown behind a "?" next to the label — see `StatTile`. */
  info?: React.ReactNode
  figure: BestFigure | undefined
  /** Value suffix, e.g. "wickets". Omitted for a bare number (runs/goals). */
  unit?: string
  onOpen: (matchId: string) => void
}) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border bg-card p-3">
      <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
        {label}
        {info && <StatInfo>{info}</StatInfo>}
      </span>
      {figure ? (
        <>
          <span className="text-lg font-medium">
            {figure.value}
            {unit && <span className="ml-1 text-xs font-normal text-muted-foreground">{unit}</span>}
          </span>
          <button
            type="button"
            onClick={() => onOpen(figure.match_id)}
            className="text-left text-xs text-primary hover:underline"
          >
            View match
          </button>
        </>
      ) : (
        <span className="text-sm text-muted-foreground">-</span>
      )}
    </div>
  )
}

/** Best single-match bowling spell — a richer card than `BestFigureCard`
 *  since a bowling figure needs runs conceded and overs alongside the
 *  wicket count to mean anything ("5/32 (8.4 overs)", not just "5"). */
function BestBowlingCard({
  figure,
  onOpen,
}: {
  figure: BestBowlingFigures | undefined
  onOpen: (matchId: string) => void
}) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border bg-card p-3">
      <span className="text-[10px] text-muted-foreground">Best bowling</span>
      {figure ? (
        <>
          <span className="text-lg font-medium">
            {figure.wickets}/{figure.runs_conceded}
            <span className="ml-1 text-xs font-normal text-muted-foreground">
              ({formatOvers(figure.overs)} overs)
            </span>
          </span>
          <button
            type="button"
            onClick={() => onOpen(figure.match_id)}
            className="text-left text-xs text-primary hover:underline"
          >
            View match
          </button>
        </>
      ) : (
        <span className="text-sm text-muted-foreground">-</span>
      )}
    </div>
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

interface RecentGamesProps {
  query: ReturnType<typeof useQuery<SearchMatch[]>>
  currentUserId?: string
  navigate: ReturnType<typeof useNavigate>
}

function RecentGames({ query, currentUserId, navigate }: RecentGamesProps) {
  if (query.isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 2 }).map((_, i) => (
          <div
            key={i}
            className={cn('h-48 animate-pulse rounded-xl border bg-card')}
            aria-hidden
          />
        ))}
      </div>
    )
  }

  if (query.isError) {
    return (
      <div className="rounded-xl border bg-card p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load recent games.</p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const matches = (query.data ?? []).slice(0, RECENT_LIMIT)

  if (matches.length === 0) {
    return (
      <div className="rounded-xl border bg-card p-6 text-center text-sm text-muted-foreground">
        No games yet.
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {matches.map((match) => (
        <MatchCard
          key={match.id}
          match={match}
          currentUserId={currentUserId}
          onOpen={() => navigate(`/matches/${match.id}`)}
        />
      ))}
    </div>
  )
}

/** Placeholder while the profile loads. */
function SportStatsSkeleton() {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-6">
      <div className="h-8 w-40 animate-pulse rounded bg-card" aria-hidden />
      <div className="grid grid-cols-5 gap-2">
        {Array.from({ length: 5 }).map((_, i) => (
          <div key={i} className="h-16 animate-pulse rounded-xl border bg-card" aria-hidden />
        ))}
      </div>
      <div className="h-40 animate-pulse rounded-xl border bg-card" aria-hidden />
    </div>
  )
}
