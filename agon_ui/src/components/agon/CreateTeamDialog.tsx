import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogTrigger,
} from '@/components/ui/dialog'

type Team = components['schemas']['Team']

export interface CreateTeamDialogProps {
  children: React.ReactNode
  /** Called with the newly created team after a successful save. */
  onCreated?: (team: Team) => void
}

/**
 * Create-a-team dialog: a name, `POST /teams`. The creator becomes the team's
 * first (admin) member server-side — nothing to set up here. On success,
 * invalidates `['my-teams']` so the caller's list refreshes with the new team,
 * then closes.
 */
export function CreateTeamDialog({ children, onCreated }: CreateTeamDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')

  const canSave = name.trim() !== ''

  const mutation = useMutation({
    mutationFn: async (): Promise<Team> => {
      const body: components['schemas']['CreateTeamInput'] = { name: name.trim() }
      const { data, error } = await fetchClient.POST('/teams', { body })
      if (error || !data) throw new Error('Could not create team')
      return data
    },
    onSuccess: (team) => {
      queryClient.invalidateQueries({ queryKey: ['my-teams'] })
      setOpen(false)
      onCreated?.(team)
    },
  })

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      // Reset for a fresh dialog each time it opens.
      setName('')
      mutation.reset()
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create a team</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="create-team-name">Team name</Label>
            <Input
              id="create-team-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. Kent CC"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter' && canSave) mutation.mutate()
              }}
            />
          </div>

          {mutation.isError && (
            <p className="text-sm text-destructive">Could not create team. Try again.</p>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => setOpen(false)}
            disabled={mutation.isPending}
          >
            Cancel
          </Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !canSave}
          >
            {mutation.isPending ? 'Creating…' : 'Create team'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
