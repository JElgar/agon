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
import { joinName, splitName } from '@/lib/names'

type UserProfile = components['schemas']['UserProfile']

export interface EditProfileDialogProps {
  profile: UserProfile
  children: React.ReactNode
}

/**
 * Edit the current user's own profile: first/last name + profile picture.
 * The API only stores a single `name` string, so the stored name is split
 * into first/last for editing and joined back before saving. The picture is
 * uploaded via the Asset API (see `ImageUploadField`); on save we PATCH
 * `/users/me` with the new name and, if a new image was uploaded, its
 * `profile_image_asset_id`. Invalidates the cached profile so the header
 * re-renders with the new image/name.
 */
export function EditProfileDialog({ profile, children }: EditProfileDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [firstName, setFirstName] = useState(() => splitName(profile.name).firstName)
  const [lastName, setLastName] = useState(() => splitName(profile.name).lastName)
  const [assetId, setAssetId] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const canSave = firstName.trim() !== '' && lastName.trim() !== ''

  const handleSave = async () => {
    if (!canSave) return
    setSaving(true)
    setError(null)
    try {
      const body: components['schemas']['UpdateUserInput'] = { name: joinName(firstName, lastName) }
      if (assetId) body.profile_image_asset_id = assetId
      const { error: patchErr } = await fetchClient.PATCH('/users/me', { body })
      if (patchErr) throw new Error('Could not save your profile')
      // Refresh the profile views (own + by-id) so the change shows immediately.
      await queryClient.invalidateQueries({ queryKey: ['profile'] })
      setOpen(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save your profile')
    } finally {
      setSaving(false)
    }
  }

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      // Reset to current values each time it opens.
      const split = splitName(profile.name)
      setFirstName(split.firstName)
      setLastName(split.lastName)
      setAssetId(null)
      setError(null)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit profile</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label>Profile picture</Label>
            <ImageUploadField
              purpose="profile_image"
              shape="circle"
              label="Upload a photo"
              initialUrl={profile.profile_image?.image_url}
              onUploaded={setAssetId}
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="edit-first-name">First name</Label>
              <Input
                id="edit-first-name"
                value={firstName}
                onChange={(e) => setFirstName(e.target.value)}
                placeholder="First name"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="edit-last-name">Last name</Label>
              <Input
                id="edit-last-name"
                value={lastName}
                onChange={(e) => setLastName(e.target.value)}
                placeholder="Last name"
              />
            </div>
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
