/**
 * A pending invite, join-link, or device-pairing code, persisted across the
 * auth round-trip.
 *
 * When someone opens an invite link (`/invite/:token`), a join link
 * (`/join/:token`), or a device-pairing link (`/pair?code=...`, scanned from
 * a Garmin watch's QR code — see docs/garmin-live-scoring.md) they may not
 * be signed in yet. Login — especially OAuth, which redirects back to the
 * app origin and drops the path — would otherwise lose the token/code. We
 * stash it (with which kind of link it was) in localStorage on landing, then
 * consume it once the user is signed in with a profile.
 */
const KEY = 'agon-pending-invite'

export type PendingInviteKind = 'invite' | 'join' | 'pair'

export interface PendingInvite {
  kind: PendingInviteKind
  /** The invite/join token, or (for `kind: 'pair'`) the pairing code. */
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
      (parsed.kind === 'invite' || parsed.kind === 'join' || parsed.kind === 'pair') &&
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
