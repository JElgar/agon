import { useState } from 'react'

/** Remembers that the user dismissed the banner, so it only nags once. */
const DISMISSED_KEY = 'agon-ios-a2hs-dismissed'

function isIosDevice(): boolean {
  const ua = window.navigator.userAgent
  if (/iphone|ipad|ipod/i.test(ua)) return true
  // iPadOS 13+ reports a desktop Safari UA (masquerading as macOS), but a
  // real Mac has no touch points to speak of.
  return navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1
}

function isSafari(): boolean {
  const ua = window.navigator.userAgent
  // Chrome/Firefox/Edge on iOS are all required to use WebKit under the
  // hood and their UA still contains "Safari", but they carry their own
  // token too — and only the real Safari can install a home-screen app.
  return /safari/i.test(ua) && !/crios|fxios|edgios|opios/i.test(ua)
}

function isStandalone(): boolean {
  // iOS Safari exposes this non-standard property once launched from a
  // home-screen icon; other engines report it via the media query instead.
  return (
    (window.navigator as { standalone?: boolean }).standalone === true ||
    window.matchMedia('(display-mode: standalone)').matches
  )
}

/**
 * iOS Safari never fires `beforeinstallprompt` — Apple has no equivalent API
 * — so there's no way to trigger a native install prompt there like on
 * Chrome/Android. The only install path is the user manually tapping
 * Share -> "Add to Home Screen", which can't be triggered from JS either.
 * This just detects "installable but not yet installed, on the one browser
 * that supports it" so the UI can show instructions instead.
 */
export function useIosAddToHomeScreenPrompt(): { show: boolean; dismiss: () => void } {
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(DISMISSED_KEY) === 'true',
  )

  const show = !dismissed && isIosDevice() && isSafari() && !isStandalone()

  const dismiss = () => {
    localStorage.setItem(DISMISSED_KEY, 'true')
    setDismissed(true)
  }

  return { show, dismiss }
}
