import { useState } from 'react'
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, MailOpen, Pencil, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { TeamFollowButton } from '@/components/agon/TeamFollowButton'
import { EditTeamDialog } from '@/components/agon/EditTeamDialog'
import { InviteToTeamDialog } from '@/components/agon/InviteToTeamDialog'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { InvitePromptDialog } from '@/components/agon/InvitePromptDialog'
import { MatchCard } from '@/components/agon/MatchCard'
import { Button } from '@/components/ui/button'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { useInvitePrompt } from '@/hooks/useInvitePrompt'
import {
  memberName,
  memberAvatarUrl,
  myPendingTeamInvitation,
  withTeamMemberInvitationStatus,
} from '@/lib/members'

type Team = components['schemas']['Team']
type TeamMember = components['schemas']['TeamMember']
type TeamMemberPage = components['schemas']['TeamMemberPage']
type SearchMatch = components['schemas']['SearchMatch']

/** Recent-activity matches to fetch/show, same limit as the profile page's. */
const RECENT_LIMIT = 5
/** Member page size. The API caps at 50; 20 matches its default. */
const MEMBER_PAGE_SIZE = 20

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

  // Paginated — see GET /teams/{team_id}/members. A team's page fetches one
  // page up front (like a user's followers list); "isAdmin"/"already on the
  // team" below only see pages actually fetched so far, which is exactly the
  // first page until "Load more" is used. Fine for a squad-sized team; a
  // (currently hypothetical) team past the first page where the viewer's own
  // row lands later would under-detect admin status until they load more.
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
  const isAdmin = members.some(
    (m) =>
      m.member.type === 'User' &&
      m.member.user_id === currentUserId &&
      m.role === 'admin',
  )
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

      {/* Respond to a pending invite, if the viewer has one — same
          accept/decline pattern as a match page's `InviteBanner`. */}
      <TeamInviteBanner teamId={team.id} name={team.name} members={members} currentUserId={currentUserId} />

      <section className="flex flex-col gap-2">
        <h2 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Members
        </h2>
        <Members query={membersQuery} members={members} currentUserId={currentUserId} />
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

/**
 * Shown when the signed-in viewer has a pending invitation to this team: a
 * prominent Accept/Decline banner, plus (the first time this team page is
 * opened while the invite is pending — see `useInvitePrompt`) a popup
 * fronting the same choice immediately. Both open the shared response
 * dialog, wired to `POST /invitations/:id/respond`. On success it refreshes
 * the member list (so the roster/badge update) and the notification badge
 * (the matching invite notification is now handled). Mirrors
 * `MatchDetailPage`'s `InviteBanner`.
 */
function TeamInviteBanner({
  teamId,
  name,
  members,
  currentUserId,
}: {
  teamId: string
  name: string
  members: TeamMember[]
  currentUserId?: string
}) {
  const queryClient = useQueryClient()
  const invitation = myPendingTeamInvitation(members, currentUserId)
  const membersKey = ['team-members', teamId]
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)
  const [promptOpen, setPromptOpen] = useInvitePrompt(invitation?.id ?? null)

  const respond = useMutation({
    mutationFn: async (
      response: components['schemas']['InvitationResponse'],
    ) => {
      if (!invitation) return
      const { error } = await fetchClient.POST(
        '/invitations/{invitation_id}/respond',
        {
          params: { path: { invitation_id: invitation.id } },
          body: { response },
        },
      )
      if (error) throw new Error('Failed to respond to invitation')
    },
    // Optimistically flip the viewer's invitation status across every fetched
    // page of the member list, so the banner/badge disappear immediately.
    onMutate: async (response) => {
      if (!currentUserId) return
      await queryClient.cancelQueries({ queryKey: membersKey })
      const previous =
        queryClient.getQueryData<InfiniteData<TeamMemberPage>>(membersKey)
      const status = response === 'accepted' ? 'accepted' : 'declined'
      if (previous) {
        queryClient.setQueryData<InfiniteData<TeamMemberPage>>(membersKey, {
          ...previous,
          pages: previous.pages.map((page) => ({
            ...page,
            items: withTeamMemberInvitationStatus(page.items, currentUserId, status),
          })),
        })
      }
      return { previous }
    },
    // Roll back the optimistic patch if the request fails.
    onError: (_err, _response, context) => {
      if (context?.previous) {
        queryClient.setQueryData(membersKey, context.previous)
      }
    },
    // Reconcile with the server regardless of outcome, and refresh notifications
    // (the invite notification is now handled) and the feed (roster changed).
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: membersKey })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({
        queryKey: ['notifications-unread-count'],
      })
    },
  })

  if (!invitation) return null

  const handleResponded = () => {
    setAction(null)
    queryClient.invalidateQueries({ queryKey: membersKey })
  }

  return (
    <>
      <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
            <MailOpen className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium">You've been invited to join this team</p>
            <p className="text-xs text-muted-foreground">
              Accept to join the roster, or decline if it's not for you.
            </p>
            <div className="mt-3 flex gap-2">
              <Button size="sm" onClick={() => setAction('accept')}>
                Accept
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAction('decline')}
              >
                Decline
              </Button>
            </div>
          </div>
        </div>
      </div>

      <InvitationResponseDialog
        open={action !== null}
        onOpenChange={(open) => !open && setAction(null)}
        action={action}
        name={name}
        suffix=" as a member"
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={handleResponded}
      />

      <InvitePromptDialog
        open={promptOpen}
        onOpenChange={setPromptOpen}
        name={name}
        suffix=" as a member"
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={handleResponded}
      />
    </>
  )
}

interface MembersProps {
  query: ReturnType<typeof useInfiniteQuery<TeamMemberPage>>
  members: TeamMember[]
  currentUserId?: string
}

/** The member list: loading / error / empty states, else the rows plus a
 *  "Load more" button — same pagination pattern as `FollowListPage`. */
function Members({ query, members, currentUserId }: MembersProps) {
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
            <MemberRow member={member} currentUserId={currentUserId} />
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
