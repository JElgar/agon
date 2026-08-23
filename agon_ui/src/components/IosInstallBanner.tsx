import { Share, X } from 'lucide-react'
import { useIosAddToHomeScreenPrompt } from '@/hooks/useIosAddToHomeScreenPrompt'
import { Button } from '@/components/ui/button'

/**
 * Chrome/Android gets the native "Install app?" popup for free via
 * `beforeinstallprompt`. iOS Safari has no equivalent event — Apple simply
 * doesn't expose one — so the only install path there is the user manually
 * tapping Share -> "Add to Home Screen". This banner is the in-app stand-in
 * for that missing native prompt: shown once (per browser) to iOS Safari
 * visitors who haven't installed yet, dismissible for good.
 */
export function IosInstallBanner() {
  const { show, dismiss } = useIosAddToHomeScreenPrompt()

  if (!show) return null

  return (
    <div className="mb-6 flex items-center gap-3 rounded-xl border bg-card p-4 text-left">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Share className="size-5" />
      </div>
      <div className="min-w-0 flex-1 text-sm">
        <p className="font-medium">Install Agon on your device</p>
        <p className="text-muted-foreground">
          Tap <Share className="inline size-3.5 align-text-bottom" strokeWidth={2.5} /> in
          Safari, then "Add to Home Screen"
        </p>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="shrink-0"
        onClick={dismiss}
        aria-label="Dismiss"
      >
        <X className="size-4" />
      </Button>
    </div>
  )
}
