import { CircleAlert } from 'lucide-react'
import type { components } from '@/types/api'
import type { TaggedPlayer } from '../PlayerSideEditor'
import { PlayerSearchInput } from '../PlayerSearchInput'
import { TeamPicker } from '../TeamPicker'
import { CARD, ColourSwatches, FormSection, PlayerChip, Segmented, ToggleRow } from './parts'

type TeamListItem = components['schemas']['TeamListItem']

export type SideKind = 'team' | 'oneoff'

const KIND_OPTIONS: { value: SideKind; label: string }[] = [
  { value: 'team', label: 'Existing team' },
  { value: 'oneoff', label: 'One-off side' },
]

export interface TeamSideCardProps {
  title: string
  idPrefix: string
  kind: SideKind
  onKindChange: (kind: SideKind) => void
  team: TeamListItem | null
  onTeamChange: (team: TeamListItem | null) => void
  /** Both sides are linked to this same team, so each needs its own name
   *  and colour to tell them apart. */
  sharedTeam: boolean
  name: string
  onNameChange: (name: string) => void
  colour: string
  onColourChange: (hex: string) => void
  teamJoinEnabled: boolean
  onTeamJoinEnabledChange: (enabled: boolean) => void
  players: TaggedPlayer[]
  onPlayersChange: (players: TaggedPlayer[]) => void
  /** Everyone tagged on either side, so nobody is offered twice. */
  taken: TaggedPlayer[]
  currentUserId?: string
  maxPlayers: string
  onMaxPlayersChange: (value: string) => void
}

/**
 * One side of a team-sport match: an existing team or a one-off side (a
 * name and colour for this match only), its players, and its player cap.
 * When both sides are the same team, a name and colour show here too.
 */
export function TeamSideCard(p: TeamSideCardProps) {
  const identityVisible = p.kind === 'oneoff' || (p.team !== null && p.sharedTeam)
  return (
    <FormSection title={p.title}>
      <div className={CARD}>
        <Segmented label={`${p.title} is`} options={KIND_OPTIONS} value={p.kind} onChange={p.onKindChange} />

        {p.kind === 'oneoff' && (
          <span className="-mt-1 text-xs text-muted-foreground">
            A name and colour just for this match, not a saved team.
          </span>
        )}

        {p.kind === 'team' && (
          <TeamPicker team={p.team} onChange={p.onTeamChange} placeholder="Find a team…" />
        )}

        {p.kind === 'team' && p.team && p.sharedTeam && (
          <div className="flex items-start gap-2 rounded-xl bg-warning/25 px-3 py-2.5 text-[13px] leading-snug">
            <CircleAlert className="mt-px size-4 shrink-0" />
            <span>{p.team.name} is on both sides, so give each side its own name and colour.</span>
          </div>
        )}

        {identityVisible && (
          <>
            <div className="flex flex-col gap-1">
              <label htmlFor={`${p.idPrefix}-name`} className="text-xs font-semibold text-muted-foreground">
                Side name
              </label>
              <input
                id={`${p.idPrefix}-name`}
                value={p.name}
                onChange={(e) => p.onNameChange(e.target.value)}
                placeholder="e.g. Whites"
                maxLength={60}
                className="h-[46px] rounded-xl border bg-card px-3 text-[15px] font-semibold outline-none placeholder:font-normal placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            </div>
            <ColourSwatches label={`${p.title} colour`} value={p.colour} onChange={p.onColourChange} />
          </>
        )}

        {p.kind === 'team' && p.team && (
          <ToggleRow
            label="Let the team join this side"
            hint="Members can add themselves"
            checked={p.teamJoinEnabled}
            onChange={p.onTeamJoinEnabledChange}
          />
        )}

        {p.players.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {p.players.map((player, i) => (
              <PlayerChip
                key={player.kind === 'user' ? player.id : player.name}
                player={player}
                currentUserId={p.currentUserId}
                onRemove={() => p.onPlayersChange(p.players.filter((_, j) => j !== i))}
              />
            ))}
          </div>
        )}

        <PlayerSearchInput
          placeholder="Invite a player or add a guest"
          taken={p.taken}
          onAdd={(player) => p.onPlayersChange([...p.players, player])}
        />

        <label className="flex min-h-12 items-center gap-2.5 border-t pt-2.5 text-sm font-semibold">
          <span className="flex-1">
            Max players
            <span className="block text-xs font-normal text-muted-foreground">
              Extra people go on the waiting list
            </span>
          </span>
          <input
            type="number"
            min={1}
            inputMode="numeric"
            value={p.maxPlayers}
            onChange={(e) => p.onMaxPlayersChange(e.target.value)}
            placeholder="No cap"
            aria-label={`${p.title} max players`}
            className="h-10 w-[84px] rounded-[10px] border bg-card px-2 text-center [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none text-[15px] font-semibold outline-none placeholder:text-sm placeholder:font-normal focus-visible:ring-2 focus-visible:ring-ring"
          />
        </label>
      </div>
    </FormSection>
  )
}
