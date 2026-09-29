import { useState } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { formatDistanceToNow } from 'date-fns'
import { ChevronLeft, Mail, Swords, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { Button } from '@/components/ui/button'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { respondToInvitation } from '@/lib/invitations'

type NotificationPage = components['schemas']['NotificationPage']
type Notification = components['schemas']['Notification']
// See `NotificationsPage`: the generated `Notification['kind']` type drops the
// discriminant, so the full `NotificationKind` union is used directly for
// exhaustive/`.type` narrowing.
type Kind = components['schemas']['NotificationKind']
type MatchInvitationKind = Extract<Kind, { type: 'MatchInvitation' }>
type TeamInvitationKind = Extract<Kind, { type: 'TeamInvitation' }>
type InvitationKind = MatchInvitationKind | TeamInvitationKind

/** Page size for the underlying notifications fetch. The API caps at 50; 20
 *  matches its default (and matches `NotificationsPage`'s own page size). */
const PAGE_SIZE = 20

/**
 * A pending match/team invitation, as it appears in this list.
 *
 * There's no dedicated hydrated feed for "pending invites only": the
 * `GET /users/me/invitations` endpoint exists, but its `InvitationDetail`
 * only carries `invited_by_user_id` (a bare id) and the match/team's
 * id+name — no inviter name/avatar and no match kickoff time, both of which
 * the mock shows on every row. Reusing that endpoint would mean re-hydrating
 * all of that client-side (batch user lookups, a per-row match fetch),
 * duplicating work the notifications feed already does server-side (every
 * `MatchInvitation`/`TeamInvitation` notification already carries the
 * inviter's name/avatar). So this page reuses `NotificationsPage`'s own
 * query (same query key, so the cache is shared) and filters client-side to
 * pending invitation-shaped notifications — exactly how `NotificationsPage`
 * itself identifies its "Needs your reply" section.
 */
interface PendingInvite {
  notificationId: string
  actorName: string
  actorImage?: string
  message: React.ReactNode
  badgeIcon: typeof Swords
  createdAt: string
  invitationId: string
  /** Match or team name, for the dialog's "Join X" / "Decline invite to X" copy. */
  name: string
  suffix?: string
  matchId?: string
  acceptLabel: string
  declineLabel: string
}

function toPendingInvite(notification: Notification, kind: InvitationKind): PendingInvite {
  const shared = {
    notificationId: notification.id,
    actorName: kind.inviter.name,
    actorImage: kind.inviter.profile_image?.image_url,
    createdAt: notification.created_at,
    invitationId: kind.invitation_id,
  }
  if (kind.type === 'MatchInvitation') {
    return {
      ...shared,
      message: (
        <>
          <strong className="font-medium">{kind.inviter.name}</strong> invited
          you to <strong className="font-medium">{kind.match_name}</strong>
        </>
      ),
      badgeIcon: Swords,
      name: kind.match_name,
      matchId: kind.match_id,
      acceptLabel: "I'm in",
      declineLabel: "Can't make it",
    }
  }
  return {
    ...shared,
    message: (
      <>
        <strong className="font-medium">{kind.inviter.name}</strong> invited
        you to join <strong className="font-medium">{kind.team_name}</strong>
      </>
    ),
    badgeIcon: Users,
    name: kind.team_name,
    suffix: ' as a member',
    acceptLabel: 'Accept',
    declineLabel: 'Decline',
  }
}

/** Pending match/team invitations, filtered from the same notifications feed
 *  `NotificationsPage` reads — see `PendingInvite`'s doc comment for why. */
function pendingInvitesFrom(items: Notification[]): PendingInvite[] {
  const invites: PendingInvite[] = []
  for (const n of items) {
    const kind = n.kind as Kind
    if (
      (kind.type === 'MatchInvitation' || kind.type === 'TeamInvitation') &&
      kind.status === 'pending'
    ) {
      invites.push(toPendingInvite(n, kind))
    }
  }
  return invites
}

/**
 * The viewer's pending invitations (match + team), replacing the old
 * "Invitations" nav placeholder. Same row shape and accept/decline flow as
 * `NotificationsPage`'s "Needs your reply" section, without the rest of the
 * feed mixed in — see the `Invitations` design mock.
 */
export function InvitationsPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  // Same query key as `NotificationsPage` — the cache (and any in-flight
  // fetch) is shared between the two pages.
  const query = useInfiniteQuery({
    queryKey: ['notifications'],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<NotificationPage> => {
      const { data, error } = await fetchClient.GET('/notifications', {
        params: { query: { cursor: pageParam, limit: PAGE_SIZE } },
      })
      if (error || !data) throw new Error('Failed to load invitations')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor,
  })

  const respond = useMutation({
    mutationFn: async (input: {
      invitationId: string
      response: components['schemas']['InvitationResponse']
    }) => {
      await respondToInvitation(input.invitationId, input.response)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({ queryKey: ['notifications-unread-count'] })
    },
  })

  const header = (
    <div className="mb-2 flex items-center gap-2">
      <Button
        variant="ghost"
        size="icon"
        className="size-8"
        aria-label="Back"
        onClick={() => navigate(-1)}
      >
        <ChevronLeft className="size-4" />
      </Button>
      <h1 className="font-display text-2xl font-extrabold">Invitations</h1>
    </div>
  )

  if (query.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        {header}
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          {Array.from({ length: 3 }).map((_, i) => (
            <li key={i} className="flex gap-3 border-b px-4 py-3.5 last:border-b-0">
              <div className="size-9 shrink-0 animate-pulse rounded-full bg-muted" />
              <div className="flex-1 space-y-2 py-1">
                <div className="h-3 w-3/4 animate-pulse rounded bg-muted" />
                <div className="h-2.5 w-16 animate-pulse rounded bg-muted" />
              </div>
            </li>
          ))}
        </ul>
      </div>
    )
  }

  if (query.isError) {
    return (
      <div className="mx-auto max-w-xl">
        {header}
        <div className="py-16 text-center">
          <p className="mb-4 text-muted-foreground">
            Couldn't load your invitations.
          </p>
          <Button variant="outline" shape="pill" onClick={() => query.refetch()}>
            Retry
          </Button>
        </div>
      </div>
    )
  }

  const items = (query.data?.pages ?? []).flatMap((page) => page.items)
  const invites = pendingInvitesFrom(items)

  return (
    <div className="mx-auto flex max-w-xl flex-col">
      {header}

      {invites.length === 0 ? (
        <div className="py-16 text-center">
          <Mail className="mx-auto mb-3 size-8 text-muted-foreground" />
          <h2 className="mb-1 font-display text-lg font-bold">
            No pending invitations
          </h2>
          <p className="text-sm text-muted-foreground">
            Match and team invites you haven't responded to yet show up here.
          </p>
        </div>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          {invites.map((invite) => (
            <InvitationRow
              key={invite.notificationId}
              invite={invite}
              respond={(response) =>
                respond.mutateAsync({ invitationId: invite.invitationId, response })
              }
            />
          ))}
        </ul>
      )}

      {query.hasNextPage && (
        <Button
          variant="outline"
          shape="pill"
          className="mt-3"
          disabled={query.isFetchingNextPage}
          onClick={() => query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more'}
        </Button>
      )}
    </div>
  )
}

function InvitationRow({
  invite,
  respond,
}: {
  invite: PendingInvite
  respond: (response: components['schemas']['InvitationResponse']) => Promise<void>
}) {
  const queryClient = useQueryClient()
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)

  return (
    <li className="flex gap-3 border-b bg-accent/40 px-4 py-3.5 last:border-b-0">
      <div className="relative shrink-0">
        <Avatar name={invite.actorName} imageUrl={invite.actorImage} size="lg" />
        <span className="absolute -bottom-0.5 -right-0.5 flex size-5 items-center justify-center rounded-full border-2 border-card bg-primary text-primary-foreground">
          <invite.badgeIcon className="size-3" />
        </span>
      </div>

      <div className="min-w-0 flex-1">
        <p className="text-[15px] leading-snug">{invite.message}</p>
        <div className="mt-0.5 text-[13px] text-muted-foreground">
          invited{' '}
          {formatDistanceToNow(new Date(invite.createdAt), { addSuffix: true })}
        </div>

        <div className="mt-2 flex flex-wrap gap-2">
          <Button size="sm" shape="pill" onClick={() => setAction('accept')}>
            {invite.acceptLabel}
          </Button>
          <Button
            size="sm"
            shape="pill"
            variant="outline"
            onClick={() => setAction('decline')}
          >
            {invite.declineLabel}
          </Button>
        </div>
      </div>

      <InvitationResponseDialog
        open={action !== null}
        onOpenChange={(open) => !open && setAction(null)}
        action={action}
        name={invite.name}
        suffix={invite.suffix}
        matchId={invite.matchId}
        respond={respond}
        onSuccess={() => {
          setAction(null)
          if (invite.matchId) {
            queryClient.invalidateQueries({ queryKey: ['match', invite.matchId] })
            queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
          }
        }}
      />
    </li>
  )
}
