import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Pencil, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { TeamFollowButton } from '@/components/agon/TeamFollowButton'
import { EditTeamDialog } from '@/components/agon/EditTeamDialog'
import { InviteToTeamDialog } from '@/components/agon/InviteToTeamDialog'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { memberName, memberAvatarUrl } from '@/lib/members'

type Team = components['schemas']['Team']
type TeamMember = components['schemas']['TeamMember']
type SearchMatch = components['schemas']['SearchMatch']

/** Recent-activity matches to fetch/show, same limit as the profile page's. */
const RECENT_LIMIT = 5

/**
 * A team's page: logo/name/follower count and a follow toggle (for a
 * non-member viewer), its member list, and its recent matches (`GET
 * /matches?team_id=`, the same discovery endpoint the profile page's
 * activity section uses with `participant` instead). An admin viewer also
 * gets edit and invite entry points. Mirrors `ProfilePage`'s structure —
 * header, then stacked sections — so a team's page reads as the same kind of
 * thing as a person's.
 */
export function TeamPage() {
  const { teamId } = useParams()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const teamQuery = useQuery({
    queryKey: ['team', teamId],
    enabled: !!teamId,
    queryFn: async (): Promise<Team> => {
      const { data, error } = await fetchClient.GET('/teams/{team_id}', {
        params: { path: { team_id: teamId! } },
      })
      if (error || !data) throw new Error('Failed to load team')
      return data
    },
  })

  const activityQuery = useQuery({
    queryKey: ['team-activity', teamId],
    enabled: !!teamId,
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { team_id: [teamId!], limit: RECENT_LIMIT } },
      })
      if (error || !data) throw new Error('Failed to load recent matches')
      return data.items
    },
  })

  if (teamQuery.isLoading) {
    return <TeamSkeleton />
  }

  if (teamQuery.isError || !teamQuery.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this team.</p>
        <Button variant="outline" onClick={() => teamQuery.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const team = teamQuery.data
  const isAdmin = team.members.some(
    (m) =>
      m.member.type === 'User' &&
      m.member.user_id === currentUserId &&
      m.role === 'admin',
  )
  const existingUserIds = team.members
    .map((m) => (m.member.type === 'User' ? m.member.user_id : null))
    .filter((id): id is string => id !== null)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-8">
      <Button
        variant="ghost"
        size="icon"
        className="size-8 self-start"
        aria-label="Back"
        onClick={() => navigate(-1)}
      >
        <ChevronLeft className="size-4" />
      </Button>

      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-4">
          <Avatar name={team.name} imageUrl={team.logo?.image_url} size="xl" />
          <div className="min-w-0">
            <h1 className="truncate text-xl font-semibold">{team.name}</h1>
            <p className="text-sm text-muted-foreground">
              {team.follower_count.toLocaleString()}{' '}
              {team.follower_count === 1 ? 'follower' : 'followers'}
            </p>
          </div>
        </div>

        <div className="flex gap-2">
          {isAdmin ? (
            <>
              <EditTeamDialog team={team}>
                <Button variant="outline" className="gap-2">
                  <Pencil className="size-4" />
                  Edit team
                </Button>
              </EditTeamDialog>
              <InviteToTeamDialog teamId={team.id} excludeUserIds={existingUserIds}>
                <Button variant="outline" className="gap-2">
                  <UserPlus className="size-4" />
                  Invite
                </Button>
              </InviteToTeamDialog>
            </>
          ) : (
            <TeamFollowButton teamId={team.id} isFollowing={team.is_followed_by_me} />
          )}
        </div>
      </div>

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Members
        </h2>
        <ul className="flex flex-col divide-y overflow-hidden rounded-xl border bg-card">
          {team.members.map((member) => (
            <li key={member.member.id}>
              <MemberRow member={member} currentUserId={currentUserId} />
            </li>
          ))}
        </ul>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Recent matches
        </h2>
        <RecentMatches query={activityQuery} currentUserId={currentUserId} />
      </section>
    </div>
  )
}

/** One row in the member list: avatar, name, role, and a pending-invite badge
 *  for someone who hasn't accepted yet. */
function MemberRow({
  member,
  currentUserId,
}: {
  member: TeamMember
  currentUserId?: string
}) {
  const isYou = member.member.type === 'User' && member.member.user_id === currentUserId
  const pending = member.member.invitation?.status === 'pending'

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <Avatar
        name={memberName(member.member)}
        imageUrl={memberAvatarUrl(member.member)}
        size="lg"
        ring={isYou ? 'you' : 'none'}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{memberName(member.member)}</div>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className="capitalize">{member.role}</span>
          {isYou && <span className="text-primary">· you</span>}
          {pending && <span>· invited</span>}
        </div>
      </div>
    </div>
  )
}

interface RecentMatchesProps {
  query: ReturnType<typeof useQuery<SearchMatch[]>>
  currentUserId?: string
}

/** Recent-matches list: loading / error / empty states, else the match cards
 *  — same states as `ProfilePage`'s `RecentActivity`. */
function RecentMatches({ query, currentUserId }: RecentMatchesProps) {
  const navigate = useNavigate()

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
          Couldn't load recent matches.
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
        No matches yet.
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

/** Placeholder while the team loads. */
function TeamSkeleton() {
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
