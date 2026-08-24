import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  deleteToken,
  getMessaging,
  getToken,
  isSupported,
  onMessage,
  type Messaging,
} from 'firebase/messaging'
import { fetchClient } from '@/lib/api-client'
import {
  firebaseConfig,
  getFirebaseApp,
  isFirebaseConfigured,
  vapidKey,
} from '@/lib/firebase'

/**
 * Whether this device has (or is getting) a registered FCM token.
 *  - `unsupported`: no Firebase config shipped (e.g. the VAPID key hasn't
 *    been generated yet), or the browser can't do FCM (missing SW/Notification
 *    APIs, or firebase's own `isSupported()` check — Safari private mode etc).
 *  - `off`: supported, but not registered on this device.
 *  - `enabling` / `disabling`: a permission prompt or (un)registration is in flight.
 *  - `on`: registered — this device receives push notifications.
 *  - `error`: the last enable/disable attempt failed (see `error`).
 */
export type PushStatus = 'unsupported' | 'off' | 'enabling' | 'disabling' | 'on' | 'error'

interface PushNotificationsContextType {
  status: PushStatus
  error: string | null
  enable: () => void
  disable: () => void
}

const PushNotificationsContext = createContext<PushNotificationsContextType | undefined>(
  undefined,
)

/** localStorage flag distinguishing "never enabled" from "explicitly turned
 *  off" — `Notification.permission` alone can't tell them apart (it can't be
 *  un-requested via JS, so it stays 'granted' either way), and we only want to
 *  silently re-register on app load in the former case. */
const ENABLED_KEY = 'agon-push-enabled'

/** Scopes the FCM service worker to its own path so it can coexist with the
 *  PWA's workbox-managed service worker at the root scope — Firebase's own
 *  documented approach for apps that already have another SW controlling '/'.
 *  See public/firebase-messaging-sw.js for the other half of this. */
const SW_SCOPE = '/firebase-cloud-messaging-push-scope'

function swUrl(): string {
  // firebase-messaging-sw.js is a plain static asset, not bundled by Vite, so
  // it can't import src/lib/firebase.ts — pass the (public, client-safe)
  // config on the URL instead; it reads these back off its own registration.
  const params = new URLSearchParams(firebaseConfig)
  return `/firebase-messaging-sw.js?${params.toString()}`
}

async function registerAndGetToken(): Promise<{ messaging: Messaging; token: string }> {
  const registration = await navigator.serviceWorker.register(swUrl(), { scope: SW_SCOPE })
  const messaging = getMessaging(getFirebaseApp())
  // getToken/deleteToken are marked deprecated in favor of the newer
  // register()/onRegistered() FID-based API, but that's a different
  // registration-token model — ours (and the backend's push_token column)
  // is the plain FCM registration token these calls still fully support.
  const token = await getToken(messaging, { vapidKey, serviceWorkerRegistration: registration })
  if (!token) throw new Error('No registration token available')
  return { messaging, token }
}

async function postDevice(token: string): Promise<void> {
  const { error } = await fetchClient.POST('/devices', {
    body: { push_token: token, platform: 'web' },
  })
  if (error) throw new Error('Failed to register this device for push notifications')
}

async function unregisterDevice(token: string): Promise<void> {
  const { error, response } = await fetchClient.POST('/devices/unregister', {
    body: { push_token: token },
  })
  // 404 means it's already gone server-side — the end state we want either way.
  if (error && response.status !== 404) {
    throw new Error('Failed to unregister this device')
  }
}

/**
 * Owns the FCM registration lifecycle for this browser: silently re-registers
 * on load if this device was previously enabled, exposes `enable`/`disable`
 * for an explicit opt-in/out UI (see NotificationsPage), and refreshes the
 * notification queries when a push arrives while the app is in the
 * foreground (background pushes are handled entirely by the service worker —
 * see public/firebase-messaging-sw.js).
 *
 * Mount once (in AuthenticatedApp, alongside AuthProvider) — consumers read
 * state via `usePushNotifications()`.
 */
export function PushNotificationsProvider({ children }: { children: React.ReactNode }) {
  const queryClient = useQueryClient()
  const [status, setStatus] = useState<PushStatus>('off')
  const [error, setError] = useState<string | null>(null)
  const messagingRef = useRef<Messaging | null>(null)

  useEffect(() => {
    let cancelled = false

    async function init() {
      if (!isFirebaseConfigured()) return setStatus('unsupported')
      if (!('serviceWorker' in navigator) || !('Notification' in window)) {
        return setStatus('unsupported')
      }
      if (!(await isSupported())) return setStatus('unsupported')
      if (cancelled) return

      if (localStorage.getItem(ENABLED_KEY) !== 'true' || Notification.permission !== 'granted') {
        return setStatus('off')
      }

      try {
        const { messaging, token } = await registerAndGetToken()
        await postDevice(token)
        if (cancelled) return
        messagingRef.current = messaging
        setStatus('on')
      } catch {
        // Config/browser looked fine but registration failed (offline, FCM
        // hiccup, revoked permission out of sync with our flag, ...). Fall
        // back to 'off' rather than 'error' — this is a silent background
        // retry, not a user-initiated action, so there's nothing to surface.
        if (!cancelled) setStatus('off')
      }
    }

    init()
    return () => {
      cancelled = true
    }
  }, [])

  // Foreground messages: the backend already wrote the NotificationRecord the
  // push is about, so just refresh the same queries the in-app read paths
  // invalidate after any notification change (see NotificationsPage).
  useEffect(() => {
    const messaging = messagingRef.current
    if (status !== 'on' || !messaging) return
    return onMessage(messaging, () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({ queryKey: ['notifications-unread-count'] })
    })
  }, [status, queryClient])

  const enable = useCallback(() => {
    setStatus('enabling')
    setError(null)
    void (async () => {
      try {
        const permission = await Notification.requestPermission()
        if (permission !== 'granted') {
          setStatus('off')
          return
        }
        const { messaging, token } = await registerAndGetToken()
        await postDevice(token)
        messagingRef.current = messaging
        localStorage.setItem(ENABLED_KEY, 'true')
        setStatus('on')
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to enable push notifications')
        setStatus('error')
      }
    })()
  }, [])

  const disable = useCallback(() => {
    const messaging = messagingRef.current
    if (!messaging) return
    setStatus('disabling')
    setError(null)
    void (async () => {
      try {
        const token = await getToken(messaging, { vapidKey })
        if (token) await unregisterDevice(token)
        await deleteToken(messaging)
        localStorage.setItem(ENABLED_KEY, 'false')
        messagingRef.current = null
        setStatus('off')
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to turn off push notifications')
        setStatus('error')
      }
    })()
  }, [])

  return (
    <PushNotificationsContext.Provider value={{ status, error, enable, disable }}>
      {children}
    </PushNotificationsContext.Provider>
  )
}

export function usePushNotifications() {
  const context = useContext(PushNotificationsContext)
  if (context === undefined) {
    throw new Error('usePushNotifications must be used within a PushNotificationsProvider')
  }
  return context
}
