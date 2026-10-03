import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { PersonAvatar } from '@/components/agon/football/FootballMatchView'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { memberName, memberAvatarUrl, playerId } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']

export interface LeaveMatchDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  match: Match
  /** The viewer's own player row — decides which flow renders. */
  myPlayer: MatchPlayer
  /** Accepted players to offer as a new owner when the viewer is the owner
   *  (the screen's own `goingPlayers(match)`, so it stays consistent with
   *  who the "going" list already shows). */
  going: MatchPlayer[]
  onLeft: () => void
}

/**
 * Leave a scheduled match. A plain player or admin just confirms and leaves
 * — `POST /matches/:id/leave` handles it in one call. The owner can't leave
 * directly (the server rejects it): this dialog has them pick a new owner
 * first, then runs the transfer (`POST /matches/:id/transfer-ownership`) and
 * the leave as two calls in sequence, so from the viewer's side it's still
 * one action. Mirrors `LeaveTeamDialog`'s own split exactly.
 */
export function LeaveMatchDialog({
  open,
  onOpenChange,
  match,
  myPlayer,
  going,
  onLeft,
}: LeaveMatchDialogProps) {
  const queryClient = useQueryClient()
  const [selectedPlayerId, setSelectedPlayerId] = useState<string | null>(null)

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['match', match.id] })
    queryClient.invalidateQueries({ queryKey: ['feed'] })
    queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
  }

  const leaveMutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/matches/{match_id}/leave', {
        params: { path: { match_id: match.id } },
      })
      if (error) throw new Error('Could not leave the match')
    },
    onSuccess: () => {
      invalidate()
      onOpenChange(false)
      onLeft()
    },
  })

  const transferAndLeaveMutation = useMutation({
    mutationFn: async (newOwnerPlayerId: string) => {
      const { error: transferError } = await fetchClient.POST(
        '/matches/{match_id}/transfer-ownership',
        {
          params: { path: { match_id: match.id } },
          body: { player_id: newOwnerPlayerId },
        },
      )
      if (transferError) throw new Error('Could not transfer ownership')
      const { error: leaveError } = await fetchClient.POST('/matches/{match_id}/leave', {
        params: { path: { match_id: match.id } },
      })
      if (leaveError) throw new Error('Ownership transferred, but leaving failed — try again')
    },
    onSuccess: () => {
      invalidate()
      onOpenChange(false)
      onLeft()
    },
  })

  const handleOpenChange = (next: boolean) => {
    onOpenChange(next)
    if (next) {
      setSelectedPlayerId(null)
      leaveMutation.reset()
      transferAndLeaveMutation.reset()
    }
  }

  if (myPlayer.role !== 'owner') {
    const busy = leaveMutation.isPending
    return (
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Leave {match.name}?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            You'll need to be invited or join again to rejoin.
          </p>
          {leaveMutation.isError && (
            <p className="text-sm text-destructive">Could not leave the match. Try again.</p>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => leaveMutation.mutate()} disabled={busy}>
              {busy ? 'Leaving…' : 'Leave match'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }

  // Owner flow: pick someone else (accepted, not yourself) to hand the role
  // to first.
  const myId = playerId(myPlayer)
  const candidates = going.filter((p) => p.member.type === 'User' && playerId(p) !== myId && p.role !== 'owner')
  const busy = transferAndLeaveMutation.isPending

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Leave {match.name}?</DialogTitle>
        </DialogHeader>

        {candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            You're the only one going, so there's no one to hand ownership to.
            Invite someone else first, or cancel the match instead.
          </p>
        ) : (
          <>
            <p className="text-sm text-muted-foreground">
              As owner, you need to hand the role to someone else before you
              can leave. Pick who takes over:
            </p>
            <ul className="flex max-h-64 flex-col divide-y overflow-y-auto rounded-lg border">
              {candidates.map((p) => {
                const id = playerId(p)
                const selected = selectedPlayerId === id
                return (
                  <li key={id}>
                    <button
                      type="button"
                      onClick={() => setSelectedPlayerId(id)}
                      className={cn(
                        'flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted',
                        selected && 'bg-muted',
                      )}
                    >
                      <PersonAvatar name={memberName(p.member)} imageUrl={memberAvatarUrl(p.member)} size={32} />
                      <span className="flex-1 truncate text-sm">{memberName(p.member)}</span>
                      <span className="text-xs capitalize text-muted-foreground">{p.role}</span>
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
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          {candidates.length > 0 && (
            <Button
              variant="destructive"
              disabled={busy || !selectedPlayerId}
              onClick={() => selectedPlayerId && transferAndLeaveMutation.mutate(selectedPlayerId)}
            >
              {busy ? 'Leaving…' : 'Transfer ownership & leave'}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
