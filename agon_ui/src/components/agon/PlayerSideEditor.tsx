import { useEffect, useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { UserPlus, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { TeamPicker } from './TeamPicker'
import { Combobox, ComboboxContent, ComboboxInput, ComboboxItem, ComboboxList } from '@/components/ui/combobox'

type UserProfile = components['schemas']['UserProfile']
type TeamListItem = components['schemas']['TeamListItem']

/** One row offered by the add-a-player combobox: a real Agon user (from
 *  `/users/search`) or the option to tag the typed name as a guest instead. */
type SearchItem = { kind: 'user'; user: UserProfile } | { kind: 'guest'; name: string }

/** A person tagged onto a side: either a registered Agon user or a typed-in guest.
 *  Both carry a stable `id` — the user's own account id for a registered user
 *  (already known before the match exists), or a freshly generated client-side
 *  token for a guest — so a create-time score's goal/card/batting detail can
 *  reference a specific player before the match (and its real player ids)
 *  exists; the server re-points these to real ids the same way it already
 *  does for side client ids (see `CreateMatchExternalInviteInput`). */
export type TaggedPlayer =
  | { kind: 'user'; id: string; name: string; imageUrl?: string }
  | { kind: 'external'; id: string; name: string }

/** A stable key for a tagged player, for React keys and de-duping. */
function taggedPlayerKey(p: TaggedPlayer): string {
  return p.kind === 'user' ? `user:${p.id}` : `ext:${p.name.toLowerCase()}`
}

export interface PlayerSideEditorProps {
  /** Section label, e.g. "Your side" / "Opposition". */
  title: string
  /** Placeholder for the search box, e.g. "Add a teammate…". */
  searchPlaceholder: string
  players: TaggedPlayer[]
  onChange: (players: TaggedPlayer[]) => void
  /** The signed-in user's id. A tagged player with this id is badged "you"; the
   *  user is also excluded from search so they can't be added twice. */
  currentUserId?: string
  /** Ids already tagged on the *other* side, so we don't offer them twice. */
  excludeUserIds?: string[]
  /** Optional custom name for this side (e.g. "The Wanderers"). Omit both
   *  props to hide the field entirely. Rendered only when `nameFieldVisible`
   *  isn't explicitly false — the parent hides it once this side is linked
   *  to a team the other side doesn't share (the server rejects a name
   *  alongside a team unless another side shares that team). */
  name?: string
  onNameChange?: (name: string) => void
  /** Whether the name field should render at all. Defaults to true; pass
   *  false once a `team` is linked here and no other side shares it. */
  nameFieldVisible?: boolean
  /** The team (if any) this side is linked to, replacing manually-tagged
   *  players/a typed name as its identity. */
  team?: TeamListItem | null
  onTeamChange?: (team: TeamListItem | null) => void
}

/** How long to wait after typing stops before hitting `/users/search`. */
const SEARCH_DEBOUNCE_MS = 300

/**
 * One side of a match: the tagged players (the signed-in user, if on this side,
 * is badged "you" but is a normal removable entry) and a search box to add
 * either a real Agon user (from `/users/search`) or an external guest by name.
 * Purely controlled — the parent owns the player list.
 */
export function PlayerSideEditor({
  title,
  searchPlaceholder,
  players,
  onChange,
  currentUserId,
  excludeUserIds = [],
  name,
  onNameChange,
  nameFieldVisible = true,
  team = null,
  onTeamChange,
}: PlayerSideEditorProps) {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const t = setTimeout(() => setDebounced(term.trim()), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [term])

  const search = useQuery({
    queryKey: ['users-search', debounced],
    enabled: debounced.length >= 2,
    queryFn: async (): Promise<UserProfile[]> => {
      const { data, error } = await fetchClient.GET('/users/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Search failed')
      return data
    },
  })

  const taggedKeys = useMemo(
    () => new Set(players.map(taggedPlayerKey)),
    [players],
  )

  const searching = debounced.length >= 2
  const results = searching
    ? (search.data ?? []).filter(
        (u) =>
          u.id !== currentUserId &&
          !excludeUserIds.includes(u.id) &&
          !taggedKeys.has(`user:${u.id}`),
      )
    : []

  const addUser = (u: UserProfile) => {
    onChange([
      ...players,
      { kind: 'user', id: u.id, name: u.name, imageUrl: u.profile_image?.image_url },
    ])
    setTerm('')
    setDebounced('')
  }

  const addExternal = (name: string) => {
    const trimmed = name.trim()
    if (!trimmed) return
    const key = `ext:${trimmed.toLowerCase()}`
    if (taggedKeys.has(key)) return
    onChange([...players, { kind: 'external', id: crypto.randomUUID(), name: trimmed }])
    setTerm('')
    setDebounced('')
  }

  const removeAt = (index: number) => {
    onChange(players.filter((_, i) => i !== index))
  }

  const trimmed = term.trim()
  const canAddGuest =
    trimmed.length >= 1 && !taggedKeys.has(`ext:${trimmed.toLowerCase()}`)

  // Real matches first, the "add as guest" fallback last — standard
  // create-new-item-last combobox convention. Enter/autoHighlight then
  // defaults to the top real match when there is one, guest otherwise;
  // arrow keys reach any other entry. (Previously — before real keyboard
  // nav existed here — Enter always added a guest, ignoring matches;
  // selecting a real match required a click either way.)
  const items: SearchItem[] = [
    ...(searching ? results.map((u): SearchItem => ({ kind: 'user', user: u })) : []),
    ...(canAddGuest ? [{ kind: 'guest', name: trimmed } as const] : []),
  ]
  const isLoading = searching && search.isLoading
  const nothingFound = !isLoading && items.length === 0 && trimmed.length > 0

  return (
    <div className="rounded-lg border bg-muted/40 p-3">
      <p className="mb-2 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </p>

      {onTeamChange && <TeamPicker team={team} onChange={onTeamChange} />}

      {onNameChange && nameFieldVisible && (
        <input
          type="text"
          value={name ?? ''}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder="Name this side (optional)"
          maxLength={60}
          className="mb-2 w-full rounded-md border bg-card px-2.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground"
        />
      )}
      {onNameChange && !nameFieldVisible && team && (
        <p className="mb-2 text-xs text-muted-foreground">
          Linked to {team.name} — link the other side to the same team to give
          each a custom name.
        </p>
      )}

      <div className="flex flex-col gap-1.5">
        {players.length === 0 && (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            No players yet.
          </p>
        )}

        {players.map((p, i) => {
          const isYou = p.kind === 'user' && p.id === currentUserId
          return (
            <div
              key={taggedPlayerKey(p)}
              className="flex items-center gap-2 rounded-md bg-card px-2 py-1.5"
            >
              {p.kind === 'user' ? (
                <Avatar
                  name={p.name}
                  imageUrl={p.imageUrl}
                  size="md"
                  ring={isYou ? 'you' : 'none'}
                />
              ) : (
                <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-dashed border-muted-foreground/50 bg-muted text-[10px] font-medium text-muted-foreground">
                  {p.name.slice(0, 2).toUpperCase()}
                </span>
              )}
              {/* data-testid: a tagged player's name has no other stable
                  accessible hook — their "Remove" button is labelled by it,
                  but that's an action, not the name itself. See
                  agon_ui/e2e/README.md's locator guidance. */}
              <span className="flex-1 truncate text-sm" data-testid="tagged-player-name">{p.name}</span>
              {isYou && (
                <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                  you
                </span>
              )}
              {p.kind === 'external' && (
                <span className="text-[10px] text-muted-foreground">Not on Agon</span>
              )}
              <button
                type="button"
                onClick={() => removeAt(i)}
                className="text-muted-foreground transition-colors hover:text-foreground"
                aria-label={`Remove ${p.name}`}
              >
                <X className="size-4" />
              </button>
            </div>
          )
        })}
      </div>

      {/* Search / add */}
      <Combobox
        items={items}
        filter={null}
        autoHighlight
        inputValue={term}
        onInputValueChange={setTerm}
        onValueChange={(next) => {
          const item = next as SearchItem | null
          if (!item) return
          if (item.kind === 'user') addUser(item.user)
          else addExternal(item.name)
        }}
      >
        <ComboboxInput placeholder={searchPlaceholder} showTrigger={false} className="mt-2" />
        <ComboboxContent>
          {isLoading && <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>}
          {nothingFound && <p className="px-3 py-2 text-xs text-muted-foreground">No matches.</p>}
          <ComboboxList>
            {(item: SearchItem) =>
              item.kind === 'user' ? (
                <ComboboxItem key={item.user.id} value={item}>
                  <Avatar name={item.user.name} imageUrl={item.user.profile_image?.image_url} size="md" />
                  <span className="flex-1 truncate">{item.user.name}</span>
                </ComboboxItem>
              ) : (
                <ComboboxItem key="guest" value={item}>
                  <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
                    <UserPlus className="size-3.5" />
                  </span>
                  <span className="flex-1 truncate">
                    Add "<span className="font-medium">{item.name}</span>" as guest
                  </span>
                </ComboboxItem>
              )
            }
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
    </div>
  )
}
