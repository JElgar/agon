import { useQuery } from '@tanstack/react-query'
import { Swords, Users, Watch } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import type { PendingInviteKind } from '@/lib/pendingInvite'

type InvitationDetail = components['schemas']['InvitationDetail']
type InvitationContext = components['schemas']['InvitationContext']
type JoinLinkPreview = components['schemas']['JoinLinkPreview']

/**
 * Shown above the login form when the visitor arrived via an invite, join,
 * or device-pairing link, so they understand why they're signing in. Invite
 * and join links preview via their public by-token endpoints, so they work
 * before authentication; a pairing code has no such preview (nothing about
 * it exists server-side until it's confirmed — see
 * `agon_core::dao::device_pairing`), so that one's just a static banner.
 * Renders nothing for invite/join if the token doesn't resolve
 * (invalid/expired/revoked) — the login form still stands on its own.
 */
export function InvitePreviewBanner({
  kind,
  token,
}: {
  kind: PendingInviteKind
  token: string
}) {
  if (kind === 'pair') return <PairPreview />
  return kind === 'join' ? <JoinPreview token={token} /> : <InvitationPreview token={token} />
}

function PairPreview() {
  return <Banner icon={Watch}>Pair a Garmin watch with your account</Banner>
}

function InvitationPreview({ token }: { token: string }) {
  const preview = useQuery({
    queryKey: ['invite-by-token', token],
    retry: false,
    queryFn: async (): Promise<InvitationDetail> => {
      const { data, error } = await fetchClient.GET(
        '/invitations/by-token/{token}',
        { params: { path: { token } } },
      )
      if (error || !data) throw new Error('invite-not-found')
      return data
    },
  })

  if (!preview.data) return null

  const context = preview.data.context as InvitationContext
  const name =
    context.type === 'Match'
      ? context.match_name || 'a match'
      : context.team_name || 'a team'

  return (
    <Banner icon={context.type === 'Match' ? Swords : Users}>
      You've been invited to join {name}
    </Banner>
  )
}

function JoinPreview({ token }: { token: string }) {
  const preview = useQuery({
    queryKey: ['join-link-by-token', token],
    retry: false,
    queryFn: async (): Promise<JoinLinkPreview> => {
      const { data, error } = await fetchClient.GET('/join-links/by-token/{token}', {
        params: { path: { token } },
      })
      if (error || !data) throw new Error('join-link-not-found')
      return data
    },
  })

  if (!preview.data) return null

  return (
    <Banner icon={Swords}>
      You've been invited to join {preview.data.match_name || 'a match'}
    </Banner>
  )
}

function Banner({
  icon: Icon,
  children,
}: {
  icon: typeof Swords
  children: React.ReactNode
}) {
  return (
    <div className="mb-6 flex items-center gap-3 rounded-xl border bg-card p-4 text-left">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Icon className="size-5" />
      </div>
      <div className="min-w-0 text-sm">
        <p className="font-medium">{children}</p>
        <p className="text-muted-foreground">Sign in or create an account to accept.</p>
      </div>
    </div>
  )
}
