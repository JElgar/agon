import { Bell, BellOff, Loader2 } from 'lucide-react'
import { usePushNotifications } from '@/hooks/usePushNotifications'
import { Button } from '@/components/ui/button'

/**
 * Opt-in prompt for push notifications on this device. Renders nothing when
 * push isn't available (unconfigured Firebase, unsupported browser) or is
 * already on — once enabled this makes room for a small "Turn off" control
 * instead, so the nudge doesn't linger after it's been acted on.
 */
export function PushNotificationsBanner() {
  const { status, error, enable, disable } = usePushNotifications()

  if (status === 'unsupported') return null

  if (status === 'on' || status === 'disabling') {
    return (
      <div className="mb-3 flex items-center justify-between gap-3 rounded-xl border bg-card px-4 py-2 text-sm text-muted-foreground">
        <span className="flex items-center gap-2">
          <Bell className="size-4" /> Push notifications are on for this device.
        </span>
        <Button
          variant="ghost"
          size="sm"
          disabled={status === 'disabling'}
          onClick={disable}
        >
          {status === 'disabling' && <Loader2 className="size-4 animate-spin" />}
          Turn off
        </Button>
      </div>
    )
  }

  return (
    <div className="mb-3 flex items-center justify-between gap-3 rounded-xl border bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <BellOff className="size-5 shrink-0 text-muted-foreground" />
        <div>
          <p className="text-sm font-medium">Get notified on this device</p>
          <p className="text-xs text-muted-foreground">
            {error ?? 'Invites, comments and more, even when Agon isn’t open.'}
          </p>
        </div>
      </div>
      <Button
        size="sm"
        disabled={status === 'enabling'}
        onClick={enable}
      >
        {status === 'enabling' && <Loader2 className="size-4 animate-spin" />}
        Enable
      </Button>
    </div>
  )
}
