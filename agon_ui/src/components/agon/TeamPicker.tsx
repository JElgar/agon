import { useEffect, useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Search, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'

type TeamListItem = components['schemas']['TeamListItem']

/** How long to wait after typing stops before hitting `/teams/search`. */
const SEARCH_DEBOUNCE_MS = 300
/** How many of the user's own teams to hold client-side for instant, as-you-type
 *  filtering. `GET /users/me/teams` is cheap — team metadata only, no member
 *  lists — so pulling a generous page up front avoids re-querying per keystroke.
 *  Fetched at the server's own page cap (see `MAX_PAGE_LIMIT`), one page at a time. */
const MY_TEAMS_LIMIT = 100
const MY_TEAMS_PAGE_LIMIT = 50

export interface TeamPickerProps {
  /** The team currently linked to this side, or `null` for an ad-hoc side. */
  team: TeamListItem | null
  onChange: (team: TeamListItem | null) => void
  placeholder?: string
}

/**
 * Link a match side to a persistent Team, instead of (or as well as) tagging
 * players onto it directly. Always searches both, in two sections:
 *   - "Your teams" — the signed-in user's own teams (`GET /users/me/teams`,
 *     fetched once up to `MY_TEAMS_LIMIT` and filtered client-side as you
 *     type), the common case of playing as your own team.
 *   - "Other teams" — every team via `GET /teams/search`, once you've typed
 *     2+ characters (debounced) — excludes anything already listed above.
 *
 * Once a team is picked it collapses to a removable chip; clearing it reverts
 * the side to ad-hoc (manually-tagged players / a typed name). Purely
 * controlled — the parent (`LogMatchPage`) owns the selection and, since the
 * server rejects a custom name alongside a team unless the *other* side
 * shares it, decides from there whether that side's name field still applies.
 */
export function TeamPicker({ team, onChange, placeholder = 'Link a team…' }: TeamPickerProps) {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')
  const [focused, setFocused] = useState(false)

  useEffect(() => {
    const t = setTimeout(() => setDebounced(term.trim()), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [term])

  // Fetched once (not re-queried per keystroke or gated on the search term) —
  // paginates at the server's page cap until MY_TEAMS_LIMIT is reached or the
  // user runs out of teams, whichever comes first.
  const myTeams = useQuery({
    queryKey: ['my-teams-picker'],
    queryFn: async (): Promise<TeamListItem[]> => {
      const items: TeamListItem[] = []
      let cursor: string | undefined
      while (items.length < MY_TEAMS_LIMIT) {
        const { data, error } = await fetchClient.GET('/users/me/teams', {
          params: { query: { cursor, limit: MY_TEAMS_PAGE_LIMIT } },
        })
        if (error || !data) throw new Error('Failed to load teams')
        items.push(...data.items)
        if (!data.next_cursor) break
        cursor = data.next_cursor
      }
      return items
    },
  })

  const searching = debounced.length >= 2
  const search = useQuery({
    queryKey: ['teams-search', debounced],
    enabled: searching,
    queryFn: async (): Promise<TeamListItem[]> => {
      const { data, error } = await fetchClient.GET('/teams/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Search failed')
      return data.items
    },
  })

  const myTeamIds = useMemo(
    () => new Set((myTeams.data ?? []).map((t) => t.id)),
    [myTeams.data],
  )

  // "Your teams" filters the already-fetched full list in-memory against the
  // raw (undebounced) term, for instant feedback on every keystroke.
  const termLower = term.trim().toLowerCase()
  const myMatches = (myTeams.data ?? []).filter((t) =>
    termLower ? t.name.toLowerCase().includes(termLower) : true,
  )
  // "Other teams" only ever adds teams not already offered above, so a team
  // you're on doesn't get listed twice just because search finds it too.
  const otherMatches = searching ? (search.data ?? []).filter((t) => !myTeamIds.has(t.id)) : []

  if (team) {
    return (
      <div className="mb-2 flex items-center gap-2 rounded-md bg-card px-2 py-1.5">
        <Avatar name={team.name} imageUrl={team.logo?.image_url} size="md" />
        <span className="flex-1 truncate text-sm font-medium">{team.name}</span>
        <button
          type="button"
          onClick={() => onChange(null)}
          className="text-muted-foreground transition-colors hover:text-foreground"
          aria-label={`Unlink ${team.name}`}
        >
          <X className="size-4" />
        </button>
      </div>
    )
  }

  const pick = (t: TeamListItem) => {
    onChange(t)
    setTerm('')
    setDebounced('')
  }

  const nothingFound =
    !myTeams.isLoading && !search.isLoading && myMatches.length === 0 && otherMatches.length === 0

  return (
    <div className="relative mb-2">
      <div className="flex items-center gap-2 rounded-md border bg-card px-2.5 py-1.5">
        <Search className="size-4 shrink-0 text-muted-foreground" />
        <input
          type="text"
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setTimeout(() => setFocused(false), 150)}
          placeholder={placeholder}
          className="w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>

      {focused && (
        <div className="absolute z-10 mt-1 max-h-72 w-full overflow-y-auto rounded-md border bg-card shadow-md">
          {myTeams.isLoading && (
            <p className="px-3 py-2 text-xs text-muted-foreground">Loading your teams…</p>
          )}

          {!myTeams.isLoading && myMatches.length > 0 && (
            <>
              <p className="px-3 py-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                Your teams
              </p>
              {myMatches.map((t) => (
                <TeamRow key={t.id} team={t} onClick={() => pick(t)} />
              ))}
            </>
          )}

          {searching && (
            <>
              <p className="border-t px-3 py-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                Other teams
              </p>
              {search.isLoading && (
                <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>
              )}
              {!search.isLoading &&
                otherMatches.map((t) => <TeamRow key={t.id} team={t} onClick={() => pick(t)} />)}
              {!search.isLoading && otherMatches.length === 0 && (
                <p className="px-3 py-2 text-xs text-muted-foreground">No other teams found.</p>
              )}
            </>
          )}

          {nothingFound && (
            <p className="px-3 py-2 text-xs text-muted-foreground">
              {term.trim() ? 'No teams found.' : "You're not on any teams yet."}
            </p>
          )}
        </div>
      )}
    </div>
  )
}

function TeamRow({ team, onClick }: { team: TeamListItem; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-muted"
    >
      <Avatar name={team.name} imageUrl={team.logo?.image_url} size="md" />
      <span className="flex-1 truncate text-sm">{team.name}</span>
    </button>
  )
}
