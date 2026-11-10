import { memo, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useLocation, useNavigate, useParams } from 'react-router-dom'
import {
  DndContext,
  DragOverlay,
  MeasuringStrategy,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  PointerSensor,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core'
import { ChevronLeft, GripVertical, Plus, Shuffle, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
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
import { AutoAssignSheet } from '@/components/agon/AutoAssignSheet'
import { TeamPicker } from '@/components/agon/TeamPicker'
import { CARD, ColourSwatches, FormSection } from '@/components/agon/logmatch/parts'
import { tournamentEntryAudienceLabel } from '@/lib/tournaments'
import { colourFromName, defaultSquadLabel, nameFromColour } from '@/lib/sideColours'

type Tournament = components['schemas']['Tournament']
type TournamentEntry = components['schemas']['TournamentEntry']
type TeamListItem = components['schemas']['TeamListItem']
type Team = components['schemas']['Team']
type UserProfile = components['schemas']['UserProfile']
type SquadLabel = components['schemas']['SquadLabel']

/** An entrant's display name + avatar image, regardless of whether the
 *  tournament's `entry_audience` makes `entrant_id` a user id or a team id. */
export interface EntrantDisplay {
  name: string
  imageUrl?: string
}

/** Looks up each entry's name/avatar (`GET /users/{id}` or `GET
 *  /teams/{id}`, whichever `entrant_id` actually is) so the entries list can
 *  show the same name+avatar treatment as a match's player list, instead of
 *  the raw id. One request per unique entrant, run in parallel. Also used by
 *  `TournamentPage`'s "Add match" picker, for the same reason. */
export function useEntrantDisplays(
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
  const location = useLocation()
  const queryClient = useQueryClient()
  // Set when `TournamentPage` redirected here because starting the
  // tournament was blocked on unassigned entrants — see its `startMutation`.
  const startBlockedReason = (location.state as { startBlockedReason?: string } | null)
    ?.startBlockedReason

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
      // The server has no default squad label, so the first entrant to land
      // in an unlabelled squad saves the default the header was already
      // showing — see `defaultSquadLabel`.
      if (!tournamentQuery.data?.squad_labels[String(squad)]) {
        const { error: labelError } = await fetchClient.PUT('/tournaments/{tournament_id}/squads/{squad}', {
          params: { path: { tournament_id: tournamentId!, squad } },
          body: defaultSquadLabel(squad),
        })
        if (labelError) throw new Error('Could not name that squad')
      }
    },
    onSuccess: invalidate,
  })

  const setSquadLabelMutation = useMutation({
    mutationFn: async ({ squad, name, colour }: { squad: number; name: string; colour: string }) => {
      const { error } = await fetchClient.PUT('/tournaments/{tournament_id}/squads/{squad}', {
        params: { path: { tournament_id: tournamentId!, squad } },
        body: { name, colour },
      })
      if (error) throw new Error('Could not update that squad')
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

      {startBlockedReason && (
        <p className="rounded-lg border border-amber-300 bg-amber-50 p-2.5 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200">
          Couldn't start the tournament yet: {startBlockedReason}
        </p>
      )}

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

          {tournament.entry_audience === 'players' ? (
            <SquadBoard
              tournament={tournament}
              entries={entries}
              entrantDisplays={entrantDisplays}
              squadLabels={tournament.squad_labels}
              onAssignSquad={(entrantId, squad) =>
                assignSquadMutation.mutate({ entrantId, squad })
              }
              onRemove={(entrantId) => removeEntryMutation.mutate(entrantId)}
              onSetSquadLabel={(squad, name, colour) =>
                setSquadLabelMutation.mutate({ squad, name, colour })
              }
            />
          ) : (
            <FormSection title={`Entries (${entries.length})`}>
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
                    onRemove={() => removeEntryMutation.mutate(entry.entrant_id)}
                  />
                ))}
              </div>
            </FormSection>
          )}
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
  onRemove,
}: {
  entry: TournamentEntry
  display: EntrantDisplay | undefined
  onRemove: () => void
}) {
  const name = display?.name ?? entry.entrant_id

  return (
    <div className="flex items-center gap-2.5 border-b py-2.5 last:border-b-0">
      <Avatar name={name} imageUrl={display?.imageUrl} size="md" />
      <span className="flex-1 truncate text-sm font-semibold">{name}</span>
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

/** Drop-zone id for entrants not yet put in a squad (`squad` unset). Distinct
 *  from any real squad column id (`"squad-<n>"`). */
const UNASSIGNED = '__unassigned__'

/** The board's columns never move or resize mid-drag, so one drop-zone
 *  measurement at drag start is enough — see `MatchRosterEditor`'s identical
 *  comment, which this mirrors. */
const measuring = { droppable: { strategy: MeasuringStrategy.BeforeDragging } }

/**
 * Drag-and-drop squad assignment for a teamless tournament, same interaction
 * as `MatchRosterEditor`'s side board: drag an entrant between squad columns
 * or out of "Unassigned", instead of typing a squad number into a box.
 * Squad moves apply immediately (one `POST .../squad` per drop, same as
 * before) — there's no batched Save because, unlike the match roster editor,
 * a squad move has no "undo by re-dragging before saving" case worth
 * building for; the server has no unassign endpoint, so dropping back onto
 * "Unassigned" is a no-op. "Add squad" just adds an empty local column —
 * squads only start existing on the server once an entrant lands in one.
 */
function SquadBoard({
  tournament,
  entries,
  entrantDisplays,
  squadLabels,
  onAssignSquad,
  onRemove,
  onSetSquadLabel,
}: {
  tournament: Tournament
  entries: TournamentEntry[]
  entrantDisplays: Map<string, EntrantDisplay>
  squadLabels: Record<string, SquadLabel>
  onAssignSquad: (entrantId: string, squad: number) => void
  onRemove: (entrantId: string) => void
  onSetSquadLabel: (squad: number, name: string, colour: string) => void
}) {
  const highestSquad = entries.reduce((max, e) => (e.squad != null ? Math.max(max, e.squad) : max), -1)
  const [extraColumns, setExtraColumns] = useState(0)
  const squadCount = Math.max(highestSquad + 1, 1) + extraColumns

  const [activeId, setActiveId] = useState<string | null>(null)

  const entriesById = useMemo(() => new Map(entries.map((e) => [e.entrant_id, e])), [entries])

  const columns = useMemo(() => {
    const cols = new Map<string, TournamentEntry[]>()
    for (let i = 0; i < squadCount; i++) cols.set(`squad-${i}`, [])
    cols.set(UNASSIGNED, [])
    for (const entry of entries) {
      const key = entry.squad != null && cols.has(`squad-${entry.squad}`) ? `squad-${entry.squad}` : UNASSIGNED
      cols.get(key)!.push(entry)
    }
    return cols
  }, [entries, squadCount])

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 8 } }))

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveId(null)
    const { active, over } = event
    if (!over) return
    const targetId = String(over.id)
    if (targetId === UNASSIGNED) return // no unassign endpoint — drop is a no-op
    const squad = Number(targetId.replace('squad-', ''))
    const entrantId = String(active.id)
    const entry = entriesById.get(entrantId)
    if (entry?.squad === squad) return
    onAssignSquad(entrantId, squad)
  }

  const activeEntry = activeId ? entriesById.get(activeId) : undefined

  const [autoAssignOpen, setAutoAssignOpen] = useState(false)
  const accepted = entries.filter((e) => e.status === 'accepted')
  const canAutoAssign = tournament.status === 'open' && accepted.length >= 2

  return (
    <FormSection title="Squads">
      {canAutoAssign && (
        <>
          <Button className="h-11" onClick={() => setAutoAssignOpen(true)}>
            <Shuffle className="size-4" />
            Auto-assign
          </Button>
          <AutoAssignSheet
            open={autoAssignOpen}
            onOpenChange={setAutoAssignOpen}
            displays={entrantDisplays}
            target={{
              kind: 'tournament',
              tournamentId: tournament.id,
              poolSize: accepted.length,
              unassignedCount: accepted.filter((e) => e.squad == null).length,
              squadSize: tournament.squad_size ?? 2,
              squadLabels,
              subtitle: tournament.name,
            }}
          />
        </>
      )}
      <p className="px-1 text-xs text-muted-foreground">
        Every player needs a squad before the tournament can start — squad-mates play as one
        team. Drag a player into a squad, or add another squad if you need more.
      </p>
      <DndContext
        sensors={sensors}
        measuring={measuring}
        onDragStart={(event: DragStartEvent) => setActiveId(String(event.active.id))}
        onDragEnd={handleDragEnd}
      >
        <div className="flex flex-col gap-3">
          {Array.from({ length: squadCount }, (_, i) => (
            <SquadColumn
              key={i}
              id={`squad-${i}`}
              header={
                <SquadHeader
                  index={i}
                  label={squadLabels[String(i)]}
                  onSave={(name, colour) => onSetSquadLabel(i, name, colour)}
                />
              }
              entries={columns.get(`squad-${i}`) ?? []}
              entrantDisplays={entrantDisplays}
              onRemove={onRemove}
            />
          ))}
          <SquadColumn
            id={UNASSIGNED}
            header={
              <p className="mb-2 truncate text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                Unassigned
              </p>
            }
            entries={columns.get(UNASSIGNED) ?? []}
            entrantDisplays={entrantDisplays}
            onRemove={onRemove}
          />
        </div>
        <DragOverlay>
          {activeEntry && (
            <EntrantChip
              entry={activeEntry}
              display={entrantDisplays.get(activeEntry.entrant_id)}
              overlay
            />
          )}
        </DragOverlay>
      </DndContext>
      <Button size="sm" variant="outline" onClick={() => setExtraColumns((n) => n + 1)}>
        <Plus className="size-4" />
        Add squad
      </Button>
    </FormSection>
  )
}

function SquadColumn({
  id,
  header,
  entries,
  entrantDisplays,
  onRemove,
}: {
  id: string
  header: ReactNode
  entries: TournamentEntry[]
  entrantDisplays: Map<string, EntrantDisplay>
  onRemove: (entrantId: string) => void
}) {
  const { setNodeRef, isOver } = useDroppable({ id })

  return (
    <div
      ref={setNodeRef}
      className={cn(
        'rounded-lg border bg-muted/40 p-2.5 transition-colors',
        isOver && 'border-primary bg-primary/5',
      )}
    >
      {header}
      <div className="flex min-h-11 flex-col gap-1.5">
        {entries.length === 0 && (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            {id === UNASSIGNED ? 'Everyone has a squad.' : 'Drag a player here.'}
          </p>
        )}
        {entries.map((entry) => (
          <DraggableEntrant
            key={entry.entrant_id}
            entry={entry}
            display={entrantDisplays.get(entry.entrant_id)}
            onRemove={onRemove}
          />
        ))}
      </div>
    </div>
  )
}

/** A squad column's editable name + colour, same auto-linking as a match
 *  side's (`TeamSideCard`'s `ColourSwatches` usage): typing a colour word
 *  into the name picks that colour, picking a colour names the squad after
 *  it, and each field stops auto-filling once the user has touched it
 *  directly. Saves on name blur (if changed) and immediately on a colour
 *  pick — there's no separate "Save" step on this page, every other edit
 *  here applies right away too. */
function SquadHeader({
  index,
  label,
  onSave,
}: {
  index: number
  label: SquadLabel | undefined
  onSave: (name: string, colour: string) => void
}) {
  const fallback = defaultSquadLabel(index)
  const [name, setName] = useState(label?.name ?? fallback.name)
  const [colour, setColour] = useState(label?.colour ?? fallback.colour)
  const [nameTouched, setNameTouched] = useState(false)
  const [colourTouched, setColourTouched] = useState(false)

  // Pick up a save made elsewhere (another tab, or this one after a
  // mutation's invalidate refetches the tournament) without clobbering
  // whatever the user is mid-typing.
  useEffect(() => {
    if (label && !nameTouched) setName(label.name)
    if (label && !colourTouched) setColour(label.colour)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [label?.name, label?.colour])

  const handleNameChange = (v: string) => {
    setName(v)
    setNameTouched(true)
    if (!colourTouched) {
      const inferred = colourFromName(v)
      if (inferred) setColour(inferred)
    }
  }
  const handleColourChange = (hex: string) => {
    setColour(hex)
    setColourTouched(true)
    if (!nameTouched) setName(nameFromColour(hex) ?? name)
    onSave(nameTouched ? name : (nameFromColour(hex) ?? name), hex)
  }

  return (
    <div className="mb-2 flex flex-col gap-1.5">
      <Input
        className="h-8 text-sm font-semibold"
        value={name}
        onChange={(e) => handleNameChange(e.target.value)}
        onBlur={() => {
          if (name.trim()) onSave(name.trim(), colour)
        }}
        aria-label={`Squad ${index + 1} name`}
      />
      <ColourSwatches label={`Squad ${index + 1} colour`} value={colour} onChange={handleColourChange} />
    </div>
  )
}

function DraggableEntrant({
  entry,
  display,
  onRemove,
}: {
  entry: TournamentEntry
  display: EntrantDisplay | undefined
  onRemove: (entrantId: string) => void
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: entry.entrant_id })

  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn('touch-none', isDragging && 'opacity-30')}
    >
      <EntrantChip entry={entry} display={display} onRemove={onRemove} />
    </div>
  )
}

/** Memoized for the same reason as `MatchRosterEditor`'s `PlayerChip`: every
 *  draggable/droppable consumer re-renders on each drag-move frame. */
const EntrantChip = memo(function EntrantChip({
  entry,
  display,
  onRemove,
  overlay = false,
}: {
  entry: TournamentEntry
  display: EntrantDisplay | undefined
  onRemove?: (entrantId: string) => void
  overlay?: boolean
}) {
  const name = display?.name ?? entry.entrant_id
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md bg-card px-2 py-1.5',
        overlay && 'border shadow-lg',
      )}
    >
      <GripVertical className="size-3.5 shrink-0 text-muted-foreground" />
      <Avatar name={name} imageUrl={display?.imageUrl} size="md" />
      <span className="flex-1 truncate text-sm">{name}</span>
      {onRemove && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            onRemove(entry.entrant_id)
          }}
          onPointerDown={(e) => e.stopPropagation()}
          aria-label={`Remove ${name}`}
          className="text-muted-foreground transition-colors hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      )}
    </div>
  )
})

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
