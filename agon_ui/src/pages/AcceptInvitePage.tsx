import { useEffect } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Swords, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { clearPendingInvite } from '@/lib/pendingInvite'

type InvitationDetail = components['schemas']['InvitationDetail']
// The generated `context`/`kind` types erase the discriminant; cast to the real
// union so `.type` narrows (same pattern as NotificationsPage).
type InvitationContext = components['schemas']['InvitationContext']
type InvitationResponse = components['schemas']['InvitationResponse']

/**
 * The invite-link acceptance screen. Reached once the visitor is signed in with
 * an Agon profile (the invite landing routes them through login/signup first).
 * Previews the invite via the public `by-token` endpoint, then accepts/declines
 * with `respond-by-token` and drops the visitor at the match/team they joined.
 */
export function AcceptInvitePage() {
  const { token } = useParams<{ token: string }>()
  const navigate = useNavigate()

  // We're now on the invite URL itself; the stashed copy (used to survive login)
  // has done its job. Clear it so returning to "/" later doesn't bounce back to
  // an already-handled invite.
  useEffect(() => {
    clearPendingInvite()
  }, [])

  const preview = useQuery({
    queryKey: ['invite-by-token', token],
    enabled: !!token,
    retry: false,
    queryFn: async (): Promise<InvitationDetail> => {
      const { data, error } = await fetchClient.GET(
        '/invitations/by-token/{token}',
        { params: { path: { token: token! } } },
      )
      if (error || !data) throw new Error('invite-not-found')
      return data
    },
  })

  const respond = useMutation({
    mutationFn: async (response: InvitationResponse) => {
      const { error } = await fetchClient.POST('/invitations/respond-by-token', {
        body: { invite_token: token!, response },
      })
      if (error) throw new Error('Failed to respond to invitation')
    },
    onSuccess: (_data, response) => {
      clearPendingInvite()
      // On accept, drop the visitor at what they just joined; on decline, send
      // them to their feed.
      if (response === 'accepted') {
        navigate(destinationFor(preview.data), { replace: true })
      } else {
        navigate('/feed', { replace: true })
      }
    },
  })

  if (preview.isLoading) {
    return <InviteCard>Loading your invite…</InviteCard>
  }

  if (preview.isError || !preview.data) {
    return (
      <InviteCard>
        <h2 className="mb-2 text-xl font-semibold">Invite not found</h2>
        <p className="mb-6 text-sm text-muted-foreground">
          This invite link is invalid or has expired.
        </p>
        <Button variant="outline" onClick={() => navigate('/feed', { replace: true })}>
          Go to your feed
        </Button>
      </InviteCard>
    )
  }

  const detail = preview.data
  const context = detail.context as InvitationContext
  const status = detail.invitation.status

  // Already handled (e.g. the link was reused) — don't offer to respond again.
  if (status !== 'pending') {
    return (
      <InviteCard>
        <h2 className="mb-2 text-xl font-semibold">
          {status === 'accepted' ? 'Already accepted' : 'Invite declined'}
        </h2>
        <p className="mb-6 text-sm text-muted-foreground">
          {status === 'accepted'
            ? 'You have already joined.'
            : 'You previously declined this invite.'}
        </p>
        <Button onClick={() => navigate(destinationFor(detail), { replace: true })}>
          {contextLabel(context).action}
        </Button>
      </InviteCard>
    )
  }

  const label = contextLabel(context)
  const Icon = context.type === 'Match' ? Swords : Users

  return (
    <InviteCard>
      <div className="mb-4 flex size-14 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Icon className="size-7" />
      </div>
      <h2 className="mb-1 text-xl font-semibold">You've been invited!</h2>
      <p className="mb-6 text-sm text-muted-foreground">
        Join <strong className="font-medium text-foreground">{label.name}</strong>
        {label.kind}.
      </p>

      {respond.isError && (
        <p className="mb-4 text-sm text-red-600">
          Something went wrong. Please try again.
        </p>
      )}

      <div className="flex w-full gap-2">
        <Button
          className="flex-1"
          disabled={respond.isPending}
          onClick={() => respond.mutate('accepted')}
        >
          {respond.isPending ? 'Joining…' : 'Accept'}
        </Button>
        <Button
          variant="outline"
          className="flex-1"
          disabled={respond.isPending}
          onClick={() => respond.mutate('declined')}
        >
          Decline
        </Button>
      </div>
    </InviteCard>
  )
}

/** Where to send the visitor after they accept (or view an already-joined invite). */
function destinationFor(detail?: InvitationDetail): string {
  if (!detail) return '/feed'
  const context = detail.context as InvitationContext
  return context.type === 'Match'
    ? `/matches/${context.match_id}`
    : `/teams/${context.team_id}`
}

/** Human labels for the invite context. `name` may be blank (server snapshot), so fall back. */
function contextLabel(context: InvitationContext): {
  name: string
  kind: string
  action: string
} {
  if (context.type === 'Match') {
    return {
      name: context.match_name || 'a match',
      kind: '',
      action: 'View match',
    }
  }
  return {
    name: context.team_name || 'a team',
    kind: ' as a member',
    action: 'View team',
  }
}

/** Centered card chrome shared by every state of the accept screen. */
function InviteCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center rounded-2xl border bg-card p-8 text-center">
      {children}
    </div>
  )
}
