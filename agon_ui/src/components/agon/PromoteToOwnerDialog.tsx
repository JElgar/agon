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

export interface PromoteToOwnerDialogProps {
  teamId: string
  /** The membership being promoted. */
  memberId: string
  memberName: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Confirm-then-transfer a team's ownership to another accepted member
 * (`POST /teams/:id/transfer-ownership`). Controlled rather than
 * trigger-wrapped — it's opened from a `DropdownMenuItem` in `MemberRow`,
 * which unmounts as soon as an item is selected, so the dialog can't hang
 * off a `DialogTrigger` the way `DeleteTeamDialog`'s does.
 *
 * The server demotes the caller (the current owner) to admin as part of
 * the same call, which is surprising enough — and hard enough to undo —
 * to warrant a real confirmation rather than a one-click toggle.
 */
export function PromoteToOwnerDialog({
  teamId,
  memberId,
  memberName,
  open,
  onOpenChange,
}: PromoteToOwnerDialogProps) {
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/teams/{team_id}/transfer-ownership', {
        params: { path: { team_id: teamId } },
        body: { member_id: memberId },
      })
      if (error) throw new Error('Could not transfer ownership')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team-members', teamId] })
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
          <DialogTitle>Make {memberName} the owner?</DialogTitle>
        </DialogHeader>

        <p className="text-sm text-muted-foreground">
          {memberName} will become the team's owner and you'll become an admin. Only{' '}
          {memberName} will be able to transfer ownership or delete the team after this.
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
