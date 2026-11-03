import { useEffect, useState } from 'react'

/** Tracks `navigator.onLine`, updated via the browser's `online`/`offline`
 *  events. This only reflects whether the device has a network interface
 *  up, not whether the API is actually reachable — good enough to decide
 *  when to show the "you're offline" banner and skip network-dependent
 *  gates. */
export function useOnlineStatus(): boolean {
  const [isOnline, setIsOnline] = useState(() => navigator.onLine)

  useEffect(() => {
    const goOnline = () => setIsOnline(true)
    const goOffline = () => setIsOnline(false)
    window.addEventListener('online', goOnline)
    window.addEventListener('offline', goOffline)
    return () => {
      window.removeEventListener('online', goOnline)
      window.removeEventListener('offline', goOffline)
    }
  }, [])

  return isOnline
}
