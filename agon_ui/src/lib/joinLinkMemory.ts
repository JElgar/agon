/**
 * Join-link tokens the viewer has previewed but not yet used to join,
 * remembered per match so `POST /matches/:id/join` can still be called with
 * that token from the match detail page itself — the same way a real
 * invitation stays actionable there (see `InviteBanner`). Read by
 * `JoinLinkBanner`, written by `JoinMatchPage` (the join-link landing
 * screen) as soon as a link resolves, so "View match" (leaving without
 * joining) doesn't lose it.
 *
 * Keyed by match id, unlike `pendingInvite`'s single slot — that one only
 * needs to survive a single auth round-trip; this needs to outlive the
 * visit, and the viewer may have previewed more than one game's link
 * without joining either.
 */
const KEY = 'agon-remembered-join-links'

type JoinLinkMap = Record<string, string>

function readAll(): JoinLinkMap {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as JoinLinkMap
    }
    return {}
  } catch {
    return {}
  }
}

function writeAll(map: JoinLinkMap): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(map))
  } catch {
    // Private mode / storage disabled — the link still works for this tab
    // session; only cross-visit persistence is lost.
  }
}

export function rememberJoinLink(matchId: string, token: string): void {
  const map = readAll()
  if (map[matchId] === token) return
  map[matchId] = token
  writeAll(map)
}

export function getRememberedJoinLink(matchId: string): string | null {
  return readAll()[matchId] ?? null
}

export function forgetJoinLink(matchId: string): void {
  const map = readAll()
  if (!(matchId in map)) return
  delete map[matchId]
  writeAll(map)
}
