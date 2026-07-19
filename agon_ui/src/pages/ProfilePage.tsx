import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronRight, Pencil } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { ProfileHeader } from '@/components/agon/ProfileHeader'
import { EditProfileDialog } from '@/components/agon/EditProfileDialog'
import { FollowButton } from '@/components/agon/FollowButton'
import { SportStatsTable } from '@/components/agon/SportStatsTable'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type UserProfile = components['schemas']['UserProfile']
type Match = components['schemas']['Match']

/** Number of sports shown before the "See all sports" toggle reveals the rest. */
const SPORT_SUMMARY_LIMIT = 3
/** Recent-activity matches to fetch/show. */
const RECENT_LIMIT = 5

/**
 * The profile page, serving both the viewer's own profile (`/profile`, via
 * `GET /users/me`) and another user's (`/users/:userId`, via
 * `GET /users/{user_id}`). When `userId` is present it's someone else's profile:
 * the follow button shows (gated on `is_followed_by_me`) and there's no email.
 *
 * Composed from the shared `ProfileHeader`, `SportStatsTable`, and `MatchCard`.
 * Reads the authenticated fetch client, so it relies on a signed-in session
 * (the app shell only mounts this once auth + profile gates pass).
 */
export function ProfilePage() {
  const { userId } = useParams()
  const navigate = useNavigate()
  const isOwnProfile = !userId
  const currentUserId = useCurrentUserId()
  const [showAllSports, setShowAllSports] = useState(false)

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

  const activityQuery = useQuery({
    queryKey: ['profile-activity', profileId],
    enabled: !!profileId,
    queryFn: async (): Promise<Match[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { participant: profileId, limit: RECENT_LIMIT } },
      })
      if (error || !data) throw new Error('Failed to load recent activity')
      return data.items
    },
  })

  if (profileQuery.isLoading) {
    return <ProfileSkeleton />
  }

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
    <div className="mx-auto flex max-w-xl flex-col gap-8">
      <div className="flex flex-col gap-5">
        <ProfileHeader profile={profile} />
        {isOwnProfile ? (
          <EditProfileDialog profile={profile}>
            <Button variant="outline" className="gap-2">
              <Pencil className="size-4" />
              Edit profile
            </Button>
          </EditProfileDialog>
        ) : (
          userId && (
            <FollowButton
              userId={userId}
              isFollowing={profile.is_followed_by_me}
            />
          )
        )}
      </div>

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Sports
        </h2>
        <SportStatsTable
          stats={profile.stats}
          limit={showAllSports ? undefined : SPORT_SUMMARY_LIMIT}
        />
        {profile.stats.length > SPORT_SUMMARY_LIMIT && (
          <Button
            variant="outline"
            className="mt-1"
            onClick={() => setShowAllSports((v) => !v)}
          >
            {showAllSports ? 'Show less' : 'See all sports'}
            {!showAllSports && <ChevronRight className="size-4" />}
          </Button>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Recent activity
        </h2>
        <RecentActivity
          query={activityQuery}
          onOpen={(id) => navigate(`/matches/${id}`)}
          currentUserId={currentUserId}
        />
      </section>
    </div>
  )
}

interface RecentActivityProps {
  query: ReturnType<typeof useQuery<Match[]>>
  onOpen: (matchId: string) => void
  currentUserId?: string
}

/** Recent-activity list: loading / error / empty states, else the match cards. */
function RecentActivity({ query, onOpen, currentUserId }: RecentActivityProps) {
  if (query.isLoading) {
    return (
      <div className="flex flex-col gap-3">
        {Array.from({ length: 2 }).map((_, i) => (
          <div
            key={i}
            className="h-48 animate-pulse rounded-xl border bg-card"
            aria-hidden
          />
        ))}
      </div>
    )
  }

  if (query.isError) {
    return (
      <div className="rounded-xl border bg-card p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">
          Couldn't load recent activity.
        </p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const matches = query.data ?? []

  if (matches.length === 0) {
    return (
      <div className="rounded-xl border bg-card p-6 text-center text-sm text-muted-foreground">
        No recent matches.
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
          onOpen={() => onOpen(match.id)}
        />
      ))}
    </div>
  )
}

/** Placeholder while the profile loads. */
function ProfileSkeleton() {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-8">
      <div className="flex items-center gap-4">
        <div className="size-16 animate-pulse rounded-full bg-card" aria-hidden />
        <div className="h-6 w-40 animate-pulse rounded bg-card" aria-hidden />
      </div>
      <div className="h-40 animate-pulse rounded-xl border bg-card" aria-hidden />
      <div className="h-48 animate-pulse rounded-xl border bg-card" aria-hidden />
    </div>
  )
}
