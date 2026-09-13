import { useNavigate } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { ChevronLeft, Watch } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { RevokeDeviceDialog } from '@/components/agon/RevokeDeviceDialog'
import { relativeTime } from '@/lib/datetime'

type PairedDeviceInfo = components['schemas']['PairedDeviceInfo']

/**
 * "Paired devices" — everything paired to the viewer's own account via the
 * device-pairing flow (a Garmin watch, so far — see
 * docs/garmin-live-scoring.md), with a revoke action on each. Reached from
 * the profile page's Account section. There's no "add a device" flow here:
 * pairing only ever starts on the device itself (`/pair?code=...`), never
 * from this page.
 */
export function PairedDevicesPage() {
  const navigate = useNavigate()

  const query = useQuery({
    queryKey: ['paired-devices'],
    queryFn: async (): Promise<PairedDeviceInfo[]> => {
      const { data, error } = await fetchClient.GET('/devices/paired')
      if (error || !data) throw new Error('Failed to load paired devices')
      return data
    },
  })

  return (
    <div className="mx-auto flex max-w-md flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="icon" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="text-lg font-semibold">Paired devices</h1>
      </div>

      {query.isLoading && (
        <p className="text-sm text-muted-foreground">Loading…</p>
      )}

      {query.isError && (
        <p className="text-sm text-destructive">Couldn't load your paired devices.</p>
      )}

      {query.data && query.data.length === 0 && (
        <div className="flex flex-col items-center gap-2 rounded-xl border bg-card p-8 text-center">
          <Watch className="size-8 text-muted-foreground" />
          <p className="text-sm text-muted-foreground">
            No devices paired yet. Scan the pairing QR code on a Garmin watch
            to pair one.
          </p>
        </div>
      )}

      {query.data && query.data.length > 0 && (
        <div className="flex flex-col gap-2">
          {query.data.map((device) => (
            <div
              key={device.device_sub}
              className="flex items-center justify-between gap-3 rounded-xl border bg-card p-3"
            >
              <div className="flex min-w-0 items-center gap-3">
                <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
                  <Watch className="size-5" />
                </div>
                <div className="min-w-0">
                  <p className="text-sm font-medium">Garmin device</p>
                  <p className="text-xs text-muted-foreground">
                    Paired {relativeTime(device.paired_at)}
                  </p>
                </div>
              </div>
              <RevokeDeviceDialog deviceSub={device.device_sub}>
                <Button variant="outline" size="sm">
                  Revoke
                </Button>
              </RevokeDeviceDialog>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
