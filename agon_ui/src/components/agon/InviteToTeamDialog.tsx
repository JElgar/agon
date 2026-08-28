import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
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
import { PlayerSideEditor, type TaggedPlayer } from './PlayerSideEditor'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

export interface InviteToTeamDialogProps {
  teamId: string
  /** Ids already on the team (members + already-pending invitees), so the
   *  search results don't offer someone twice. */
  excludeUserIds: string[]
  children: React.ReactNode
}

/**
 * Invite more people to an existing team — the after-the-fact counterpart of
 * `CreateTeamDialog`'s bundled initial invites, same `PlayerSideEditor` tagging
 * UI over `POST /teams/{id}/invitations` — including the same "invite as
 * admin" toggle, applied to the whole batch. Admin-only; the team page only
 * renders the trigger for an admin viewer. On success, invalidates the team's
 * members list (`['team-members', teamId]`) so the new (pending) members show
 * up immediately.
 */
export function InviteToTeamDialog({
  teamId,
  excludeUserIds,
  children,
}: InviteToTeamDialogProps) {
  const queryClient = useQueryClient()
  const currentUserId = useCurrentUserId()
  const [open, setOpen] = useState(false)
  const [invitees, setInvitees] = useState<TaggedPlayer[]>([])
  const [inviteAsAdmin, setInviteAsAdmin] = useState(false)

  const canSave = invitees.length > 0

  const mutation = useMutation({
    mutationFn: async () => {
      const body: components['schemas']['AddInvitationsInput'] = {
        invited_user_ids: invitees.filter((p) => p.kind === 'user').map((p) => p.id),
        invited_external_names: invitees
          .filter((p) => p.kind === 'external')
          .map((p) => p.name),
        role: inviteAsAdmin ? 'admin' : undefined,
      }
      const { error } = await fetchClient.POST('/teams/{team_id}/invitations', {
        params: { path: { team_id: teamId } },
        body,
      })
      if (error) throw new Error('Could not send invites')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team-members', teamId] })
      setOpen(false)
    },
  })

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
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
          <DialogTitle>Invite people</DialogTitle>
        </DialogHeader>

        <PlayerSideEditor
          title="Invitees"
          searchPlaceholder="Add a teammate…"
          players={invitees}
          onChange={setInvitees}
          currentUserId={currentUserId}
          excludeUserIds={excludeUserIds}
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

        {mutation.isError && (
          <p className="text-sm text-destructive">Could not send invites. Try again.</p>
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
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !canSave}
          >
            {mutation.isPending ? 'Inviting…' : 'Send invites'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
