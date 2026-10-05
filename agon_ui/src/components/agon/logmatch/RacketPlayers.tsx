import { initials } from '@/lib/members'
import { useState } from 'react'
import { Plus, X } from 'lucide-react'
import { Avatar } from '../Avatar'
import type { TaggedPlayer } from '../PlayerSideEditor'
import { PlayerSearchInput } from '../PlayerSearchInput'
import { CARD, FormSection, Segmented } from './parts'

export type RacketFormat = 'singles' | 'doubles'

const RACKET_FORMAT_OPTIONS: { value: RacketFormat; label: string }[] = [
  { value: 'singles', label: 'Singles · 1v1' },
  { value: 'doubles', label: 'Doubles · 2v2' },
]

interface RacketPlayersProps {
  format: RacketFormat
  onFormatChange: (format: RacketFormat) => void
  sideA: TaggedPlayer[]
  sideB: TaggedPlayer[]
  onSideAChange: (players: TaggedPlayer[]) => void
  onSideBChange: (players: TaggedPlayer[]) => void
  currentUserId?: string
}

/**
 * Players for a racket sport: an explicit Singles / Doubles choice, then
 * either a "you vs opponent" picker (singles) or two pair lists capped at
 * two players each (doubles). Sides here are identified by their players'
 * names, so there's no side name or colour to fill in.
 */
export function RacketPlayers(props: RacketPlayersProps) {
  const { format, onFormatChange } = props
  return (
    <>
      <FormSection title="Format">
        <Segmented
          label="Singles or doubles"
          options={RACKET_FORMAT_OPTIONS}
          value={format}
          onChange={onFormatChange}
        />
      </FormSection>
      {format === 'singles' ? <SinglesPicker {...props} /> : <DoublesPicker {...props} />}
    </>
  )
}

function SinglesPicker({ sideA, sideB, onSideAChange, onSideBChange, currentUserId }: RacketPlayersProps) {
  const [open, setOpen] = useState<'a' | 'b' | null>(null)
  const taken = [...sideA, ...sideB]
  const slot = (key: 'a' | 'b', players: TaggedPlayer[], onChange: (p: TaggedPlayer[]) => void) => {
    const player = players[0]
    if (player) {
      const isYou = player.kind === 'user' && player.id === currentUserId
      return (
        <div className="flex flex-col items-center gap-2 text-center">
          <div className="relative">
            {player.kind === 'user' ? (
              <Avatar name={player.name} imageUrl={player.imageUrl} size="xl" className="size-14 text-base" />
            ) : (
              <span className="flex size-14 items-center justify-center rounded-full bg-muted text-base font-bold text-muted-foreground">
                {initials(player.name)}
              </span>
            )}
            <button
              type="button"
              onClick={() => onChange([])}
              aria-label={`Remove ${player.name}`}
              className="absolute -right-1 -top-1 flex size-6 items-center justify-center rounded-full border bg-card text-muted-foreground hover:text-foreground"
            >
              <X className="size-3" />
            </button>
          </div>
          <span className="max-w-full truncate text-sm font-bold" data-testid="tagged-player-name">
            {isYou ? 'You' : player.name}
          </span>
        </div>
      )
    }
    return (
      <button
        type="button"
        onClick={() => setOpen(open === key ? null : key)}
        aria-expanded={open === key}
        className="flex flex-col items-center gap-2 text-primary"
      >
        <span className="flex size-14 items-center justify-center rounded-full border-[1.5px] border-dashed border-primary/50">
          <Plus className="size-5" />
        </span>
        <span className="text-sm font-bold">{key === 'a' ? 'Invite player' : 'Invite opponent'}</span>
      </button>
    )
  }

  return (
    <div className="flex flex-col gap-2.5">
      <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2.5 rounded-[20px] border bg-card px-3.5 py-[18px]">
        {slot('a', sideA, onSideAChange)}
        <span className="flex h-[26px] items-center rounded-full bg-accent px-2.5 text-[11px] font-bold text-accent-foreground">
          vs
        </span>
        {slot('b', sideB, onSideBChange)}
      </div>
      {open && (open === 'a' ? sideA : sideB).length === 0 && (
        <PlayerSearchInput
          autoFocus
          placeholder={open === 'a' ? 'Invite a player or add a guest' : 'Invite your opponent or add a guest'}
          taken={taken}
          onAdd={(p) => {
            if (open === 'a') onSideAChange([p])
            else onSideBChange([p])
            setOpen(null)
          }}
        />
      )}
    </div>
  )
}

function DoublesPicker({ sideA, sideB, onSideAChange, onSideBChange, currentUserId }: RacketPlayersProps) {
  const taken = [...sideA, ...sideB]
  const youOnA = sideA.some((p) => p.kind === 'user' && p.id === currentUserId)
  return (
    <>
      <Pair
        title={youOnA ? 'Your pair' : 'Pair 1'}
        players={sideA}
        onChange={onSideAChange}
        taken={taken}
        currentUserId={currentUserId}
        placeholder={sideA.length === 0 ? 'Invite a player or add a guest' : 'Invite your partner or add a guest'}
      />
      <Pair
        title={youOnA ? 'Opponents' : 'Pair 2'}
        players={sideB}
        onChange={onSideBChange}
        taken={taken}
        currentUserId={currentUserId}
        placeholder={sideB.length === 0 ? 'Invite an opponent or add a guest' : 'Invite their partner or add a guest'}
      />
    </>
  )
}

function Pair({
  title,
  players,
  onChange,
  taken,
  currentUserId,
  placeholder,
}: {
  title: string
  players: TaggedPlayer[]
  onChange: (players: TaggedPlayer[]) => void
  taken: TaggedPlayer[]
  currentUserId?: string
  placeholder: string
}) {
  return (
    <FormSection title={title} aside={`${players.length} of 2`}>
      <div className={CARD}>
        {players.map((p, i) => {
          const isYou = p.kind === 'user' && p.id === currentUserId
          return (
            <div key={p.kind === 'user' ? p.id : p.name} className="flex h-11 items-center gap-2.5 rounded-[14px] bg-secondary px-1.5">
              {p.kind === 'user' ? (
                <Avatar name={p.name} imageUrl={p.imageUrl} size="lg" className="size-8" />
              ) : (
                <span className="flex size-8 items-center justify-center rounded-full bg-muted text-xs font-bold text-muted-foreground">
                  {initials(p.name)}
                </span>
              )}
              <span className="flex-1 truncate text-sm font-semibold" data-testid="tagged-player-name">
                {isYou ? `${p.name} (you)` : p.name}
              </span>
              {p.kind === 'external' && <span className="text-[11px] font-semibold text-muted-foreground">Guest</span>}
              <button
                type="button"
                onClick={() => onChange(players.filter((_, j) => j !== i))}
                aria-label={`Remove ${p.name}`}
                className="flex size-8 items-center justify-center text-muted-foreground hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            </div>
          )
        })}
        {players.length < 2 && (
          <PlayerSearchInput
            placeholder={placeholder}
            taken={taken}
            onAdd={(p) => onChange([...players, p])}
          />
        )}
      </div>
    </FormSection>
  )
}
