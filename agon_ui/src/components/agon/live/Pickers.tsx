import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Avatar } from '@/components/agon/Avatar'
import { memberName, playerId } from '@/lib/members'

type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']

export function sideName(side: MatchSide, fallback: string): string {
  return side.name?.trim() || fallback
}

/** A row of tappable side tiles — the first step for most live-event entry
 *  flows (football's goal/card/sub dialog, cricket's innings-start panel). */
export function SidePicker({
  sides,
  value,
  onChange,
}: {
  sides: MatchSide[]
  value: string | undefined
  onChange: (sideId: string) => void
}) {
  return (
    <div className="grid grid-cols-2 gap-2">
      {sides.map((side, i) => (
        <button
          key={side.id}
          type="button"
          aria-pressed={value === side.id}
          onClick={() => onChange(side.id)}
          className={cn(
            'rounded-lg border p-3 text-sm font-medium transition-colors',
            value === side.id
              ? 'border-primary bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:bg-muted',
          )}
        >
          {sideName(side, i === 0 ? 'Side A' : 'Side B')}
        </button>
      ))}
    </div>
  )
}

/** A vertical list of tappable player rows for one side's roster. `exclude`
 *  drops one or more player ids already spoken for (e.g. the other batter,
 *  or the bowler who just finished an over). */
export function PlayerPicker({
  players,
  value,
  onChange,
  exclude,
  emptyLabel = 'No players on this side yet.',
}: {
  players: MatchPlayer[]
  value: string | undefined | null
  onChange: (playerId: string) => void
  exclude?: string | (string | null | undefined)[]
  emptyLabel?: string
}) {
  const excluded = new Set(
    (Array.isArray(exclude) ? exclude : [exclude]).filter((id): id is string => !!id),
  )
  const options = players.filter((p) => !excluded.has(playerId(p)))
  if (options.length === 0) {
    return <p className="text-xs text-muted-foreground">{emptyLabel}</p>
  }
  return (
    <div className="flex max-h-48 flex-col gap-1 overflow-y-auto">
      {options.map((p) => {
        const id = playerId(p)
        const name = memberName(p.member)
        return (
          <button
            key={id}
            type="button"
            aria-pressed={value === id}
            onClick={() => onChange(id)}
            className={cn(
              'flex items-center gap-2 rounded-lg border px-2.5 py-2 text-left text-sm transition-colors',
              value === id
                ? 'border-primary bg-accent text-accent-foreground'
                : 'hover:bg-muted',
            )}
          >
            <Avatar name={name} size="sm" />
            <span className="truncate">{name}</span>
          </button>
        )
      })}
    </div>
  )
}
