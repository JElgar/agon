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

/**
 * An entry is only ever removed by `forgetJoinLink` — on a successful join,
 * or when `JoinLinkBanner` notices its link no longer resolves, which only
 * happens if the viewer revisits that exact match while still not a
 * participant. A link previewed via "View match" and then abandoned
 * otherwise sits here forever, so this cap bounds it regardless: oldest
 * (least-recently-seen) entries are evicted first once there are too many.
 */
const MAX_ENTRIES = 20

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
  // Delete-then-reinsert moves the key to the end — plain object insertion
  // order for string keys — so eviction below always drops the entries
  // least recently seen, not just the numerically oldest ones.
  delete map[matchId]
  map[matchId] = token
  const keys = Object.keys(map)
  if (keys.length > MAX_ENTRIES) {
    for (const staleId of keys.slice(0, keys.length - MAX_ENTRIES)) {
      delete map[staleId]
    }
  }
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
