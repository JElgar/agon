/**
 * A pending invite or join-link token, persisted across the auth round-trip.
 *
 * When someone opens an invite link (`/invite/:token`) or a join link
 * (`/join/:token`) they may not be signed in yet. Login — especially OAuth,
 * which redirects back to the app origin and drops the path — would
 * otherwise lose the token. We stash it (with which kind of link it was) in
 * localStorage on landing, then consume it once the user is signed in with a
 * profile.
 */
const KEY = 'agon-pending-invite'

export type PendingInviteKind = 'invite' | 'join'

export interface PendingInvite {
  kind: PendingInviteKind
  token: string
}

export function setPendingInvite(kind: PendingInviteKind, token: string): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ kind, token } satisfies PendingInvite))
  } catch {
    // Private mode / storage disabled — the in-URL token still works while the
    // tab survives; only cross-redirect persistence is lost.
  }
}

export function getPendingInvite(): PendingInvite | null {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (
      typeof parsed === 'object' &&
      parsed !== null &&
      'kind' in parsed &&
      'token' in parsed &&
      (parsed.kind === 'invite' || parsed.kind === 'join') &&
      typeof parsed.token === 'string'
    ) {
      return parsed as PendingInvite
    }
    return null
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
