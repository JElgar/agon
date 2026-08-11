import { useMemo, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core'
import { GripVertical, X } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Avatar } from './Avatar'
import { memberAvatarUrl, memberName, playerId } from '@/lib/members'

/** "Alice", "Alice and Bob", or "Alice, Bob and Charlie" — for the removal
 *  confirmation's "you're removing …" sentence. */
function joinNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? ''
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']

/** Drop-zone id for players with no side yet (`side_id` unset). Distinct from
 *  any real side id, which the server mints as an opaque generated string. */
const UNASSIGNED = '__unassigned__'

/**
 * Pre-game (or any time before it's locked in) roster editor: drag players
 * between sides — or into "Unassigned" — and remove anyone who shouldn't be
 * on the roster at all. Purely local state until "Save", which diffs against
 * the match's current roster and sends only what changed — `side_assignments`
 * for anyone whose side moved, `removed_player_ids` for anyone dropped — in
 * one `PATCH`. Saving with any pending removals detours through a confirm
 * dialog naming who's being cut, since that part can't be undone once it's
 * sent; a save with only side moves goes straight through. Shown in place of
 * the read-only roster grid when a participant taps "Edit".
 */
export function MatchRosterEditor({
  match,
  onDone,
}: {
  match: Match
  onDone: () => void
}) {
  const queryClient = useQueryClient()

  // player_id -> side_id (or undefined for unassigned). Seeded from the
  // match's current roster; only local edits (drag / remove) touch it from
  // here on.
  const [assignments, setAssignments] = useState<Map<string, string | undefined>>(
    () => new Map(match.players.map((p) => [playerId(p), p.side_id])),
  )
  const [removedIds, setRemovedIds] = useState<Set<string>>(new Set())
  const [activeId, setActiveId] = useState<string | null>(null)
  const [confirmOpen, setConfirmOpen] = useState(false)

  const playersById = useMemo(
    () => new Map(match.players.map((p) => [playerId(p), p])),
    [match.players],
  )

  const columns = useMemo(() => {
    const cols = new Map<string, MatchPlayer[]>()
    for (const side of match.sides) cols.set(side.id, [])
    cols.set(UNASSIGNED, [])
    for (const [id, sideId] of assignments) {
      if (removedIds.has(id)) continue
      const player = playersById.get(id)
      if (!player) continue
      const key = sideId && cols.has(sideId) ? sideId : UNASSIGNED
      cols.get(key)!.push(player)
    }
    return cols
  }, [assignments, removedIds, playersById, match.sides])

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  )

  const handleDragStart = (event: DragStartEvent) => {
    setActiveId(String(event.active.id))
  }

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveId(null)
    const { active, over } = event
    if (!over) return
    const id = String(active.id)
    const targetSideId = String(over.id)
    setAssignments((prev) => {
      const next = new Map(prev)
      next.set(id, targetSideId === UNASSIGNED ? undefined : targetSideId)
      return next
    })
  }

  const remove = (id: string) => {
    setRemovedIds((prev) => new Set(prev).add(id))
  }

  const undoRemove = (id: string) => {
    setRemovedIds((prev) => {
      const next = new Set(prev)
      next.delete(id)
      return next
    })
  }

  // What changed vs. the match's own current state — sent as-is even if
  // empty (a no-op save is harmless, and keeps the mutation body uniform).
  const sideAssignmentsBody = useMemo(
    () =>
      match.players
        .filter((p) => !removedIds.has(playerId(p)))
        .filter((p) => assignments.get(playerId(p)) !== p.side_id)
        .map((p) => ({ player_id: playerId(p), side_id: assignments.get(playerId(p)) })),
    [match.players, assignments, removedIds],
  )
  const removedIdsBody = useMemo(() => Array.from(removedIds), [removedIds])
  const dirty = sideAssignmentsBody.length > 0 || removedIdsBody.length > 0

  const save = useMutation({
    mutationFn: async () => {
      const body: components['schemas']['UpdateMatchInput'] = {}
      if (sideAssignmentsBody.length > 0) body.side_assignments = sideAssignmentsBody
      if (removedIdsBody.length > 0) body.removed_player_ids = removedIdsBody
      if (!dirty) return
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body,
      })
      if (error) throw new Error('Failed to save the roster')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      onDone()
    },
  })

  // Removing someone is destructive (no undo once saved), so saving with any
  // pending removals detours through a confirmation naming who's being cut,
  // rather than applying silently. A save with only side moves — reversible,
  // no data lost — goes straight through.
  const removedNames = Array.from(removedIds)
    .map((id) => playersById.get(id))
    .filter((p): p is MatchPlayer => !!p)
    .map((p) => memberName(p.member))
  const handleSaveClick = () => {
    if (removedIds.size > 0) {
      setConfirmOpen(true)
    } else {
      save.mutate()
    }
  }

  const activePlayer = activeId ? playersById.get(activeId) : undefined

  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
      <p className="text-xs text-muted-foreground">
        Drag a player to move them to another side, or remove them from the match.
      </p>

      <DndContext
        sensors={sensors}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
      >
        <div className="flex flex-col gap-3">
          {match.sides.map((side) => (
            <RosterColumn
              key={side.id}
              id={side.id}
              title={side.name?.trim() || 'Side'}
              players={columns.get(side.id) ?? []}
              onRemove={remove}
            />
          ))}
          <RosterColumn
            id={UNASSIGNED}
            title="Unassigned"
            players={columns.get(UNASSIGNED) ?? []}
            onRemove={remove}
          />
        </div>

        <DragOverlay>
          {activePlayer && <PlayerChip player={activePlayer} overlay />}
        </DragOverlay>
      </DndContext>

      {removedIds.size > 0 && (
        <div className="rounded-lg border border-dashed p-2.5">
          <p className="mb-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            Removed
          </p>
          <div className="flex flex-col gap-1">
            {Array.from(removedIds).map((id) => {
              const player = playersById.get(id)
              if (!player) return null
              return (
                <div key={id} className="flex items-center justify-between gap-2 text-sm">
                  <span className="truncate text-muted-foreground line-through">
                    {memberName(player.member)}
                  </span>
                  <button
                    type="button"
                    onClick={() => undoRemove(id)}
                    className="shrink-0 text-xs text-primary hover:underline"
                  >
                    Undo
                  </button>
                </div>
              )
            })}
          </div>
        </div>
      )}

      {save.isError && (
        <p className="text-xs text-destructive">
          Something went wrong. Please try again.
        </p>
      )}

      <div className="flex gap-2">
        <Button
          size="sm"
          disabled={!dirty || save.isPending}
          onClick={handleSaveClick}
        >
          {save.isPending ? 'Saving…' : 'Save'}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={save.isPending}
          onClick={onDone}
        >
          Cancel
        </Button>
      </div>

      <Dialog open={confirmOpen} onOpenChange={(open) => !save.isPending && setConfirmOpen(open)}>
        <DialogContent className="max-w-sm text-center sm:text-center">
          <DialogHeader className="text-center sm:text-center">
            <DialogTitle>Remove {removedNames.length === 1 ? 'this player' : 'these players'}?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            You're removing {joinNames(removedNames)} from the match. This can't be undone
            once saved.
          </p>
          {save.isError && (
            <p className="text-xs text-destructive">
              Something went wrong. Please try again.
            </p>
          )}
          <div className="mt-2 flex flex-col gap-2">
            <Button
              type="button"
              variant="destructive"
              disabled={save.isPending}
              onClick={() => save.mutate()}
            >
              {save.isPending ? 'Saving…' : 'Yes, remove them'}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={save.isPending}
              onClick={() => setConfirmOpen(false)}
            >
              Cancel
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  )
}

/** One side's (or "Unassigned"'s) drop zone: a titled list of draggable
 *  player chips. Highlights while a dragged player is over it. */
function RosterColumn({
  id,
  title,
  players,
  onRemove,
}: {
  id: string
  title: string
  players: MatchPlayer[]
  onRemove: (id: string) => void
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
      <p className="mb-2 truncate text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </p>
      <div className="flex min-h-11 flex-col gap-1.5">
        {players.length === 0 && (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            {id === UNASSIGNED ? 'Drop a player here to unassign them.' : 'No players.'}
          </p>
        )}
        {players.map((p) => (
          <DraggablePlayer key={playerId(p)} player={p} onRemove={onRemove} />
        ))}
      </div>
    </div>
  )
}

function DraggablePlayer({
  player,
  onRemove,
}: {
  player: MatchPlayer
  onRemove: (id: string) => void
}) {
  const id = playerId(player)
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id })

  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn('touch-none', isDragging && 'opacity-30')}
    >
      <PlayerChip player={player} onRemove={onRemove} />
    </div>
  )
}

/** The visual row shared by a column's live chip and the drag overlay clone. */
function PlayerChip({
  player,
  onRemove,
  overlay = false,
}: {
  player: MatchPlayer
  onRemove?: (id: string) => void
  overlay?: boolean
}) {
  const name = memberName(player.member)
  const avatarUrl = memberAvatarUrl(player.member)
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md bg-card px-2 py-1.5',
        overlay && 'border shadow-lg',
      )}
    >
      <GripVertical className="size-3.5 shrink-0 text-muted-foreground" />
      <Avatar name={name} imageUrl={avatarUrl} size="md" />
      <span className="flex-1 truncate text-sm">{name}</span>
      {onRemove && (
        <button
          type="button"
          onClick={(e) => {
            // The chip's whole surface starts a drag — keep the remove click
            // from also being read as one.
            e.stopPropagation()
            onRemove(playerId(player))
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
}
