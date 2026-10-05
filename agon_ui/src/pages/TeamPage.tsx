import { useState } from 'react'
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import {
  ChevronLeft,
  ChevronRight,
  Crown,
  LogOut,
  MailOpen,
  MoreVertical,
  Pencil,
  ShieldMinus,
  ShieldPlus,
  Trash2,
  UserMinus,
  UserPlus,
} from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { TeamFollowButton } from '@/components/agon/TeamFollowButton'
import { EditTeamDialog } from '@/components/agon/EditTeamDialog'
import { InviteToTeamDialog } from '@/components/agon/InviteToTeamDialog'
import { DeleteTeamDialog } from '@/components/agon/DeleteTeamDialog'
import { LeaveTeamDialog } from '@/components/agon/LeaveTeamDialog'
import { PromoteToOwnerDialog } from '@/components/agon/PromoteToOwnerDialog'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { InvitePromptDialog } from '@/components/agon/InvitePromptDialog'
import { respondToInvitation } from '@/lib/invitations'
import { MatchCard } from '@/components/agon/MatchCard'
import { StatTile } from '@/components/agon/StatTile'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { useInvitePrompt } from '@/hooks/useInvitePrompt'
import {
  memberName,
  memberAvatarUrl,
  myPendingTeamInvitation,
  withTeamMemberInvitationStatus,
} from '@/lib/members'
import { sportIcon, sportLabel } from '@/lib/sports'
import { formatWinRate } from '@/lib/stats'
import { teamRecord, teamSportRecords } from '@/lib/teamStats'

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
 * Matches fetched to derive the "Team stats" banner and per-sport rows
 * (there's no team-stats endpoint — see `lib/teamStats.ts`). The API caps a
 * page at 50, so a team with more than 50 completed matches gets a record
 * based on only its most recent 50, not its full history — an accepted,
 * documented approximation (see the PR description), same kind of gap as
 * the sport-stats page's "no per-match annotation" one.
 */
const TEAM_STATS_MATCH_LIMIT = 50

/**
 * A team's page: logo/name/follower count, its member list, and its recent
 * matches (`GET /matches?team_id=`, the same discovery endpoint the profile
 * page's activity section uses with `participant` instead). What else the
 * viewer sees depends on their role — owner gets edit/invite/delete plus
 * per-member remove and promote/demote controls, admin gets the same minus
 * delete, a plain member sees the team read-only. Any accepted member can
 * leave (the owner is routed through transferring ownership first — see
 * `LeaveTeamDialog`); a non-member sees a follow toggle instead. Mirrors
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

  // A separate, larger fetch (vs. `activityQuery`'s 5) purely to derive the
  // stats banner/sport rows below from — see `TEAM_STATS_MATCH_LIMIT`.
  const statsMatchesQuery = useQuery({
    queryKey: ['team-stats-matches', teamId],
    enabled: !!teamId,
    queryFn: async (): Promise<SearchMatch[]> => {
      const { data, error } = await fetchClient.GET('/matches', {
        params: { query: { team_id: [teamId!], limit: TEAM_STATS_MATCH_LIMIT } },
      })
      if (error || !data) throw new Error('Failed to load team stats')
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
        <Button variant="outline" shape="pill" onClick={() => teamQuery.refetch()}>
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

  // `Team`/`TeamMemberPage` carry no total member count (the member list is
  // cursor-paginated, not counted) — this is the count of whatever's been
  // fetched so far, with a "+" once there's a next page still to load.
  const memberCountLabel = `${members.length}${membersQuery.hasNextPage ? '+' : ''}`

  const statsMatches = statsMatchesQuery.data ?? []
  const record = teamRecord(statsMatches, team.id)
  const sportRecords = teamSportRecords(statsMatches, team.id)

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-6 md:max-w-3xl md:gap-8 lg:max-w-6xl">
      <div className="flex items-center justify-between md:hidden">
        <Button
          variant="ghost"
          size="icon"
          className="size-11 rounded-full"
          aria-label="Back"
          onClick={() => navigate(-1)}
        >
          <ChevronLeft className="size-5" />
        </Button>
      </div>

      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between md:gap-6">
        <div className="flex items-center gap-4">
          {/* On desktop the back button sits inline with the header instead
              of on its own row above (mobile keeps the standalone row, since
              there's no adjacent content to align it with there). */}
          <Button
            variant="ghost"
            size="icon"
            className="hidden size-11 shrink-0 rounded-full md:inline-flex"
            aria-label="Back"
            onClick={() => navigate(-1)}
          >
            <ChevronLeft className="size-5" />
          </Button>
          <Avatar
            name={team.name}
            imageUrl={team.logo?.image_url}
            size="xl"
            className="size-[76px] rounded-[20px] text-2xl"
          />
          <div className="min-w-0">
            <h1 className="truncate font-display text-2xl font-extrabold md:text-3xl">
              {team.name}
            </h1>
            <p className="text-sm text-muted-foreground">
              <span className="font-bold text-foreground">
                {team.follower_count.toLocaleString()}
              </span>{' '}
              {team.follower_count === 1 ? 'follower' : 'followers'} ·{' '}
              <span className="font-bold text-foreground">{memberCountLabel}</span>{' '}
              {members.length === 1 ? 'member' : 'members'}
            </p>
          </div>
        </div>

        <div className="flex gap-2 md:shrink-0">
          {canManage && (
            <>
              <EditTeamDialog team={team}>
                <Button variant="outline" className="flex-1 gap-2 rounded-2xl md:flex-none">
                  <Pencil className="size-4" />
                  Edit
                </Button>
              </EditTeamDialog>
              <InviteToTeamDialog teamId={team.id} excludeUserIds={existingUserIds}>
                <Button variant="outline" className="flex-1 gap-2 rounded-2xl md:flex-none">
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
                  <Button
                    variant="outline"
                    size="icon"
                    className="size-11 shrink-0 rounded-2xl border-destructive/30 text-destructive hover:bg-destructive/10 hover:text-destructive"
                    aria-label="Delete team"
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </DeleteTeamDialog>
              )}
            </>
          )}
          {/* A member (owner/admin/plain) leaves; a non-member follows —
              mutually exclusive, so exactly one of these always renders. */}
          {myRole ? (
            <LeaveTeamDialog
              teamId={team.id}
              teamName={team.name}
              myRole={myRole}
              members={members}
              currentUserId={currentUserId}
              onLeft={() => navigate('/teams')}
            >
              <Button variant="outline" className="flex-1 gap-2 rounded-2xl md:flex-none">
                <LogOut className="size-4" />
                Leave
              </Button>
            </LeaveTeamDialog>
          ) : (
            <TeamFollowButton
              teamId={team.id}
              isFollowing={team.is_followed_by_me}
              shape="pill"
              className="flex-1 md:flex-none"
            />
          )}
        </div>
      </div>

      {/* Respond to a pending invite, if the viewer has one — same
          accept/decline pattern as a match page's `InviteBanner`. */}
      <TeamInviteBanner teamId={team.id} name={team.name} members={members} currentUserId={currentUserId} />

      {/* Stats / members / recent matches stack on mobile; from `md:` up they
          sit side by side (stats+members share a row, matches spans below —
          `lg:` widens to a 3-up row) instead of one long scrolling column. */}
      <div className="flex flex-col gap-6 md:grid md:grid-cols-2 md:items-start md:gap-6 lg:grid-cols-3">
        <TeamStats
          query={statsMatchesQuery}
          record={record}
          sportRecords={sportRecords}
        />

        <section className="flex flex-col gap-2.5">
          <h2 className="font-display text-lg font-bold">Members</h2>
          <Members
            query={membersQuery}
            members={members}
            currentUserId={currentUserId}
            teamId={team.id}
            canManage={canManage}
            viewerIsOwner={isOwner}
          />
        </section>

        <section className="flex flex-col gap-2.5 md:col-span-2 lg:col-span-1">
          <h2 className="font-display text-lg font-bold">Recent matches</h2>
          <RecentMatches query={activityQuery} currentUserId={currentUserId} />
        </section>
      </div>
    </div>
  )
}

interface TeamStatsProps {
  query: ReturnType<typeof useQuery<SearchMatch[]>>
  record: ReturnType<typeof teamRecord>
  sportRecords: ReturnType<typeof teamSportRecords>
}

/**
 * "Team stats" — a matches/wins/win-rate banner plus a per-sport progress-bar
 * breakdown, visually matching how the (separately redesigned) profile page
 * shows a user's own all-sports stats banner + per-sport rows. There's no
 * `StatBanner`/`SportProgressRow` primitive yet (this is the first team-stats
 * implementation), so it's built inline here from `StatTile` + `Card`; see
 * `lib/teamStats.ts` for how the numbers are derived (client-side from
 * recent matches, not a backend stats endpoint).
 */
function TeamStats({ query, record, sportRecords }: TeamStatsProps) {
  if (query.isLoading) {
    return (
      <div className="flex flex-col gap-2.5">
        <div className="h-28 animate-pulse rounded-2xl bg-card" aria-hidden />
        <div className="h-24 animate-pulse rounded-2xl border bg-card" aria-hidden />
      </div>
    )
  }

  // A failed stats fetch shouldn't block the rest of the page — just drop
  // the section rather than showing an error state for a non-critical one.
  if (query.isError) return null

  // No confirmed matches yet: nothing meaningful to show a record or
  // breakdown for.
  if (record.matches === 0) return null

  return (
    <div className="flex flex-col gap-2.5">
      <section
        aria-label="Team stats"
        className="grid grid-cols-3 gap-2 rounded-2xl border border-banner-tint-border bg-banner-tint p-[18px]"
      >
        <StatTile tone="banner-blue" value={record.matches} label="Matches" />
        <StatTile tone="banner-blue" value={record.wins} label="Wins" />
        <StatTile
          tone="banner-blue"
          value={formatWinRate(record.winRatePct)}
          label="Win rate"
        />
      </section>

      {sportRecords.length > 0 && (
        <section className="flex flex-col gap-1">
          <h2 className="px-1 font-display text-lg font-bold">Team&apos;s sports</h2>
          <Card className="flex flex-col divide-y overflow-hidden">
            {sportRecords.map((entry) => {
              const Icon = sportIcon(entry.sport)
              const pct = Math.round(entry.winRatePct ?? 0)
              return (
                <div key={entry.sport} className="flex items-center gap-3.5 px-4 py-3.5">
                  <span className="flex size-11 shrink-0 items-center justify-center rounded-full bg-accent">
                    <Icon className="size-5 text-accent-foreground" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      <span className="text-base font-bold">{sportLabel(entry.sport)}</span>
                      <span className="text-sm text-muted-foreground">
                        {entry.matches} {entry.matches === 1 ? 'match' : 'matches'} ·{' '}
                        {pct}% won
                      </span>
                    </div>
                    <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted">
                      <div
                        className="h-full rounded-full bg-primary"
                        style={{ width: `${pct}%` }}
                      />
                    </div>
                  </div>
                </div>
              )
            })}
          </Card>
        </section>
      )}
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
      await respondToInvitation(invitation.id, response)
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
      <div className="rounded-2xl border border-primary/30 bg-primary/5 p-4">
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
              <Button shape="pill" size="sm" onClick={() => setAction('accept')}>
                Accept
              </Button>
              <Button
                shape="pill"
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
  teamId: string
  /** Whether the viewer is the team's owner or an admin — gates the
   *  remove/promote/demote controls on each row. */
  canManage: boolean
  /** Whether the viewer is specifically the owner — gates the "Promote to
   *  owner" item, which only the current owner may use. */
  viewerIsOwner: boolean
}

/** The member list: loading / error / empty states, else the rows plus a
 *  "Load more" button — same pagination pattern as `FollowListPage`. */
function Members({
  query,
  members,
  currentUserId,
  teamId,
  canManage,
  viewerIsOwner,
}: MembersProps) {
  if (query.isLoading) {
    return (
      <Card className="flex flex-col overflow-hidden">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="flex items-center gap-3 border-b px-4 py-3 last:border-b-0">
            <div className="size-10 shrink-0 animate-pulse rounded-full bg-muted" />
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
      <Card className="p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load members.</p>
        <Button variant="outline" shape="pill" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </Card>
    )
  }

  return (
    <Card className="flex flex-col divide-y overflow-hidden">
      {members.map((member) => (
        <MemberRow
          key={member.member.id}
          member={member}
          currentUserId={currentUserId}
          teamId={teamId}
          canManage={canManage}
          viewerIsOwner={viewerIsOwner}
        />
      ))}

      {query.hasNextPage && (
        <button
          type="button"
          disabled={query.isFetchingNextPage}
          onClick={() => query.fetchNextPage()}
          className="flex items-center justify-center gap-1.5 py-3 text-sm font-bold text-primary disabled:opacity-60"
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more members'}
          {!query.isFetchingNextPage && <ChevronRight className="size-4" />}
        </button>
      )}
    </Card>
  )
}

/** One row in the member list: avatar, name, role, a pending-invite badge for
 *  someone who hasn't accepted yet, and — for an owner/admin viewer, on
 *  anyone but the team's owner — a "…" menu of labeled actions (promote,
 *  demote, remove, and — owner only — transfer ownership) rather than a row
 *  of bare icon buttons, so what each one does doesn't depend on guessing
 *  at an icon. Owns its own mutations (mirrors `TeamFollowButton`),
 *  invalidating the members list on success. */
function MemberRow({
  member,
  currentUserId,
  teamId,
  canManage,
  viewerIsOwner,
}: {
  member: TeamMember
  currentUserId?: string
  teamId: string
  canManage: boolean
  viewerIsOwner: boolean
}) {
  const queryClient = useQueryClient()
  const [promoteToOwnerOpen, setPromoteToOwnerOpen] = useState(false)
  const isYou = member.member.type === 'User' && member.member.user_id === currentUserId
  const pending = member.member.invitation?.status === 'pending'
  // The owner's role is permanent — the server rejects changing or removing
  // it regardless of caller, so there's nothing for these controls to do on
  // that row even for another owner-equivalent caller (there's only ever one).
  const isOwnerRow = member.role === 'owner'
  const name = memberName(member.member)

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
  // Ownership can only transfer to someone who has actually accepted — the
  // server rejects a pending invitee (see `transfer_team_ownership`).
  const canPromoteToOwner = viewerIsOwner && !pending

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <Avatar
        name={name}
        imageUrl={memberAvatarUrl(member.member)}
        size="lg"
        ring={isYou ? 'you' : 'none'}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[15px] font-semibold">
          {name}
          {isYou && <span className="text-muted-foreground"> (you)</span>}
        </div>
        <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
          <span className="capitalize">{member.role}</span>
          {pending && <span>· invited</span>}
        </div>
      </div>

      {canManage && !isOwnerRow && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 shrink-0"
              disabled={busy}
              aria-label={`Manage ${name}`}
            >
              <MoreVertical className="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            {member.role === 'admin' ? (
              <DropdownMenuItem
                disabled={busy}
                onSelect={() => roleMutation.mutate('member')}
              >
                <ShieldMinus />
                Demote to member
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem
                disabled={busy}
                onSelect={() => roleMutation.mutate('admin')}
              >
                <ShieldPlus />
                Promote to admin
              </DropdownMenuItem>
            )}
            {canPromoteToOwner && (
              <DropdownMenuItem
                disabled={busy}
                onSelect={() => setPromoteToOwnerOpen(true)}
              >
                <Crown />
                Promote to owner
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-destructive focus:bg-destructive/10 focus:text-destructive"
              disabled={busy}
              onSelect={() => removeMutation.mutate()}
            >
              <UserMinus />
              Remove from team
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      {canPromoteToOwner && (
        <PromoteToOwnerDialog
          teamId={teamId}
          memberId={member.member.id}
          memberName={name}
          open={promoteToOwnerOpen}
          onOpenChange={setPromoteToOwnerOpen}
        />
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
            className="h-48 animate-pulse rounded-2xl border bg-card"
            aria-hidden
          />
        ))}
      </div>
    )
  }

  if (query.isError) {
    return (
      <Card className="p-6 text-center">
        <p className="mb-3 text-sm text-muted-foreground">
          Couldn't load recent matches.
        </p>
        <Button variant="outline" shape="pill" size="sm" onClick={() => query.refetch()}>
          Retry
        </Button>
      </Card>
    )
  }

  const matches = query.data ?? []

  if (matches.length === 0) {
    return (
      <Card className="p-6 text-center text-sm text-muted-foreground">
        No matches yet.
      </Card>
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
    <div className="mx-auto flex max-w-xl flex-col gap-6 md:max-w-3xl md:gap-8 lg:max-w-6xl">
      <div className="flex items-center gap-4">
        <div className="size-[76px] animate-pulse rounded-[20px] bg-card" aria-hidden />
        <div className="h-6 w-40 animate-pulse rounded bg-card" aria-hidden />
      </div>
      <div className="flex flex-col gap-6 md:grid md:grid-cols-2 md:gap-6 lg:grid-cols-3">
        <div className="h-28 animate-pulse rounded-2xl bg-card" aria-hidden />
        <div className="h-40 animate-pulse rounded-2xl border bg-card" aria-hidden />
        <div className="h-48 animate-pulse rounded-2xl border bg-card md:col-span-2 lg:col-span-1" aria-hidden />
      </div>
    </div>
  )
}
