import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
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
import { ImageUploadField } from './ImageUploadField'

type Team = components['schemas']['Team']

export interface EditTeamDialogProps {
  team: Team
  children: React.ReactNode
}

/**
 * Edit a team's own name and logo — the team-side counterpart of
 * `EditProfileDialog`. Admin-only; the team page only renders the trigger for
 * an admin viewer. Invalidates the team's own query and `['my-teams']` (the
 * list also shows the name/logo) on save.
 */
export function EditTeamDialog({ team, children }: EditTeamDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [name, setName] = useState(team.name)
  const [assetId, setAssetId] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const canSave = name.trim() !== ''

  const handleSave = async () => {
    if (!canSave) return
    setSaving(true)
    setError(null)
    try {
      const body: components['schemas']['UpdateTeamInput'] = { name: name.trim() }
      if (assetId) body.logo_asset_id = assetId
      const { error: patchErr } = await fetchClient.PATCH('/teams/{team_id}', {
        params: { path: { team_id: team.id } },
        body,
      })
      if (patchErr) throw new Error('Could not save this team')
      await queryClient.invalidateQueries({ queryKey: ['team', team.id] })
      await queryClient.invalidateQueries({ queryKey: ['my-teams'] })
      setOpen(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save this team')
    } finally {
      setSaving(false)
    }
  }

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      // Reset to current values each time it opens.
      setName(team.name)
      setAssetId(null)
      setError(null)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit team</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label>Team logo</Label>
            <ImageUploadField
              purpose="team_logo"
              shape="circle"
              label="Add a team logo"
              initialUrl={team.logo?.image_url}
              onUploaded={setAssetId}
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-team-name">Team name</Label>
            <Input
              id="edit-team-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Team name"
            />
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => setOpen(false)}
            disabled={saving}
          >
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={saving || !canSave}>
            {saving ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
