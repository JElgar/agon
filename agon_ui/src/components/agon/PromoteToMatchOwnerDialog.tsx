import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'

export interface PromoteToMatchOwnerDialogProps {
  matchId: string
  /** The roster player being promoted. */
  playerId: string
  playerName: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Confirm-then-transfer a match's ownership to another accepted player
 * (`POST /matches/:id/transfer-ownership`). Controlled rather than
 * trigger-wrapped, same reasoning as the team version this mirrors
 * (`PromoteToOwnerDialog`) — it's opened from a `DropdownMenuItem` in
 * `SideRoster`, which unmounts as soon as an item is selected.
 *
 * The server demotes the caller (the current owner) to admin as part of
 * the same call, which is surprising enough — and hard enough to undo —
 * to warrant a real confirmation rather than a one-click toggle.
 */
export function PromoteToMatchOwnerDialog({
  matchId,
  playerId,
  playerName,
  open,
  onOpenChange,
}: PromoteToMatchOwnerDialogProps) {
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/matches/{match_id}/transfer-ownership', {
        params: { path: { match_id: matchId } },
        body: { player_id: playerId },
      })
      if (error) throw new Error('Could not transfer ownership')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      onOpenChange(false)
    },
  })

  const handleOpenChange = (next: boolean) => {
    if (!next) mutation.reset()
    onOpenChange(next)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Make {playerName} the owner?</DialogTitle>
        </DialogHeader>

        <p className="text-sm text-muted-foreground">
          {playerName} will become this match's owner and you'll become an admin. Only{' '}
          {playerName} will be able to transfer ownership after this.
        </p>

        {mutation.isError && (
          <p className="text-sm text-destructive">Could not transfer ownership. Try again.</p>
        )}

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => handleOpenChange(false)}
            disabled={mutation.isPending}
          >
            Cancel
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending ? 'Transferring…' : 'Transfer ownership'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
