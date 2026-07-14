/**
 * A pending invite token, persisted across the auth round-trip.
 *
 * When someone opens an invite link (`/invite/:token`) they may not be signed
 * in yet. Login — especially OAuth, which redirects back to the app origin and
 * drops the path — would otherwise lose the token. We stash it in localStorage
 * on landing, then consume it once the user is signed in with a profile.
 */
const KEY = 'agon-pending-invite'

export function setPendingInvite(token: string): void {
  try {
    localStorage.setItem(KEY, token)
  } catch {
    // Private mode / storage disabled — the in-URL token still works while the
    // tab survives; only cross-redirect persistence is lost.
  }
}

export function getPendingInvite(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

export function clearPendingInvite(): void {
  try {
    localStorage.removeItem(KEY)
  } catch {
    // ignore
  }
}
