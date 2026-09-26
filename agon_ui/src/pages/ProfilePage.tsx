import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { LogOut, Search, Settings, Share2, Users, Watch } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { EditProfileDialog } from '@/components/agon/EditProfileDialog'
import { FollowButton } from '@/components/agon/FollowButton'
import { StatBanner } from '@/components/agon/StatBanner'
import { SportProgressRow } from '@/components/agon/SportProgressRow'
import { ProfileMatchRow } from '@/components/agon/ProfileMatchRow'
import { Chip } from '@/components/agon/Chip'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/ui/sheet'
import { ThemeToggle } from '@/components/ThemeToggle'
import { useAuth } from '@/hooks/useAuth'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { useMatchesTogether } from '@/hooks/useMatchesTogether'
import { sortedByActivity, totalMatches, overallWinRate, formatWinRate } from '@/lib/stats'
import { sportLabel, type MatchType } from '@/lib/sports'

type UserProfile = components['schemas']['UserProfile']
type SearchMatch = components['schemas']['SearchMatch']

/** Matches fetched per list — generous enough for the chip/search filtering
 *  below to work over a real slice of history without paging. */
const MATCHES_LIMIT = 50

/**
 * The profile page, serving both the viewer's own profile (`/profile`, via
 * `GET /users/me`) and another user's (`/users/:userId`, via
 * `GET /users/{user_id}`) — restyled to the "Agon redesign" canvas's Profile
 * board, including its "viewing someone else" state (head-to-head +
 * playing-together records, folded into this same page rather than a
 * separate screen).
 */
export function ProfilePage() {
  const { userId } = useParams()
  const navigate = useNavigate()
  const isOwnProfile = !userId
  const currentUserId = useCurrentUserId()

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

  if (profileQuery.isLoading) return <ProfileSkeleton />

  if (profileQuery.isError || !profileQuery.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this profile.</p>
        <Button variant="outline" onClick={() => profileQuery.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const profile = profileQuery.data

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <header className="flex items-center justify-between">
        <h1 className="font-display text-[22px] font-extrabold">Profile</h1>
        {isOwnProfile && <SettingsSheet />}
      </header>

      {isOwnProfile ? (
        <OwnProfile profile={profile} onOpenMatch={(id) => navigate(`/matches/${id}`)} />
      ) : (
        <OtherProfile
          profile={profile}
          currentUserId={currentUserId}
          onOpenMatch={(id) => navigate(`/matches/${id}`)}
        />
      )}
    </div>
  )
}

/** Gear-icon button opening the account settings that used to live inline at
 *  the bottom of this page — sign-out, theme and the mobile-only Teams/paired
 *  devices links (desktop still has the sidebar for those). */
function SettingsSheet() {
  const { signOut } = useAuth()
  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon" className="rounded-full" aria-label="Settings">
          <Settings className="size-[22px]" />
        </Button>
      </SheetTrigger>
      <SheetContent side="bottom" className="rounded-t-2xl">
        <SheetHeader>
          <SheetTitle>Account</SheetTitle>
        </SheetHeader>
        <div className="flex flex-col gap-2 pt-2 md:hidden">
          <Button variant="ghost" className="justify-start gap-2" asChild>
            <Link to="/teams">
              <Users className="size-4" /> Teams
            </Link>
          </Button>
        </div>
        <div className="flex items-center justify-between gap-2 rounded-xl border bg-card p-3">
          <span className="text-sm text-muted-foreground">Appearance</span>
          <ThemeToggle />
        </div>
        <Button variant="outline" className="w-full justify-start gap-2" asChild>
          <Link to="/devices">
            <Watch className="size-4" /> Paired devices
          </Link>
        </Button>
        <Button variant="outline" className="w-full gap-2" onClick={signOut}>
          <LogOut className="size-4" /> Sign out
        </Button>
      </SheetContent>
    </Sheet>
  )
}

/** Shares this profile's link via the native share sheet, falling back to a
 *  clipboard copy — same fallback chain as `ShareMatchButton` on `MatchCard`. */
function shareProfile(profile: UserProfile) {
  const url = `${window.location.origin}/users/${profile.id}`
  if (navigator.share) {
    navigator.share({ title: profile.name, url }).catch(() => {})
    return
  }
  navigator.clipboard?.writeText(url).catch(() => window.prompt('Copy this profile link:', url))
}

const MATCH_FILTERS: { id: string; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'won', label: 'Won' },
  { id: 'lost', label: 'Lost' },
]

/** Client-side filter over an already-fetched match page: sport chips are
 *  derived from what's actually in the list, `won`/`lost` chips from each
 *  match's `outcome` (resolved server-side for the `participant` this list
 *  is scoped to), and free text over the match name. */
function filterMatches(matches: SearchMatch[], filter: string, q: string): SearchMatch[] {
  const needle = q.trim().toLowerCase()
  return matches.filter((m) => {
    if (filter === 'won' && m.outcome !== 'won') return false
    if (filter === 'lost' && m.outcome !== 'lost') return false
    if (filter !== 'all' && filter !== 'won' && filter !== 'lost' && m.match_type !== filter) return false
    if (needle && !m.name.toLowerCase().includes(needle)) return false
    return true
  })
}

/** The sports actually present in a match list, in first-seen order — used
 *  to build the sport filter chips without hardcoding a sport list. */
function sportsIn(matches: SearchMatch[]): MatchType[] {
  const seen: MatchType[] = []
  for (const m of matches) {
    if (!seen.includes(m.match_type)) seen.push(m.match_type)
  }
  return seen
}

function OwnProfile({
  profile,
  onOpenMatch,
}: {
  profile: UserProfile
  onOpenMatch: (id: string) => void
}) {
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState('all')

  const matchesQuery = useQuery({
    queryKey: ['profile-matches', profile.id],
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { participant: profile.id, limit: MATCHES_LIMIT } },
      })
      if (error || !data) throw new Error('Failed to load matches')
      return data.items
    },
  })

  const matches = useMemo(() => matchesQuery.data ?? [], [matchesQuery.data])
  const sports = useMemo(() => sportsIn(matches), [matches])
  const filtered = useMemo(() => filterMatches(matches, filter, q), [matches, filter, q])
  const narrowed = filter !== 'all' || q.trim() !== ''

  const matches_played = totalMatches(profile.stats)
  const winRate = overallWinRate(profile.stats)
  const sportRows = sortedByActivity(profile.stats)

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <Avatar
          name={profile.name}
          imageUrl={profile.profile_image?.image_url}
          size="xl"
          ring="you"
          className="size-[84px] text-2xl"
        />
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="truncate font-display text-2xl font-extrabold tracking-tight">
            {profile.name}
          </h2>
          <p className="text-sm text-muted-foreground">
            <Link to={`/users/${profile.id}/followers`} className="font-bold text-foreground hover:underline">
              {profile.follower_count}
            </Link>{' '}
            followers &middot;{' '}
            <Link to={`/users/${profile.id}/following`} className="font-bold text-foreground hover:underline">
              {profile.following_count}
            </Link>{' '}
            following
          </p>
        </div>
      </div>

      <div className="flex gap-2.5">
        <EditProfileDialog profile={profile}>
          <Button variant="outline" shape="pill" className="h-11 flex-grow font-bold">
            Edit profile
          </Button>
        </EditProfileDialog>
        <Button
          variant="outline"
          shape="pill"
          className="h-11 flex-grow gap-2 font-bold"
          onClick={() => shareProfile(profile)}
        >
          <Share2 className="size-4" /> Share profile
        </Button>
      </div>

      <StatBanner
        aria-label="All sports"
        stats={[
          { value: matches_played, label: 'Matches' },
          { value: totalWins(profile), label: 'Wins' },
          { value: formatWinRate(winRate), label: 'Win rate' },
        ]}
      />

      {sportRows.length > 0 && (
        <>
          <h3 className="px-1 pt-2 font-display text-[19px] font-bold">Your sports</h3>
          <Card className="flex flex-col overflow-hidden">
            {sportRows.map(({ sport, stats }, i) => (
              <SportProgressRow
                key={sport}
                sport={sport}
                matchesPlayed={stats.matches_played}
                winPercentage={stats.win_percentage}
                to={`/profile/stats/${sport}`}
                isFirst={i === 0}
              />
            ))}
          </Card>
        </>
      )}

      <div className="flex items-baseline justify-between px-1 pt-2">
        <h3 className="font-display text-[19px] font-bold">Your matches</h3>
        <span className="text-[13px] text-muted-foreground">
          {narrowed ? `Showing ${filtered.length} of ${matches.length}` : `${matches.length} matches`}
        </span>
      </div>
      <label className="flex h-12 items-center gap-2.5 rounded-2xl border bg-card px-3.5 text-muted-foreground [&:has(input:focus)]:ring-1 [&:has(input:focus)]:ring-ring">
        <Search className="size-5 shrink-0" />
        <input
          type="search"
          aria-label="Search your matches"
          placeholder="Search by match name"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          className="min-w-0 flex-grow bg-transparent text-[15px] font-medium text-foreground outline-none placeholder:text-muted-foreground"
        />
      </label>
      <div role="group" aria-label="Filter matches" className="flex flex-wrap gap-2">
        {[...MATCH_FILTERS, ...sports.map((s) => ({ id: s, label: sportLabel(s) }))].map((c) => (
          <Chip key={c.id} pressed={filter === c.id} onClick={() => setFilter(c.id)}>
            {c.label}
          </Chip>
        ))}
      </div>
      <MatchList
        isLoading={matchesQuery.isLoading}
        isError={matchesQuery.isError}
        matches={filtered}
        onOpen={onOpenMatch}
        onRetry={() => matchesQuery.refetch()}
        onReset={() => {
          setQ('')
          setFilter('all')
        }}
      />
    </div>
  )
}

function totalWins(profile: UserProfile): number {
  const stats = profile.stats
  return sortedByActivity(stats).reduce((sum, s) => sum + s.stats.wins, 0)
}

const H2H_FILTERS: { id: string; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'against', label: 'As opponents' },
  { id: 'with', label: 'As teammates' },
]

function OtherProfile({
  profile,
  currentUserId,
  onOpenMatch,
}: {
  profile: UserProfile
  currentUserId: string | undefined
  onOpenMatch: (id: string) => void
}) {
  const [filter, setFilter] = useState('all')
  const { isLoading, isError, together, headToHead, playingTogether, refetch } =
    useMatchesTogether(currentUserId, profile.id)

  const sports = useMemo(() => sportsIn(together.map((t) => t.match)), [together])
  const filtered = useMemo(
    () =>
      together.filter((t) => {
        if (filter === 'against' && t.kind !== 'against') return false
        if (filter === 'with' && t.kind !== 'with') return false
        if (filter !== 'all' && filter !== 'against' && filter !== 'with' && t.match.match_type !== filter)
          return false
        return true
      }),
    [together, filter],
  )

  const matches_played = totalMatches(profile.stats)
  const winRate = overallWinRate(profile.stats)

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <Avatar
          name={profile.name}
          imageUrl={profile.profile_image?.image_url}
          size="xl"
          className="size-[84px] text-2xl"
        />
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="truncate font-display text-2xl font-extrabold tracking-tight">
            {profile.name}
          </h2>
          <p className="text-sm text-muted-foreground">
            <Link to={`/users/${profile.id}/followers`} className="font-bold text-foreground hover:underline">
              {profile.follower_count}
            </Link>{' '}
            followers &middot;{' '}
            <Link to={`/users/${profile.id}/following`} className="font-bold text-foreground hover:underline">
              {profile.following_count}
            </Link>{' '}
            following
          </p>
        </div>
      </div>

      <div className="flex gap-2.5">
        <FollowButton
          userId={profile.id}
          isFollowing={profile.is_followed_by_me}
          shape="pill"
          className="h-11 flex-grow font-bold"
        />
      </div>

      <StatBanner
        aria-label={`${profile.name}'s overall stats`}
        stats={[
          { value: matches_played, label: 'Matches' },
          { value: totalWins(profile), label: 'Wins' },
          { value: formatWinRate(winRate), label: 'Win rate' },
        ]}
      />

      <h3 className="px-1 pt-2 font-display text-[19px] font-bold">Head to head</h3>
      <StatBanner
        aria-label="Record as opponents"
        stats={[
          { value: headToHead.youWon, label: 'You won' },
          { value: headToHead.draws, label: 'Draws' },
          { value: headToHead.theyWon, label: `${firstName(profile.name)} won` },
        ]}
        footnote={`${headToHead.total} match${headToHead.total === 1 ? '' : 'es'} played against each other`}
      />

      <h3 className="px-1 pt-2 font-display text-[19px] font-bold">Playing together</h3>
      <StatBanner
        tone="terracotta"
        aria-label="Record as teammates"
        stats={[
          { value: playingTogether.won, label: 'Won' },
          { value: playingTogether.draws, label: 'Draws' },
          { value: playingTogether.lost, label: 'Lost' },
        ]}
        footnote={`${playingTogether.total} match${playingTogether.total === 1 ? '' : 'es'} played on the same side`}
      />

      <div className="flex items-baseline justify-between px-1 pt-2">
        <h3 className="font-display text-[19px] font-bold">Matches together</h3>
        <span className="text-[13px] text-muted-foreground">
          {together.length} match{together.length === 1 ? '' : 'es'}
        </span>
      </div>
      <div role="group" aria-label="Filter matches together" className="flex flex-wrap gap-2">
        {[...H2H_FILTERS, ...sports.map((s) => ({ id: s, label: sportLabel(s) }))].map((c) => (
          <Chip key={c.id} pressed={filter === c.id} onClick={() => setFilter(c.id)}>
            {c.label}
          </Chip>
        ))}
      </div>
      <MatchList
        isLoading={isLoading}
        isError={isError}
        matches={filtered.map((t) => t.match)}
        together={filtered}
        onOpen={onOpenMatch}
        onRetry={refetch}
      />
    </div>
  )
}

function firstName(name: string): string {
  return name.trim().split(/\s+/)[0] ?? name
}

interface MatchListProps {
  isLoading: boolean
  isError: boolean
  matches: SearchMatch[]
  together?: { match: SearchMatch; kind: 'with' | 'against' }[]
  onOpen: (id: string) => void
  onRetry: () => void
  onReset?: () => void
}

/** The white section-card list of match rows — loading/error/empty states,
 *  else `ProfileMatchRow` per match, tagging "Teammates" when `together`
 *  identifies that match as a played-together (not played-against) one. */
function MatchList({ isLoading, isError, matches, together, onOpen, onRetry, onReset }: MatchListProps) {
  if (isLoading) {
    return (
      <div className="flex flex-col gap-2">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="h-[60px] animate-pulse rounded-2xl bg-card" aria-hidden />
        ))}
      </div>
    )
  }

  if (isError) {
    return (
      <Card className="p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load matches.</p>
        <Button variant="outline" size="sm" onClick={onRetry}>
          Retry
        </Button>
      </Card>
    )
  }

  if (matches.length === 0) {
    return (
      <Card className="flex flex-col items-center gap-2 px-5 py-7 text-center">
        <span className="text-[15px] font-bold">No matches found</span>
        {onReset && (
          <Button variant="secondary" shape="pill" size="sm" onClick={onReset}>
            Clear search and filters
          </Button>
        )}
      </Card>
    )
  }

  const kindByMatchId = new Map(together?.map((t) => [t.match.id, t.kind]))

  return (
    <Card className="flex flex-col overflow-hidden">
      {matches.map((m, i) => (
        <ProfileMatchRow
          key={m.id}
          match={m}
          isFirst={i === 0}
          teammates={kindByMatchId.get(m.id) === 'with'}
          onOpen={() => onOpen(m.id)}
        />
      ))}
    </Card>
  )
}

/** Placeholder while the profile loads. */
function ProfileSkeleton() {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center gap-4">
        <div className="size-[84px] animate-pulse rounded-full bg-card" aria-hidden />
        <div className="h-6 w-40 animate-pulse rounded bg-card" aria-hidden />
      </div>
      <div className="h-32 animate-pulse rounded-2xl border bg-card" aria-hidden />
      <div className="h-48 animate-pulse rounded-2xl border bg-card" aria-hidden />
    </div>
  )
}
