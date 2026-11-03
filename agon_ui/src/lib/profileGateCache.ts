/**
 * Remembers whether the signed-in account last resolved to a completed Agon
 * signup ("has-profile"), so a network failure on the `/users/me` gate check
 * (e.g. no connection) doesn't block someone who has already been let into
 * the app from getting back in and seeing their cached data. Only ever set
 * from a real, successful response — a 404 clears it, since that's a real
 * "no profile yet" answer, not a network failure.
 */
const KEY = 'agon-has-profile'

export function setHasProfileCache(hasProfile: boolean): void {
  try {
    if (hasProfile) {
      localStorage.setItem(KEY, 'true')
    } else {
      localStorage.removeItem(KEY)
    }
  } catch {
    // Private mode / storage disabled — just no offline fallback for the gate.
  }
}

export function getHasProfileCache(): boolean {
  try {
    return localStorage.getItem(KEY) === 'true'
  } catch {
    return false
  }
}
