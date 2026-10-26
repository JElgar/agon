import { WifiOff } from 'lucide-react'
import { useOnlineStatus } from '@/hooks/useOnlineStatus'

/** Sticky strip shown for as long as the device is offline, so pages that
 *  keep rendering cached data don't look like they've silently gone stale.
 *  No dismiss button: it reflects live connectivity and clears itself. */
export function OfflineBanner() {
  const isOnline = useOnlineStatus()

  if (isOnline) return null

  return (
    <div className="sticky top-0 z-20 flex items-center justify-center gap-2 bg-warning px-4 py-1.5 text-xs font-medium text-warning-foreground">
      <WifiOff className="size-3.5" />
      You're offline — showing saved data
    </div>
  )
}
