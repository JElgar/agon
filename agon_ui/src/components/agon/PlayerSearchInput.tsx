import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Plus, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { Combobox, ComboboxContent, ComboboxInput, ComboboxItem, ComboboxList } from '@/components/ui/combobox'
import { InputGroupAddon } from '@/components/ui/input-group'
import type { TaggedPlayer } from './PlayerSideEditor'
import { taggedPlayerKey } from '@/lib/logMatch'

type UserProfile = components['schemas']['UserProfile']

/** One row offered by the combobox: a real Agon user (from `/users/search`)
 *  or the option to tag the typed name as a guest instead. */
type SearchItem = { kind: 'user'; user: UserProfile } | { kind: 'guest'; name: string }

const SEARCH_DEBOUNCE_MS = 300

export interface PlayerSearchInputProps {
  placeholder: string
  /** Everyone already tagged anywhere in the form, so nobody is offered twice. */
  taken: TaggedPlayer[]
  /** Account ids never to offer (e.g. the signed-in user). */
  excludeUserIds?: string[]
  onAdd: (player: TaggedPlayer) => void
  className?: string
  autoFocus?: boolean
}

/**
 * Search box that adds one player per pick: a registered Agon user found via
 * `/users/search` (debounced), or the typed name as a guest. Real matches are
 * listed first so Enter picks the top match, with "Add … as guest" last.
 */
export function PlayerSearchInput({
  placeholder,
  taken,
  excludeUserIds = [],
  onAdd,
  className,
  autoFocus,
}: PlayerSearchInputProps) {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')

  useEffect(() => {
    const t = setTimeout(() => setDebounced(term.trim()), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [term])

  const searching = debounced.length >= 2
  const search = useQuery({
    queryKey: ['users-search', debounced],
    enabled: searching,
    queryFn: async (): Promise<UserProfile[]> => {
      const { data, error } = await fetchClient.GET('/users/search', {
        params: { query: { q: debounced } },
      })
      if (error || !data) throw new Error('Search failed')
      return data
    },
  })

  const takenKeys = new Set(taken.map(taggedPlayerKey))
  const results = searching
    ? (search.data ?? []).filter(
        (u) => !excludeUserIds.includes(u.id) && !takenKeys.has(`user:${u.id}`),
      )
    : []
  const trimmed = term.trim()
  const canAddGuest = trimmed.length >= 1 && !takenKeys.has(`ext:${trimmed.toLowerCase()}`)
  const items: SearchItem[] = [
    ...results.map((u): SearchItem => ({ kind: 'user', user: u })),
    ...(canAddGuest ? [{ kind: 'guest', name: trimmed } as const] : []),
  ]
  const isLoading = searching && search.isLoading
  const nothingFound = !isLoading && items.length === 0 && trimmed.length > 0

  const reset = () => {
    setTerm('')
    setDebounced('')
  }

  return (
    <Combobox
      items={items}
      filter={null}
      autoHighlight
      inputValue={term}
      onInputValueChange={setTerm}
      // Keep the box blank after a pick: the picked player becomes a tagged
      // row, and without this the combobox fills the input with a
      // stringified `SearchItem`.
      itemToStringLabel={() => ''}
      onValueChange={(next) => {
        const item = next as SearchItem | null
        if (!item) return
        if (item.kind === 'user') {
          onAdd({
            kind: 'user',
            id: item.user.id,
            name: item.user.name,
            imageUrl: item.user.profile_image?.image_url,
          })
        } else {
          onAdd({ kind: 'external', id: crypto.randomUUID(), name: item.name })
        }
        reset()
      }}
    >
      <ComboboxInput
        placeholder={placeholder}
        showTrigger={false}
        autoFocus={autoFocus}
        className={className ?? 'h-11 rounded-2xl border-dashed bg-transparent shadow-none'}
      >
        <InputGroupAddon align="inline-start" className="text-muted-foreground">
          <Plus className="size-4" />
        </InputGroupAddon>
      </ComboboxInput>
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
  )
}
