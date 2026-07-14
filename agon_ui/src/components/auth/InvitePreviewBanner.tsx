import { useQuery } from '@tanstack/react-query'
import { Swords, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type InvitationDetail = components['schemas']['InvitationDetail']
type InvitationContext = components['schemas']['InvitationContext']

/**
 * Shown above the login form when the visitor arrived via an invite link, so
 * they understand why they're signing in. Uses the public `by-token` endpoint,
 * so it works before authentication. Renders nothing if the token doesn't
 * resolve (invalid/expired) — the login form still stands on its own.
 */
export function InvitePreviewBanner({ token }: { token: string }) {
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
  const Icon = context.type === 'Match' ? Swords : Users
  const name =
    context.type === 'Match'
      ? context.match_name || 'a match'
      : context.team_name || 'a team'

  return (
    <div className="mb-6 flex items-center gap-3 rounded-xl border bg-card p-4 text-left">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Icon className="size-5" />
      </div>
      <div className="min-w-0 text-sm">
        <p className="font-medium">You've been invited to join {name}</p>
        <p className="text-muted-foreground">
          Sign in or create an account to accept.
        </p>
      </div>
    </div>
  )
}
