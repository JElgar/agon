import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Pencil, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { Button } from '@/components/ui/button'
import { Combobox, ComboboxContent, ComboboxInput, ComboboxItem, ComboboxList } from '@/components/ui/combobox'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type UserProfile = components['schemas']['UserProfile']

/** A user picked as a side admin — just enough to render a chip, whether
 *  found via search (`UserProfile`) or already on the roster (`UserMember`). */
interface AdminUser {
  id: string
  name: string
  avatarUrl?: string
}

const SEARCH_DEBOUNCE_MS = 300

/**
 * Match-admin-only editor for one side's `side_admin_user_ids` —
 * `MatchPlayerRole::SideAdmin` grants, whole-value replaced on save via
 * `PUT /matches/:match_id/sides/:side_id/admins`. A side admin doesn't need
 * to be a player on the side, or on the roster at all, so this searches all
 * registered users rather than just this side's roster.
 *
 * Granting the role itself stays match-admin-only — a side admin can
 * configure and invite to their own side (see `MatchOpenJoinEditor`,
 * `InvitePlayers`'s `restrictedSideId`) but can't hand that out to anyone
 * else.
 */
export function SideAdminsEditor({ match, side }: { match: Match; side: MatchSide }) {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [admins, setAdmins] = useState<AdminUser[]>([])
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
  const results = searching
    ? (search.data ?? []).filter((u) => !admins.some((a) => a.id === u.id))
    : []

  const save = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.PUT('/matches/{match_id}/sides/{side_id}/admins', {
        params: { path: { match_id: match.id, side_id: side.id } },
        body: { user_ids: admins.map((a) => a.id) },
      })
      if (error) throw new Error('Failed to save side admins')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      setEditing(false)
    },
  })

  if (!editing) {
    return (
      <div className="flex items-center justify-between gap-2 py-1">
        <p className="text-xs text-muted-foreground">
          {side.side_admin_user_ids.length > 0
            ? `${side.side_admin_user_ids.length} side admin${side.side_admin_user_ids.length === 1 ? '' : 's'}`
            : 'No side admins'}
        </p>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 shrink-0 gap-1 px-2 text-xs text-muted-foreground"
          onClick={() => {
            setAdmins(
              side.side_admin_user_ids.map((id): AdminUser => {
                const player = match.players.find(
                  (p) => p.member.type === 'User' && p.member.user_id === id,
                )
                const member = player?.member.type === 'User' ? player.member : undefined
                return { id, name: member?.name ?? id, avatarUrl: member?.avatar_url }
              }),
            )
            setTerm('')
            setEditing(true)
          }}
        >
          <Pencil className="size-3" /> Edit
        </Button>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2 rounded-lg border p-2">
      <p className="text-xs font-medium">Side admins — {sideDisplayName(side) ?? 'this side'}</p>
      <p className="text-xs text-muted-foreground">
        Can configure ringer settings and invite people to this side only. Doesn't need to be a
        player on it.
      </p>
      {admins.length > 0 && (
        <ul className="flex flex-col gap-1">
          {admins.map((a) => (
            <li key={a.id} className="flex items-center gap-2 text-xs">
              <Avatar name={a.name} imageUrl={a.avatarUrl} size="sm" />
              <span className="flex-1 truncate">{a.name}</span>
              <button
                type="button"
                onClick={() => setAdmins((prev) => prev.filter((p) => p.id !== a.id))}
                className="text-muted-foreground hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
      <Combobox
        items={results}
        filter={null}
        autoHighlight
        inputValue={term}
        onInputValueChange={setTerm}
        itemToStringLabel={() => ''}
        onValueChange={(next) => {
          const user = next as UserProfile | null
          if (!user) return
          setAdmins((prev) => [
            ...prev,
            { id: user.id, name: user.name, avatarUrl: user.profile_image?.image_url },
          ])
          setTerm('')
        }}
      >
        <ComboboxInput
          placeholder="Search people to make side admin…"
          showTrigger={false}
          className="h-8 rounded-md border bg-background text-xs"
        />
        <ComboboxContent>
          {searching && search.isLoading && (
            <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>
          )}
          <ComboboxList>
            {(user: UserProfile) => (
              <ComboboxItem key={user.id} value={user}>
                <Avatar name={user.name} imageUrl={user.profile_image?.image_url} size="sm" />
                <span className="flex-1 truncate">{user.name}</span>
              </ComboboxItem>
            )}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
      {save.isError && <p className="text-xs text-destructive">{(save.error as Error).message}</p>}
      <div className="flex gap-2">
        <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? 'Saving…' : 'Save'}
        </Button>
        <Button size="sm" variant="outline" disabled={save.isPending} onClick={() => setEditing(false)}>
          Cancel
        </Button>
      </div>
    </div>
  )
}
