import { useEffect, useState } from 'react'
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Combobox,
  ComboboxContent,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox'
import { Avatar } from '@/components/agon/Avatar'
import { TeamPicker } from '@/components/agon/TeamPicker'
import { CARD, FormSection } from '@/components/agon/logmatch/parts'
import { tournamentEntryAudienceLabel } from '@/lib/tournaments'

type Tournament = components['schemas']['Tournament']
type TournamentEntry = components['schemas']['TournamentEntry']
type TeamListItem = components['schemas']['TeamListItem']
type Team = components['schemas']['Team']
type UserProfile = components['schemas']['UserProfile']

/** An entrant's display name + avatar image, regardless of whether the
 *  tournament's `entry_audience` makes `entrant_id` a user id or a team id. */
interface EntrantDisplay {
  name: string
  imageUrl?: string
}

/** Looks up each entry's name/avatar (`GET /users/{id}` or `GET
 *  /teams/{id}`, whichever `entrant_id` actually is) so the entries list can
 *  show the same name+avatar treatment as a match's player list, instead of
 *  the raw id. One request per unique entrant, run in parallel. */
function useEntrantDisplays(
  entrantIds: string[],
  audience: Tournament['entry_audience'] | undefined,
): Map<string, EntrantDisplay> {
  const unique = [...new Set(entrantIds)]
  const results = useQueries({
    queries: unique.map((id) => ({
      queryKey: ['entrant-display', audience, id],
      enabled: audience != null,
      queryFn: async (): Promise<EntrantDisplay> => {
        if (audience === 'teams') {
          const { data, error } = await fetchClient.GET('/teams/{team_id}', {
            params: { path: { team_id: id } },
          })
          if (error || !data) throw new Error('Failed to load team')
          const team = data as Team
          return { name: team.name, imageUrl: team.logo?.image_url }
        }
        const { data, error } = await fetchClient.GET('/users/{user_id}', {
          params: { path: { user_id: id } },
        })
        if (error || !data) throw new Error('Failed to load user')
        const user = data as UserProfile
        return { name: user.name, imageUrl: user.profile_image?.image_url }
      },
    })),
  })

  const map = new Map<string, EntrantDisplay>()
  unique.forEach((id, i) => {
    const data = results[i]?.data
    if (data) map.set(id, data)
  })
  return map
}

const SEARCH_DEBOUNCE_MS = 300

/**
 * Manage a tournament's entries (`GET`/`POST`/`DELETE
 * /tournaments/{id}/entries`) — a team picker when `entry_audience` is
 * `teams`, a user search when it's `players`, plus squad assignment for a
 * teamless tournament (`POST /tournaments/{id}/entries/{entrant_id}/squad`
 * — mirrors assigning an unassigned player to a side in an ordinary match).
 */
export function TournamentEntriesPage() {
  const { tournamentId } = useParams<{ tournamentId: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const tournamentQuery = useQuery({
    queryKey: ['tournament', tournamentId],
    enabled: !!tournamentId,
    queryFn: async (): Promise<Tournament> => {
      const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data) throw new Error('Failed to load this tournament')
      return data
    },
  })

  const entriesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'entries'],
    enabled: !!tournamentId,
    queryFn: async (): Promise<TournamentEntry[]> => {
      const items: TournamentEntry[] = []
      let cursor: string | undefined
      do {
        const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}/entries', {
          params: { path: { tournament_id: tournamentId! }, query: { cursor, limit: 50 } },
        })
        if (error || !data) throw new Error('Failed to load entries')
        items.push(...data.items)
        cursor = data.next_cursor
      } while (cursor)
      return items
    },
  })

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ['tournament', tournamentId] })

  const addEntryMutation = useMutation({
    mutationFn: async ({ entrant_id, seed }: { entrant_id: string; seed?: number }) => {
      const { error } = await fetchClient.POST('/tournaments/{tournament_id}/entries', {
        params: { path: { tournament_id: tournamentId! } },
        body: { entrant_id, seed },
      })
      if (error) throw new Error('Could not add that entry')
    },
    onSuccess: invalidate,
  })

  const removeEntryMutation = useMutation({
    mutationFn: async (entrantId: string) => {
      const { error } = await fetchClient.DELETE(
        '/tournaments/{tournament_id}/entries/{entrant_id}',
        { params: { path: { tournament_id: tournamentId!, entrant_id: entrantId } } },
      )
      if (error) throw new Error('Could not remove that entry')
    },
    onSuccess: invalidate,
  })

  const assignSquadMutation = useMutation({
    mutationFn: async ({ entrantId, squad }: { entrantId: string; squad: number }) => {
      const { error } = await fetchClient.POST(
        '/tournaments/{tournament_id}/entries/{entrant_id}/squad',
        {
          params: { path: { tournament_id: tournamentId!, entrant_id: entrantId } },
          body: { squad },
        },
      )
      if (error) throw new Error('Could not assign that squad')
    },
    onSuccess: invalidate,
  })

  const tournament = tournamentQuery.data
  const entries = entriesQuery.data ?? []
  const entrantDisplays = useEntrantDisplays(
    entries.map((e) => e.entrant_id),
    tournament?.entry_audience,
  )

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          className="-ml-2"
          onClick={() => (tournamentId ? navigate(`/tournaments/${tournamentId}`) : navigate(-1))}
          aria-label="Back"
        >
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-2xl">Entries</h1>
      </div>

      {tournament && (
        <>
          <FormSection
            title={`Add ${tournamentEntryAudienceLabel(tournament.entry_audience).toLowerCase()}`}
          >
            {tournament.entry_audience === 'teams' ? (
              <AddTeamEntryRow
                manualSeeding={tournament.seeding === 'manual'}
                onAdd={(entrant_id, seed) => addEntryMutation.mutate({ entrant_id, seed })}
              />
            ) : (
              <UserSearchRow
                taken={entries.map((e) => e.entrant_id)}
                onAdd={(user) => addEntryMutation.mutate({ entrant_id: user.id })}
              />
            )}
            {addEntryMutation.isError && (
              <p className="text-sm text-destructive">{addEntryMutation.error.message}</p>
            )}
          </FormSection>

          <FormSection title={`Entries (${entries.length})`}>
            {tournament.entry_audience === 'players' && (
              <p className="px-1 text-xs text-muted-foreground">
                Players need a squad number before the tournament can start — type one into
                the box next to a player and tap Assign. Squad-mates play as one team, so give
                everyone on the same squad the same number.
              </p>
            )}
            <div className={CARD}>
              {entries.length === 0 && (
                <p className="py-2 text-center text-sm text-muted-foreground">
                  No entries yet.
                </p>
              )}
              {entries.map((entry) => (
                <EntryRow
                  key={entry.entrant_id}
                  entry={entry}
                  display={entrantDisplays.get(entry.entrant_id)}
                  teamless={tournament.entry_audience === 'players'}
                  onRemove={() => removeEntryMutation.mutate(entry.entrant_id)}
                  onAssignSquad={(squad) =>
                    assignSquadMutation.mutate({ entrantId: entry.entrant_id, squad })
                  }
                />
              ))}
            </div>
          </FormSection>
        </>
      )}

      {tournamentQuery.isLoading && (
        <div className="h-28 animate-pulse rounded-2xl border bg-card" aria-hidden />
      )}
    </div>
  )
}

/** Team picker plus, when the tournament uses manual seeding, the seed
 *  number it requires (`add_tournament_entry` 400s without one). */
function AddTeamEntryRow({
  manualSeeding,
  onAdd,
}: {
  manualSeeding: boolean
  onAdd: (entrantId: string, seed?: number) => void
}) {
  const [team, setTeam] = useState<TeamListItem | null>(null)
  const [seed, setSeed] = useState('')

  const canAdd = !!team && (!manualSeeding || seed.trim() !== '')

  return (
    <div className="flex items-center gap-2">
      <div className="flex-1">
        <TeamPicker team={team} onChange={setTeam} placeholder="Add a team…" />
      </div>
      {manualSeeding && (
        <Input
          className="h-10 w-16 text-center"
          type="number"
          min={1}
          placeholder="Seed"
          value={seed}
          onChange={(e) => setSeed(e.target.value)}
        />
      )}
      <Button
        size="sm"
        disabled={!canAdd}
        onClick={() => {
          if (!team) return
          onAdd(team.id, manualSeeding ? Number(seed) : undefined)
          setTeam(null)
          setSeed('')
        }}
      >
        Add
      </Button>
    </div>
  )
}

function EntryRow({
  entry,
  display,
  teamless,
  onRemove,
  onAssignSquad,
}: {
  entry: TournamentEntry
  display: EntrantDisplay | undefined
  teamless: boolean
  onRemove: () => void
  onAssignSquad: (squad: number) => void
}) {
  const [squadInput, setSquadInput] = useState(
    entry.squad != null ? String(entry.squad + 1) : '',
  )
  const name = display?.name ?? entry.entrant_id

  return (
    <div className="flex items-center gap-2.5 border-b py-2.5 last:border-b-0">
      <Avatar name={name} imageUrl={display?.imageUrl} size="md" />
      <span className="flex-1 truncate text-sm font-semibold">{name}</span>
      {teamless && (
        <div className="flex items-center gap-1.5">
          {entry.squad == null ? (
            <span className="rounded-full bg-amber-100 px-2 py-0.5 text-xs font-bold text-amber-800 dark:bg-amber-900 dark:text-amber-200">
              Unassigned
            </span>
          ) : (
            <span className="rounded-full bg-accent px-2 py-0.5 text-xs font-bold text-accent-foreground">
              Squad {entry.squad + 1}
            </span>
          )}
          <Input
            className="h-8 w-16 text-center"
            type="number"
            min={1}
            placeholder="Squad #"
            aria-label="Squad number"
            value={squadInput}
            onChange={(e) => setSquadInput(e.target.value)}
          />
          <Button
            size="sm"
            variant="outline"
            disabled={!squadInput.trim()}
            onClick={() => onAssignSquad(Number(squadInput) - 1)}
          >
            Assign
          </Button>
        </div>
      )}
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${name}`}
        className="flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:text-foreground"
      >
        <X className="size-4" />
      </button>
    </div>
  )
}

/** Debounced `/users/search`, adding one real Agon account (no guest option
 *  — a tournament entrant must be a real user id). */
function UserSearchRow({
  taken,
  onAdd,
}: {
  taken: string[]
  onAdd: (user: UserProfile) => void
}) {
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

  const results = (search.data ?? []).filter((u) => !taken.includes(u.id))

  return (
    <Combobox
      items={results}
      filter={null}
      inputValue={term}
      onInputValueChange={setTerm}
      // Keep the box blank after a pick: without this the combobox fills the
      // input with a stringified `UserProfile` (see PlayerSearchInput.tsx's
      // same fix).
      itemToStringLabel={() => ''}
      onValueChange={(user) => {
        if (!user) return
        onAdd(user as UserProfile)
        setTerm('')
        setDebounced('')
      }}
    >
      <ComboboxInput placeholder="Add a player…" showTrigger={false} />
      <ComboboxContent>
        {searching && search.isLoading && (
          <p className="px-3 py-2 text-xs text-muted-foreground">Searching…</p>
        )}
        {searching && !search.isLoading && results.length === 0 && (
          <p className="px-3 py-2 text-xs text-muted-foreground">No one found.</p>
        )}
        <ComboboxList>
          {(user: UserProfile) => (
            <ComboboxItem key={user.id} value={user}>
              <Avatar name={user.name} imageUrl={user.profile_image?.image_url} size="md" />
              <span className="flex-1 truncate">{user.name}</span>
            </ComboboxItem>
          )}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}
