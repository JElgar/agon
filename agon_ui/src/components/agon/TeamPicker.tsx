import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Search, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'

type TeamListItem = components['schemas']['TeamListItem']

/** How long to wait after typing stops before hitting `/teams/search`. */
const SEARCH_DEBOUNCE_MS = 300

export interface TeamPickerProps {
  /** The team currently linked to this side, or `null` for an ad-hoc side. */
  team: TeamListItem | null
  onChange: (team: TeamListItem | null) => void
  placeholder?: string
}

/**
 * Link a match side to a persistent Team, instead of (or as well as) tagging
 * players onto it directly. Defaults to the signed-in user's own teams
 * (`GET /users/me/teams`) — the common case of playing as your own team — and
 * falls through to `GET /teams/search` once the user types 2+ characters.
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

  const searching = debounced.length >= 2

  const myTeams = useQuery({
    queryKey: ['my-teams-picker'],
    enabled: !searching,
    queryFn: async (): Promise<TeamListItem[]> => {
      const { data, error } = await fetchClient.GET('/users/me/teams', {
        params: { query: { limit: 6 } },
      })
      if (error || !data) throw new Error('Failed to load teams')
      return data.items
    },
  })

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

  const results = searching ? (search.data ?? []) : (myTeams.data ?? [])
  const loading = searching ? search.isLoading : myTeams.isLoading
  // Hide the dropdown for a 1-character term (too short to search, and no
  // longer showing "your teams" either) so it doesn't flash empty states.
  const showDropdown = focused && (term.trim().length === 0 || searching)

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

      {showDropdown && (
        <div className="absolute z-10 mt-1 w-full overflow-hidden rounded-md border bg-card shadow-md">
          {!searching && (
            <p className="px-3 py-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
              Your teams
            </p>
          )}
          {loading && <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>}
          {!loading &&
            results.map((t) => (
              <button
                key={t.id}
                type="button"
                onClick={() => {
                  onChange(t)
                  setTerm('')
                  setDebounced('')
                }}
                className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-muted"
              >
                <Avatar name={t.name} imageUrl={t.logo?.image_url} size="md" />
                <span className="flex-1 truncate text-sm">{t.name}</span>
              </button>
            ))}
          {!loading && results.length === 0 && (
            <p className="px-3 py-2 text-xs text-muted-foreground">
              {searching ? 'No teams found.' : "You're not on any teams yet."}
            </p>
          )}
        </div>
      )}
    </div>
  )
}
