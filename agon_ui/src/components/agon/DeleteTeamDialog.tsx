import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogTrigger,
} from '@/components/ui/dialog'

export interface DeleteTeamDialogProps {
  teamId: string
  teamName: string
  children: React.ReactNode
  /** Called after a successful delete (e.g. navigate away). */
  onDeleted: () => void
}

/**
 * Confirm-then-delete a team. Owner-only — the team page only renders the
 * trigger for an owner viewer, and the server rejects anyone else anyway.
 * Irreversible (the server cascades: every member and follower goes with
 * it), so this is a real confirmation, not a toast-and-undo.
 */
export function DeleteTeamDialog({
  teamId,
  teamName,
  children,
  onDeleted,
}: DeleteTeamDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)

  const mutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.DELETE('/teams/{team_id}', {
        params: { path: { team_id: teamId } },
      })
      if (error) throw new Error('Could not delete team')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['my-teams'] })
      setOpen(false)
      onDeleted()
    },
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete {teamName}?</DialogTitle>
        </DialogHeader>

        <p className="text-sm text-muted-foreground">
          This permanently deletes the team and removes every member. This
          can't be undone.
        </p>

        {mutation.isError && (
          <p className="text-sm text-destructive">Could not delete team. Try again.</p>
        )}

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => setOpen(false)}
            disabled={mutation.isPending}
          >
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending}
          >
            {mutation.isPending ? 'Deleting…' : 'Delete team'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
