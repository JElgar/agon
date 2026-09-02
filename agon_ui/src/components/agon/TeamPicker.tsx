import { useEffect, useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import {
  Combobox,
  ComboboxCollection,
  ComboboxContent,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxList,
} from '@/components/ui/combobox'

type TeamListItem = components['schemas']['TeamListItem']

/** How long to wait after typing stops before hitting `/teams/search`. */
const SEARCH_DEBOUNCE_MS = 300
/** How many of the user's own teams to hold client-side for instant, as-you-type
 *  filtering. `GET /users/me/teams` is cheap — team metadata only, no member
 *  lists — so pulling a generous page up front avoids re-querying per keystroke.
 *  Fetched at the server's own page cap (see `MAX_PAGE_LIMIT`), one page at a time. */
const MY_TEAMS_LIMIT = 100
const MY_TEAMS_PAGE_LIMIT = 50

/** One section of the combobox's grouped `items` — see `Combobox.Group`'s doc:
 *  an array of `{ value, items }` objects, `value` doubling as the heading text. */
interface TeamGroup {
  value: string
  items: TeamListItem[]
}

export interface TeamPickerProps {
  /** The team currently linked to this side, or `null` for an ad-hoc side. */
  team: TeamListItem | null
  onChange: (team: TeamListItem | null) => void
  placeholder?: string
}

/**
 * Link a match side to a persistent Team, instead of (or as well as) tagging
 * players onto it directly. Built on shadcn's `Combobox` (Base UI) — always
 * searches both, in two sections:
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
  const myMatches = useMemo(
    () =>
      (myTeams.data ?? []).filter((t) =>
        termLower ? t.name.toLowerCase().includes(termLower) : true,
      ),
    [myTeams.data, termLower],
  )
  // "Other teams" only ever adds teams not already offered above, so a team
  // you're on doesn't get listed twice just because search finds it too.
  const otherMatches = useMemo(
    () => (searching ? (search.data ?? []).filter((t) => !myTeamIds.has(t.id)) : []),
    [searching, search.data, myTeamIds],
  )

  // Built fresh each render (cheap — at most MY_TEAMS_LIMIT + a search page)
  // so the combobox always reflects the latest filter; `filter={null}` below
  // tells it not to re-filter these itself.
  const groups: TeamGroup[] = []
  if (myMatches.length > 0) groups.push({ value: 'Your teams', items: myMatches })
  if (searching) groups.push({ value: 'Other teams', items: otherMatches })

  if (team) {
    return (
      <div className="mb-2 flex items-center gap-2 rounded-md bg-card px-2 py-1.5">
        <Avatar name={team.name} imageUrl={team.logo?.image_url} size="md" />
        <span className="flex-1 truncate text-sm font-medium">{team.name}</span>
        <button
          type="button"
          onClick={() => {
            onChange(null)
            // The combobox sets `term` to a stringified form of the picked
            // team on select (no `itemToStringValue` — the input's hidden
            // while a team's linked, so it's never been worth wiring one
            // up). Clear it here too, or the search box reappears pre-filled
            // with that stringified team instead of empty.
            setTerm('')
            setDebounced('')
          }}
          className="text-muted-foreground transition-colors hover:text-foreground"
          aria-label={`Unlink ${team.name}`}
        >
          <X className="size-4" />
        </button>
      </div>
    )
  }

  const isLoading = myTeams.isLoading || (searching && search.isLoading)
  const nothingFound = !isLoading && groups.every((g) => g.items.length === 0)

  return (
    <Combobox
      items={groups}
      filter={null}
      inputValue={term}
      onInputValueChange={setTerm}
      onValueChange={(next) => {
        if (!next) return
        onChange(next as TeamListItem)
        setTerm('')
        setDebounced('')
      }}
    >
      <ComboboxInput placeholder={placeholder} showTrigger={false} className="mb-2" />
      <ComboboxContent>
        {isLoading && (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {searching ? 'Searching…' : 'Loading your teams…'}
          </p>
        )}
        {nothingFound && (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {term.trim() ? 'No teams found.' : "You're not on any teams yet."}
          </p>
        )}
        <ComboboxList>
          {(group: TeamGroup) => (
            <ComboboxGroup key={group.value} items={group.items}>
              <ComboboxLabel>{group.value}</ComboboxLabel>
              <ComboboxCollection>
                {(t: TeamListItem) => (
                  <ComboboxItem key={t.id} value={t}>
                    <Avatar name={t.name} imageUrl={t.logo?.image_url} size="md" />
                    <span className="flex-1 truncate">{t.name}</span>
                  </ComboboxItem>
                )}
              </ComboboxCollection>
            </ComboboxGroup>
          )}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}
