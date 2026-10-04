import { X } from 'lucide-react'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { TeamPicker } from './TeamPicker'
import { PlayerSearchInput } from './PlayerSearchInput'
import { taggedPlayerKey } from '@/lib/logMatch'
import { SIDE_COLOURS } from '@/lib/sideColours'
import { cn } from '@/lib/utils'

type TeamListItem = components['schemas']['TeamListItem']

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
  /** This side's colour, for an ad-hoc side (the create-match API requires
   *  one whenever there's no `team_id`). Rendered alongside the name field —
   *  hidden by the same `nameFieldVisible` a linked team hides it behind,
   *  since a linked team is the colour's source of truth instead. */
  colour?: string
  onColourChange?: (hex: string) => void
}

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
  colour,
  onColourChange,
}: PlayerSideEditorProps) {
  const removeAt = (index: number) => {
    onChange(players.filter((_, i) => i !== index))
  }

  return (
    <div className="flex flex-col gap-2">
      <p className="px-1 font-display text-[15px] font-bold">{title}</p>

      {onTeamChange && <TeamPicker team={team} onChange={onTeamChange} />}

      {onNameChange && nameFieldVisible && (
        <input
          type="text"
          value={name ?? ''}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder="Name this side (optional)"
          maxLength={60}
          className="w-full rounded-xl border bg-card px-3 py-2 text-sm outline-none placeholder:text-muted-foreground"
        />
      )}
      {onNameChange && !nameFieldVisible && team && (
        <p className="text-xs text-muted-foreground">
          Linked to {team.name} — link the other side to the same team to give
          each a custom name.
        </p>
      )}

      {onColourChange && nameFieldVisible && (
        <div className="flex items-center gap-1.5 px-1" role="radiogroup" aria-label={`${title} colour`}>
          {SIDE_COLOURS.map((c) => (
            <button
              key={c.hex}
              type="button"
              role="radio"
              aria-checked={colour === c.hex}
              aria-label={c.label}
              onClick={() => onColourChange(c.hex)}
              className={cn(
                'size-6 shrink-0 rounded-full border transition-shadow',
                colour === c.hex
                  ? 'ring-2 ring-primary ring-offset-1 ring-offset-card'
                  : 'border-border/60',
              )}
              style={{ backgroundColor: c.hex }}
            />
          ))}
        </div>
      )}

      <div className="flex flex-col gap-3 rounded-2xl border bg-card p-3.5">
        {players.length === 0 ? (
          <p className="px-1 py-1 text-xs text-muted-foreground">
            No players yet.
          </p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {players.map((p, i) => {
              const isYou = p.kind === 'user' && p.id === currentUserId
              return (
                <div
                  key={taggedPlayerKey(p)}
                  className="inline-flex h-10 items-center gap-2 rounded-full bg-secondary py-1 pl-1 pr-3"
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
                  <span className="truncate text-sm font-medium" data-testid="tagged-player-name">
                    {p.name}
                  </span>
                  {isYou && (
                    <span className="text-[10px] font-medium text-primary">(you)</span>
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
                    <X className="size-3.5" />
                  </button>
                </div>
              )
            })}
          </div>
        )}

        <PlayerSearchInput
          placeholder={searchPlaceholder}
          taken={players}
          excludeUserIds={currentUserId ? [currentUserId, ...excludeUserIds] : excludeUserIds}
          onAdd={(p) => onChange([...players, p])}
        />
      </div>
    </div>
  )
}
