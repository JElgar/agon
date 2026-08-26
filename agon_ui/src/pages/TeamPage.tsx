import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Pencil, ShieldMinus, ShieldPlus, Trash2, UserPlus, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { TeamFollowButton } from '@/components/agon/TeamFollowButton'
import { EditTeamDialog } from '@/components/agon/EditTeamDialog'
import { InviteToTeamDialog } from '@/components/agon/InviteToTeamDialog'
import { DeleteTeamDialog } from '@/components/agon/DeleteTeamDialog'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { memberName, memberAvatarUrl } from '@/lib/members'

type Team = components['schemas']['Team']
type TeamMember = components['schemas']['TeamMember']
type TeamMemberPage = components['schemas']['TeamMemberPage']
type TeamRole = components['schemas']['TeamRole']
type SearchMatch = components['schemas']['SearchMatch']

/** Recent-activity matches to fetch/show, same limit as the profile page's. */
const RECENT_LIMIT = 5
/** Member page size. The API caps at 50; 20 matches its default. */
const MEMBER_PAGE_SIZE = 20

/**
 * A team's page: logo/name/follower count and a follow toggle (for a viewer
 * who isn't an owner/admin), its member list, and its recent matches (`GET
 * /matches?team_id=`, the same discovery endpoint the profile page's
 * activity section uses with `participant` instead). What else the viewer
 * sees depends on their role — owner gets edit/invite/delete plus per-member
 * remove and promote/demote controls, admin gets the same minus delete, a
 * plain member or non-member sees the team read-only (just follow). Mirrors
 * `ProfilePage`'s structure — header, then stacked sections.
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

  // Paginated — see GET /teams/{team_id}/members. A team's page fetches one
  // page up front (like a user's followers list); "myRole"/"already on the
  // team" below only see pages actually fetched so far, which is exactly the
  // first page until "Load more" is used. Fine for a squad-sized team; a
  // (currently hypothetical) team past the first page where the viewer's own
  // row lands later would under-detect their role until they load more.
  const membersQuery = useInfiniteQuery({
    queryKey: ['team-members', teamId],
    enabled: !!teamId,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<TeamMemberPage> => {
      const { data, error } = await fetchClient.GET('/teams/{team_id}/members', {
        params: {
          path: { team_id: teamId! },
          query: { cursor: pageParam, limit: MEMBER_PAGE_SIZE },
        },
      })
      if (error || !data) throw new Error('Failed to load members')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
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
  const members = (membersQuery.data?.pages ?? []).flatMap((page) => page.items)
  const myRole: TeamRole | undefined = members.find(
    (m) => m.member.type === 'User' && m.member.user_id === currentUserId,
  )?.role
  const isOwner = myRole === 'owner'
  const canManage = isOwner || myRole === 'admin'
  const existingUserIds = members
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
          {canManage ? (
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
              {isOwner && (
                <DeleteTeamDialog
                  teamId={team.id}
                  teamName={team.name}
                  onDeleted={() => navigate('/teams')}
                >
                  <Button variant="outline" className="gap-2 text-destructive">
                    <Trash2 className="size-4" />
                    Delete
                  </Button>
                </DeleteTeamDialog>
              )}
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
        <Members
          query={membersQuery}
          members={members}
          currentUserId={currentUserId}
          teamId={team.id}
          canManage={canManage}
        />
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

interface MembersProps {
  query: ReturnType<typeof useInfiniteQuery<TeamMemberPage>>
  members: TeamMember[]
  currentUserId?: string
  teamId: string
  /** Whether the viewer is the team's owner or an admin — gates the
   *  remove/promote/demote controls on each row. */
  canManage: boolean
}

/** The member list: loading / error / empty states, else the rows plus a
 *  "Load more" button — same pagination pattern as `FollowListPage`. */
function Members({ query, members, currentUserId, teamId, canManage }: MembersProps) {
  if (query.isLoading) {
    return (
      <ul className="flex flex-col overflow-hidden rounded-xl border bg-card">
        {Array.from({ length: 3 }).map((_, i) => (
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

  if (query.isError) {
    return (
      <div className="rounded-xl border bg-card p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load members.</p>
        <Button variant="outline" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  return (
    <>
      <ul className="flex flex-col divide-y overflow-hidden rounded-xl border bg-card">
        {members.map((member) => (
          <li key={member.member.id}>
            <MemberRow
              member={member}
              currentUserId={currentUserId}
              teamId={teamId}
              canManage={canManage}
            />
          </li>
        ))}
      </ul>

      {query.hasNextPage && (
        <Button
          variant="outline"
          disabled={query.isFetchingNextPage}
          onClick={() => query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more'}
        </Button>
      )}
    </>
  )
}

/** One row in the member list: avatar, name, role, a pending-invite badge for
 *  someone who hasn't accepted yet, and — for an owner/admin viewer, on
 *  anyone but the team's owner — a promote/demote toggle and a remove
 *  button. Owns its own mutations (mirrors `TeamFollowButton`), invalidating
 *  the members list on success. */
function MemberRow({
  member,
  currentUserId,
  teamId,
  canManage,
}: {
  member: TeamMember
  currentUserId?: string
  teamId: string
  canManage: boolean
}) {
  const queryClient = useQueryClient()
  const isYou = member.member.type === 'User' && member.member.user_id === currentUserId
  const pending = member.member.invitation?.status === 'pending'
  // The owner's role is permanent — the server rejects changing or removing
  // it regardless of caller, so there's nothing for these controls to do on
  // that row even for another owner-equivalent caller (there's only ever one).
  const isOwnerRow = member.role === 'owner'

  const invalidateMembers = () =>
    queryClient.invalidateQueries({ queryKey: ['team-members', teamId] })

  const roleMutation = useMutation({
    mutationFn: async (role: 'admin' | 'member') => {
      const { error } = await fetchClient.PATCH('/teams/{team_id}/members/{member_id}', {
        params: { path: { team_id: teamId, member_id: member.member.id } },
        body: { role },
      })
      if (error) throw new Error('Failed to update role')
    },
    onSuccess: invalidateMembers,
  })

  const removeMutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.DELETE('/teams/{team_id}/members/{member_id}', {
        params: { path: { team_id: teamId, member_id: member.member.id } },
      })
      if (error) throw new Error('Failed to remove member')
    },
    onSuccess: invalidateMembers,
  })

  const busy = roleMutation.isPending || removeMutation.isPending

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

      {canManage && !isOwnerRow && (
        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="size-8"
            disabled={busy}
            aria-label={member.role === 'admin' ? 'Make member' : 'Make admin'}
            title={member.role === 'admin' ? 'Make member' : 'Make admin'}
            onClick={() =>
              roleMutation.mutate(member.role === 'admin' ? 'member' : 'admin')
            }
          >
            {member.role === 'admin' ? (
              <ShieldMinus className="size-4" />
            ) : (
              <ShieldPlus className="size-4" />
            )}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-8 text-destructive"
            disabled={busy}
            aria-label={`Remove ${memberName(member.member)}`}
            title="Remove from team"
            onClick={() => removeMutation.mutate()}
          >
            <X className="size-4" />
          </Button>
        </div>
      )}
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
