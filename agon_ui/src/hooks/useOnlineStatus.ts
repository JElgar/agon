import { useCallback, useEffect, useState } from 'react'

/**
 * Connectivity signal for offline live-scoring. `navigator.onLine` plus the
 * `online`/`offline` window events are the fast, free signal, but
 * `navigator.onLine` is known to be unreliable on its own (e.g. it reads
 * `true` behind a captive portal with no real route out) — so
 * `reportNetworkFailure`/`reportNetworkSuccess` let a caller that just
 * attempted a real request correct it in either direction. See
 * `useOfflineLiveScoring.ts`, the only caller: a network-classified append
 * failure calls `reportNetworkFailure` even if the browser still thinks
 * it's online, and a successful sync calls `reportNetworkSuccess`.
 */
export function useOnlineStatus() {
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

  const reportNetworkFailure = useCallback(() => setIsOnline(false), [])
  const reportNetworkSuccess = useCallback(() => setIsOnline(true), [])

  return { isOnline, reportNetworkFailure, reportNetworkSuccess }
}

/**
 * Whether a failed request looks like "no network," as opposed to the
 * server actually answering with an error. `fetch()` (and `openapi-fetch`,
 * which `agon_ui`'s `fetchClient` wraps — see `lib/api-client.ts`) rejects
 * with a `TypeError` when a request never gets a response at all: DNS
 * failure, connection refused, no route. That's exactly "offline, queue
 * this" — a real HTTP error status (400/403/409/...) resolves normally
 * instead of throwing, and should keep surfacing as today, not silently
 * queue.
 */
export function isNetworkError(error: unknown): boolean {
  return error instanceof TypeError
}
