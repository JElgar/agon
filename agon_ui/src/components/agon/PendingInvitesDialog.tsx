import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import { Avatar } from '@/components/agon/Avatar'
import { memberName, memberAvatarUrl, pendingInvitees } from '@/lib/members'

type Match = components['schemas']['Match']

/** Display label for the side a pending invitee was placed on, or
 *  "Unassigned" if none. */
function sideNameFor(match: Match, sideId: string | null | undefined): string {
  if (!sideId) return 'Unassigned'
  const side = match.sides.find((s) => s.id === sideId)
  return side?.name?.trim() || 'Unassigned'
}

/**
 * "Review the final roster" flow — every player on `match` whose invitation
 * is still pending, with a one-click Revoke per row. Opened from
 * `PendingInvitesNudge`, which surfaces it as a dismissible heads-up before
 * kickoff or a result is published: a still-pending invite next to a game
 * that's clearly already happened without that person is usually stale, not
 * something to keep waiting on.
 *
 * Match admin only to open (mirrors who can send/revoke invites elsewhere);
 * callers gate that themselves.
 */
export function PendingInvitesDialog({
  open,
  onOpenChange,
  match,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  match: Match
}) {
  const queryClient = useQueryClient()
  const invitees = pendingInvitees(match)

  const revoke = useMutation({
    mutationFn: async (invitationId: string) => {
      const { error } = await fetchClient.DELETE('/invitations/{invitation_id}', {
        params: { path: { invitation_id: invitationId } },
      })
      if (error) throw new Error('Failed to revoke that invite')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Review the final roster</DialogTitle>
          <DialogDescription>
            These invites are still unanswered. Revoke any that clearly aren't
            playing, or leave them — they don't count toward the roster
            either way.
          </DialogDescription>
        </DialogHeader>

        {invitees.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Nothing pending anymore.
          </p>
        ) : (
          <div className="space-y-2">
            {invitees.map((p) => (
              <div
                key={p.member.id}
                className="flex items-center justify-between gap-2 rounded-lg border bg-muted/30 px-2.5 py-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  <Avatar name={memberName(p.member)} imageUrl={memberAvatarUrl(p.member)} size="sm" />
                  <div className="min-w-0">
                    <span className="block truncate text-sm font-medium">
                      {memberName(p.member)}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {sideNameFor(match, p.side_id)} · invited
                    </span>
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={revoke.isPending && revoke.variables === p.member.invitation!.id}
                  onClick={() => revoke.mutate(p.member.invitation!.id)}
                >
                  Revoke
                </Button>
              </div>
            ))}
          </div>
        )}

        {revoke.isError && (
          <p className="text-xs text-destructive">Something went wrong. Try again.</p>
        )}
      </DialogContent>
    </Dialog>
  )
}
