import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogTrigger,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { memberName, memberAvatarUrl } from '@/lib/members'

type TeamMember = components['schemas']['TeamMember']
type TeamRole = components['schemas']['TeamRole']

export interface LeaveTeamDialogProps {
  teamId: string
  teamName: string
  /** The viewer's own role — decides which flow renders. */
  myRole: TeamRole
  /** The team's currently-loaded members (same list the page already has) —
   *  only used to offer a new-owner picker when the viewer is the owner. */
  members: TeamMember[]
  currentUserId?: string
  children: React.ReactNode
  /** Called after the viewer has actually left. */
  onLeft: () => void
}

/**
 * Leave a team. A plain member or admin just confirms and leaves — `POST
 * /teams/:id/leave` handles it in one call. The owner can't leave directly
 * (the server rejects it): this dialog has them pick a new owner first, then
 * runs the transfer (`POST /teams/:id/transfer-ownership`) and the leave as
 * two calls in sequence, so from the viewer's side it's still one action.
 */
export function LeaveTeamDialog({
  teamId,
  teamName,
  myRole,
  members,
  currentUserId,
  children,
  onLeft,
}: LeaveTeamDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [selectedMembershipId, setSelectedMembershipId] = useState<string | null>(null)

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['team-members', teamId] })
    queryClient.invalidateQueries({ queryKey: ['my-teams'] })
  }

  const leaveMutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/teams/{team_id}/leave', {
        params: { path: { team_id: teamId } },
      })
      if (error) throw new Error('Could not leave team')
    },
    onSuccess: () => {
      invalidate()
      setOpen(false)
      onLeft()
    },
  })

  const transferAndLeaveMutation = useMutation({
    mutationFn: async (newOwnerMembershipId: string) => {
      const { error: transferError } = await fetchClient.POST(
        '/teams/{team_id}/transfer-ownership',
        {
          params: { path: { team_id: teamId } },
          body: { member_id: newOwnerMembershipId },
        },
      )
      if (transferError) throw new Error('Could not transfer ownership')
      const { error: leaveError } = await fetchClient.POST('/teams/{team_id}/leave', {
        params: { path: { team_id: teamId } },
      })
      if (leaveError) throw new Error('Ownership transferred, but leaving failed — try again')
    },
    onSuccess: () => {
      invalidate()
      setOpen(false)
      onLeft()
    },
  })

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      setSelectedMembershipId(null)
      leaveMutation.reset()
      transferAndLeaveMutation.reset()
    }
  }

  if (myRole !== 'owner') {
    const busy = leaveMutation.isPending
    return (
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogTrigger asChild>{children}</DialogTrigger>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Leave {teamName}?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            You'll need to be added or invited again to rejoin.
          </p>
          {leaveMutation.isError && (
            <p className="text-sm text-destructive">Could not leave team. Try again.</p>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => setOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => leaveMutation.mutate()}
              disabled={busy}
            >
              {busy ? 'Leaving…' : 'Leave team'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }

  // Owner flow: pick someone else (accepted, not yourself) to hand the role
  // to first.
  const candidates = members.filter(
    (m) =>
      m.member.type === 'User' &&
      m.member.user_id !== currentUserId &&
      !m.member.invitation &&
      m.role !== 'owner',
  )
  const busy = transferAndLeaveMutation.isPending

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Leave {teamName}?</DialogTitle>
        </DialogHeader>

        {candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            You're the only member of this team, so there's no one to hand
            ownership to. Add someone else first, or delete the team instead.
          </p>
        ) : (
          <>
            <p className="text-sm text-muted-foreground">
              As owner, you need to hand the role to someone else before you
              can leave. Pick who takes over:
            </p>
            <ul className="flex max-h-64 flex-col divide-y overflow-y-auto rounded-lg border">
              {candidates.map((m) => {
                const id = m.member.id
                const selected = selectedMembershipId === id
                return (
                  <li key={id}>
                    <button
                      type="button"
                      onClick={() => setSelectedMembershipId(id)}
                      className={cn(
                        'flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted',
                        selected && 'bg-muted',
                      )}
                    >
                      <Avatar
                        name={memberName(m.member)}
                        imageUrl={memberAvatarUrl(m.member)}
                        size="md"
                      />
                      <span className="flex-1 truncate text-sm">{memberName(m.member)}</span>
                      <span className="text-xs capitalize text-muted-foreground">{m.role}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          </>
        )}

        {transferAndLeaveMutation.isError && (
          <p className="text-sm text-destructive">
            {transferAndLeaveMutation.error instanceof Error
              ? transferAndLeaveMutation.error.message
              : 'Something went wrong. Try again.'}
          </p>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => setOpen(false)} disabled={busy}>
            Cancel
          </Button>
          {candidates.length > 0 && (
            <Button
              variant="destructive"
              disabled={busy || !selectedMembershipId}
              onClick={() =>
                selectedMembershipId && transferAndLeaveMutation.mutate(selectedMembershipId)
              }
            >
              {busy ? 'Leaving…' : 'Transfer ownership & leave'}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
