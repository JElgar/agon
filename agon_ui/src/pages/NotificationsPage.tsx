import { useState } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { formatDistanceToNow } from 'date-fns'
import {
  Bell,
  CheckCircle2,
  ClipboardCheck,
  Flame,
  MessageCircle,
  Swords,
  UserPlus,
  Users,
} from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from '@/components/agon/Avatar'
import { FollowButton } from '@/components/agon/FollowButton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  InvitationResponseDialog,
  type PendingScoreRef,
} from '@/components/agon/InvitationResponseDialog'

type NotificationPage = components['schemas']['NotificationPage']
type Notification = components['schemas']['Notification']
// `Notification['kind']` is generated as `Omit<NotificationKind, "type"> & unknown`,
// which erases the discriminated union (no per-variant narrowing on `.type`). Use
// the full `NotificationKind` union directly so the `switch` narrows correctly.
type Kind = components['schemas']['NotificationKind']

/** Page size for notifications. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

/**
 * The notifications inbox: the viewer's notifications newest-first with
 * cursor-based infinite scroll. Match/team invitations surface inline
 * Confirm/Decline actions (wired to `POST /invitations/:id/respond`); the rest
 * offer a "View" jump to the referenced entity. Opening or acting on a
 * notification marks it read, and "Mark all read" clears the lot.
 */
export function NotificationsPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const query = useInfiniteQuery({
    queryKey: ['notifications'],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<NotificationPage> => {
      const { data, error } = await fetchClient.GET('/notifications', {
        params: { query: { cursor: pageParam, limit: PAGE_SIZE } },
      })
      if (error || !data) throw new Error('Failed to load notifications')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor,
  })

  /** Invalidate both the list and the unread badge after any read/act change. */
  const refreshNotifications = () => {
    queryClient.invalidateQueries({ queryKey: ['notifications'] })
    queryClient.invalidateQueries({ queryKey: ['notifications-unread-count'] })
  }

  const markAllRead = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/notifications/read')
      if (error) throw new Error('Failed to mark all read')
    },
    onSuccess: refreshNotifications,
  })

  const markRead = useMutation({
    mutationFn: async (id: string) => {
      const { error } = await fetchClient.POST(
        '/notifications/{notification_id}/read',
        { params: { path: { notification_id: id } } },
      )
      if (error) throw new Error('Failed to mark read')
    },
    onSuccess: refreshNotifications,
  })

  const respond = useMutation({
    mutationFn: async (input: {
      invitationId: string
      response: components['schemas']['InvitationResponse']
    }) => {
      const { error } = await fetchClient.POST(
        '/invitations/{invitation_id}/respond',
        {
          params: { path: { invitation_id: input.invitationId } },
          body: { response: input.response },
        },
      )
      if (error) throw new Error('Failed to respond to invitation')
    },
    onSuccess: refreshNotifications,
  })

  if (query.isLoading) {
    return <NotificationsSkeleton />
  }

  if (query.isError) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">
          Couldn't load your notifications.
        </p>
        <Button variant="outline" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const items = (query.data?.pages ?? []).flatMap((page) => page.items)
  const hasUnread = items.some((n) => !n.is_read)

  if (items.length === 0) {
    return (
      <div className="py-16 text-center">
        <Bell className="mx-auto mb-3 size-8 text-muted-foreground" />
        <h2 className="mb-1 text-lg font-medium">No notifications yet</h2>
        <p className="text-sm text-muted-foreground">
          Match invites, follows, likes and comments show up here.
        </p>
      </div>
    )
  }

  return (
    <div className="mx-auto flex max-w-xl flex-col">
      <div className="mb-2 flex items-center justify-between">
        <h1 className="text-xl font-semibold">Notifications</h1>
        {hasUnread && (
          <Button
            variant="ghost"
            size="sm"
            disabled={markAllRead.isPending}
            onClick={() => markAllRead.mutate()}
          >
            Mark all read
          </Button>
        )}
      </div>

      <ul className="flex flex-col overflow-hidden rounded-xl border bg-card">
        {items.map((n) => (
          <NotificationRow
            key={n.id}
            notification={n}
            navigate={navigate}
            onMarkRead={() => {
              if (!n.is_read) markRead.mutate(n.id)
            }}
            respondToInvitation={(invitationId, response) =>
              respond.mutateAsync({ invitationId, response })
            }
          />
        ))}
      </ul>

      {query.hasNextPage && (
        <Button
          variant="outline"
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

interface NotificationRowProps {
  notification: Notification
  navigate: ReturnType<typeof useNavigate>
  onMarkRead: () => void
  respondToInvitation: (
    invitationId: string,
    response: components['schemas']['InvitationResponse'],
  ) => Promise<void>
}

function NotificationRow({
  notification,
  navigate,
  onMarkRead,
  respondToInvitation,
}: NotificationRowProps) {
  const queryClient = useQueryClient()
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)

  // See the `Kind` alias: the generated `notification.kind` type drops the
  // discriminant, so cast to the real union for exhaustive narrowing.
  const view = describe(notification.kind as Kind)

  /** Navigate to the notification's target and mark it read on the way. */
  const open = () => {
    onMarkRead()
    if (view.href) navigate(view.href)
  }

  return (
    <li
      className={cn(
        'flex gap-3 border-b px-4 py-3 last:border-b-0',
        !notification.is_read && 'bg-primary/5',
      )}
    >
      <div className="relative shrink-0">
        <Avatar name={view.actorName} imageUrl={view.actorImage} size="lg" />
        <span
          className={cn(
            'absolute -bottom-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full border-2 border-card text-primary-foreground',
            view.badgeClass,
          )}
        >
          <view.badgeIcon className="size-2.5" />
        </span>
      </div>

      <div className="min-w-0 flex-1">
        <button
          type="button"
          onClick={open}
          className="block text-left text-sm leading-snug"
        >
          {view.message}
        </button>
        <div className="mt-1 text-xs text-muted-foreground">
          {formatDistanceToNow(new Date(notification.created_at), {
            addSuffix: true,
          })}
        </div>

        {view.actions && (
          <div className="mt-2 flex flex-wrap gap-2">
            {view.actions.invitation && (
              <>
                <Button size="sm" onClick={() => setAction('accept')}>
                  Confirm
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setAction('decline')}
                >
                  Decline
                </Button>
              </>
            )}
            {view.followBack && (
              <FollowButton
                userId={view.followBack.userId}
                isFollowing={view.followBack.isFollowing}
                size="sm"
              />
            )}
            {view.href && (
              <Button size="sm" variant="ghost" onClick={open}>
                {view.actions.viewLabel}
              </Button>
            )}
          </div>
        )}
      </div>

      {!notification.is_read && (
        <span
          className="mt-1.5 size-2 shrink-0 rounded-full bg-primary"
          aria-label="Unread"
        />
      )}

      {view.actions?.invitation && (
        <InvitationResponseDialog
          open={action !== null}
          onOpenChange={(open) => !open && setAction(null)}
          action={action}
          name={view.actions.invitation.name}
          suffix={view.actions.invitation.suffix}
          pendingScore={view.actions.invitation.pendingScore}
          respond={(response) =>
            respondToInvitation(view.actions!.invitation!.id, response)
          }
          onSuccess={() => {
            setAction(null)
            const pendingScore = view.actions?.invitation?.pendingScore
            if (pendingScore) {
              queryClient.invalidateQueries({
                queryKey: ['match', pendingScore.matchId],
              })
              queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
            }
          }}
        />
      )}
    </li>
  )
}

/** Icon + accent for the small badge overlaid on the actor avatar. */
type BadgeIcon = typeof Bell

interface NotificationView {
  actorName: string
  actorImage?: string
  message: React.ReactNode
  badgeIcon: BadgeIcon
  badgeClass: string
  /** Where "View"/opening the row navigates, if anywhere. */
  href?: string
  actions?: {
    /** Present for invitation kinds → renders Confirm/Decline (via the shared
     *  accept/decline dialog). */
    invitation?: {
      id: string
      /** Match or team name, for the dialog's "Join X" / "Decline invite to X" copy. */
      name: string
      /** Trailing qualifier after the name, e.g. ' as a member' for a team. */
      suffix?: string
      /** Present only for a match invite whose score is already awaiting this
       *  invitee's side — lets the dialog offer to confirm it in one step. */
      pendingScore?: PendingScoreRef | null
    }
    /** Label for the plain "View" jump. */
    viewLabel: string
  }
  /** Present on follow notifications → renders an inline follow-back button. */
  followBack?: {
    userId: string
    isFollowing: boolean
  }
}

/**
 * Map a notification kind to its display: actor, message, badge, and the
 * action/navigation it offers. Centralised so the row stays presentational and
 * every variant is handled exhaustively.
 */
function describe(kind: Kind): NotificationView {
  switch (kind.type) {
    case 'MatchInvitation':
      return {
        actorName: kind.inviter.name,
        actorImage: kind.inviter.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.inviter.name}</strong> invited
            you to <strong className="font-medium">{kind.match_name}</strong>.
          </>
        ),
        badgeIcon: Swords,
        badgeClass: 'bg-primary',
        href: `/matches/${kind.match_id}`,
        actions: {
          invitation: {
            id: kind.invitation_id,
            name: kind.match_name,
            pendingScore: kind.pending_score_submission_id
              ? {
                  matchId: kind.match_id,
                  submissionId: kind.pending_score_submission_id,
                }
              : null,
          },
          viewLabel: 'View match',
        },
      }
    case 'TeamInvitation':
      return {
        actorName: kind.inviter.name,
        actorImage: kind.inviter.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.inviter.name}</strong> invited
            you to join <strong className="font-medium">{kind.team_name}</strong>
            .
          </>
        ),
        badgeIcon: Users,
        badgeClass: 'bg-primary',
        href: `/teams/${kind.team_id}`,
        actions: {
          invitation: {
            id: kind.invitation_id,
            name: kind.team_name,
            suffix: ' as a member',
          },
          viewLabel: 'View team',
        },
      }
    case 'InvitationAccepted': {
      // `context` has the same discriminant erasure as `kind` — cast to the
      // real union so `.type` narrows Match vs Team.
      const context = kind.context as components['schemas']['InvitationContext']
      const href =
        context.type === 'Match'
          ? `/matches/${context.match_id}`
          : `/teams/${context.team_id}`
      return {
        actorName: kind.accepted_by.name,
        actorImage: kind.accepted_by.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.accepted_by.name}</strong>{' '}
            accepted your invitation.
          </>
        ),
        badgeIcon: UserPlus,
        badgeClass: 'bg-emerald-600',
        href,
        actions: { viewLabel: 'View' },
      }
    }
    case 'Follow':
      return {
        actorName: kind.follower.name,
        actorImage: kind.follower.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.follower.name}</strong> started
            following you.
          </>
        ),
        badgeIcon: UserPlus,
        badgeClass: 'bg-emerald-600',
        href: `/users/${kind.follower.id}`,
        actions: { viewLabel: 'View profile' },
        followBack: {
          userId: kind.follower.id,
          isFollowing: kind.follower.is_followed_by_me,
        },
      }
    case 'Like':
      return {
        actorName: kind.liked_by.name,
        actorImage: kind.liked_by.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.liked_by.name}</strong> liked{' '}
            <strong className="font-medium">{kind.match_name}</strong>.
          </>
        ),
        badgeIcon: Flame,
        badgeClass: 'bg-amber-500',
        href: `/matches/${kind.match_id}`,
        actions: { viewLabel: 'View match' },
      }
    case 'Comment':
      return {
        actorName: kind.commenter.name,
        actorImage: kind.commenter.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.commenter.name}</strong>{' '}
            commented: “{kind.preview}”
          </>
        ),
        badgeIcon: MessageCircle,
        badgeClass: 'bg-muted-foreground',
        href: `/matches/${kind.match_id}`,
        actions: { viewLabel: 'View match' },
      }
    case 'Reply':
      return {
        actorName: kind.replier.name,
        actorImage: kind.replier.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.replier.name}</strong> replied: “
            {kind.preview}”
          </>
        ),
        badgeIcon: MessageCircle,
        badgeClass: 'bg-muted-foreground',
        href: `/matches/${kind.match_id}`,
        actions: { viewLabel: 'View match' },
      }
    case 'ScoreSubmitted':
      return {
        actorName: kind.submitted_by.name,
        actorImage: kind.submitted_by.profile_image?.image_url,
        message: kind.needs_confirmation ? (
          <>
            <strong className="font-medium">{kind.submitted_by.name}</strong>{' '}
            submitted a score for{' '}
            <strong className="font-medium">{kind.match_name}</strong> — confirm
            it?
          </>
        ) : (
          <>
            <strong className="font-medium">{kind.submitted_by.name}</strong>{' '}
            updated the score for{' '}
            <strong className="font-medium">{kind.match_name}</strong>.
          </>
        ),
        badgeIcon: ClipboardCheck,
        badgeClass: kind.needs_confirmation ? 'bg-primary' : 'bg-muted-foreground',
        href: `/matches/${kind.match_id}`,
        actions: {
          viewLabel: kind.needs_confirmation ? 'Review score' : 'View match',
        },
      }
    case 'ScoreConfirmed':
      return {
        actorName: kind.confirmed_by.name,
        actorImage: kind.confirmed_by.profile_image?.image_url,
        message: (
          <>
            <strong className="font-medium">{kind.confirmed_by.name}</strong>{' '}
            confirmed the score for{' '}
            <strong className="font-medium">{kind.match_name}</strong>.
          </>
        ),
        badgeIcon: CheckCircle2,
        badgeClass: 'bg-emerald-600',
        href: `/matches/${kind.match_id}`,
        actions: { viewLabel: 'View match' },
      }
  }
}

/** Placeholder rows while the first page loads. */
function NotificationsSkeleton() {
  return (
    <div className="mx-auto max-w-xl">
      <div className="mb-2 h-6 w-40 animate-pulse rounded bg-muted" aria-hidden />
      <ul className="flex flex-col overflow-hidden rounded-xl border bg-card">
        {Array.from({ length: 5 }).map((_, i) => (
          <li key={i} className="flex gap-3 border-b px-4 py-3 last:border-b-0">
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
