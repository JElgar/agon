import { useEffect, useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Search, UserPlus, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Avatar } from './Avatar'

type UserProfile = components['schemas']['UserProfile']

/** A person tagged onto a side: either a registered Agon user or a typed-in guest. */
export type TaggedPlayer =
  | { kind: 'user'; id: string; name: string; imageUrl?: string }
  | { kind: 'external'; name: string }

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
  /** When set, shown as a non-removable "you" chip above the tagged players. */
  youName?: string
  /** Ids already tagged on the *other* side, so we don't offer them twice. */
  excludeUserIds?: string[]
}

/** How long to wait after typing stops before hitting `/users/search`. */
const SEARCH_DEBOUNCE_MS = 300

/**
 * One side of a match: the "you" chip (optional), the tagged players, and a
 * search box to add either a real Agon user (from `/users/search`) or an
 * external guest by name. Purely controlled — the parent owns the player list.
 */
export function PlayerSideEditor({
  title,
  searchPlaceholder,
  players,
  onChange,
  youName,
  excludeUserIds = [],
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

  const results = (search.data ?? []).filter(
    (u) => !excludeUserIds.includes(u.id) && !taggedKeys.has(`user:${u.id}`),
  )

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
    onChange([...players, { kind: 'external', name: trimmed }])
    setTerm('')
    setDebounced('')
  }

  const removeAt = (index: number) => {
    onChange(players.filter((_, i) => i !== index))
  }

  const trimmed = term.trim()
  const showDropdown = trimmed.length >= 2
  const canAddGuest =
    trimmed.length >= 1 && !taggedKeys.has(`ext:${trimmed.toLowerCase()}`)

  return (
    <div className="rounded-lg border bg-muted/40 p-3">
      <p className="mb-2 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </p>

      <div className="flex flex-col gap-1.5">
        {youName && (
          <div className="flex items-center gap-2 rounded-md bg-card px-2 py-1.5">
            <Avatar name={youName} size="md" ring="you" />
            <span className="flex-1 truncate text-sm">{youName}</span>
            <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
              you
            </span>
          </div>
        )}

        {players.map((p, i) => (
          <div
            key={taggedPlayerKey(p)}
            className="flex items-center gap-2 rounded-md bg-card px-2 py-1.5"
          >
            {p.kind === 'user' ? (
              <Avatar name={p.name} imageUrl={p.imageUrl} size="md" />
            ) : (
              <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full border border-dashed border-muted-foreground/50 bg-muted text-[10px] font-medium text-muted-foreground">
                {p.name.slice(0, 2).toUpperCase()}
              </span>
            )}
            <span className="flex-1 truncate text-sm">{p.name}</span>
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
        ))}
      </div>

      {/* Search / add */}
      <div className="relative mt-2">
        <div className="flex items-center gap-2 rounded-md border bg-card px-2.5 py-1.5">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            type="text"
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            placeholder={searchPlaceholder}
            className="w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            onKeyDown={(e) => {
              if (e.key === 'Enter' && canAddGuest) {
                e.preventDefault()
                addExternal(term)
              }
            }}
          />
        </div>

        {showDropdown && (
          <div className="absolute z-10 mt-1 w-full overflow-hidden rounded-md border bg-card shadow-md">
            {search.isLoading && (
              <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>
            )}
            {!search.isLoading &&
              results.map((u) => (
                <button
                  key={u.id}
                  type="button"
                  onClick={() => addUser(u)}
                  className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-muted"
                >
                  <Avatar name={u.name} imageUrl={u.profile_image?.image_url} size="md" />
                  <span className="flex-1 truncate text-sm">{u.name}</span>
                </button>
              ))}
            {canAddGuest && (
              <button
                type="button"
                onClick={() => addExternal(term)}
                className={cn(
                  'flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-muted',
                  results.length > 0 && 'border-t',
                )}
              >
                <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
                  <UserPlus className="size-3.5" />
                </span>
                <span className="flex-1 truncate text-sm">
                  Add "<span className="font-medium">{trimmed}</span>" as guest
                </span>
              </button>
            )}
            {!search.isLoading && results.length === 0 && !canAddGuest && (
              <p className="px-3 py-2 text-xs text-muted-foreground">No matches.</p>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
