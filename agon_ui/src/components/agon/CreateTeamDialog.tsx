import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogTrigger,
} from '@/components/ui/dialog'
import { ImageUploadField } from './ImageUploadField'
import { PlayerSideEditor, type TaggedPlayer } from './PlayerSideEditor'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type Team = components['schemas']['Team']

export interface CreateTeamDialogProps {
  children: React.ReactNode
  /** Called with the newly created team after a successful save. */
  onCreated?: (team: Team) => void
}

/**
 * Create-a-team dialog: name, an optional logo, and optional initial
 * invites — all sent in one `POST /teams`. The creator becomes the team's
 * owner server-side; anyone tagged here gets a real invite (pending roster
 * slot + a standalone invitation they can accept), the same as inviting them
 * after the fact via `POST /teams/{id}/invitations` — including the same
 * "invite as admin" choice, which applies to everyone tagged in this one
 * dialog (invite some as admin and others as plain members by creating the
 * team, then using `InviteToTeamDialog` for the second batch). On success,
 * invalidates `['my-teams']` so the caller's list refreshes with the new
 * team, then closes.
 */
export function CreateTeamDialog({ children, onCreated }: CreateTeamDialogProps) {
  const queryClient = useQueryClient()
  const currentUserId = useCurrentUserId()
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [assetId, setAssetId] = useState<string | null>(null)
  const [invitees, setInvitees] = useState<TaggedPlayer[]>([])
  const [inviteAsAdmin, setInviteAsAdmin] = useState(false)

  const canSave = name.trim() !== ''

  const mutation = useMutation({
    mutationFn: async (): Promise<Team> => {
      const body: components['schemas']['CreateTeamInput'] = {
        name: name.trim(),
        logo_asset_id: assetId ?? undefined,
        invited_user_ids: invitees
          .filter((p) => p.kind === 'user')
          .map((p) => p.id),
        invited_external_names: invitees
          .filter((p) => p.kind === 'external')
          .map((p) => p.name),
        invited_role: inviteAsAdmin ? 'admin' : undefined,
      }
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
      setAssetId(null)
      setInvitees([])
      setInviteAsAdmin(false)
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
            <Label>Team logo</Label>
            <ImageUploadField
              purpose="team_logo"
              shape="circle"
              label="Add a team logo"
              onUploaded={setAssetId}
            />
          </div>

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

          <div className="flex flex-col gap-1.5">
            <Label>Invite teammates (optional)</Label>
            <PlayerSideEditor
              title="Members"
              searchPlaceholder="Add a teammate…"
              players={invitees}
              onChange={setInvitees}
              currentUserId={currentUserId}
            />
            {invitees.length > 0 && (
              <div className="flex items-center justify-between rounded-lg border px-3 py-2">
                <div>
                  <Label htmlFor="invite-as-admin" className="text-sm">
                    Invite as admin
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    Everyone tagged above can manage the team, not just view it.
                  </p>
                </div>
                <Switch
                  id="invite-as-admin"
                  checked={inviteAsAdmin}
                  onCheckedChange={setInviteAsAdmin}
                />
              </div>
            )}
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
