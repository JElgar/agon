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

export interface RevokeDeviceDialogProps {
  deviceSub: string
  children: React.ReactNode
}

/**
 * Confirm-then-revoke a paired device (see `PairedDevicesPage`). Deletes the
 * device's `AUTH#<device_sub>` guard server-side, so any already-minted
 * token for it (e.g. a Garmin watch that's already paired) stops working
 * immediately — irreversible from here, the device would need to re-pair
 * from scratch, so this is a real confirmation, not a toast-and-undo.
 */
export function RevokeDeviceDialog({ deviceSub, children }: RevokeDeviceDialogProps) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)

  const mutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.DELETE('/devices/paired/{device_sub}', {
        params: { path: { device_sub: deviceSub } },
      })
      if (error) throw new Error('Could not revoke device')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['paired-devices'] })
      setOpen(false)
    },
  })

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Revoke this device?</DialogTitle>
        </DialogHeader>

        <p className="text-sm text-muted-foreground">
          It'll stop working immediately and won't be able to record anything
          on your account. You'd need to pair it again from scratch to use it
          here.
        </p>

        {mutation.isError && (
          <p className="text-sm text-destructive">Could not revoke device. Try again.</p>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => setOpen(false)} disabled={mutation.isPending}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending}
          >
            {mutation.isPending ? 'Revoking…' : 'Revoke device'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
