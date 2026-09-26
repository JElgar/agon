import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { PlayerRow, SideHeading, SideSwatch, cardClass } from '@/components/agon/football/FootballMatchView'
import { useViewerFollowing } from '@/hooks/useViewerFollowing'

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']

function sideLabel(match: Match, idx: number): string {
  return match.sides[idx]?.name?.trim() || (idx === 0 ? 'Side A' : 'Side B')
}

/** "Set by set": games per set for each side, the set winner in bold. */
export function SetsCard({ match, score }: { match: Match; score: Score }) {
  if (score.type !== 'Sets') return null
  const [a, b] = match.sides
  const sa = score.entries[a?.id ?? ''] ?? []
  const sb = score.entries[b?.id ?? ''] ?? []
  const count = Math.max(sa.length, sb.length)
  if (count === 0) return null
  const won = (mine: number[], theirs: number[]) =>
    Array.from({ length: count }, (_, i) => (mine[i] ?? 0) > (theirs[i] ?? 0)).filter(Boolean).length
  const setsA = won(sa, sb)
  const setsB = won(sb, sa)
  const grid = { gridTemplateColumns: `minmax(0,1fr) repeat(${count}, 34px) 40px` }

  const row = (idx: number, mine: number[], theirs: number[], sets: number, otherSets: number) => (
    <div style={grid} className="grid min-h-11 items-center gap-x-1 border-t border-hairline text-sm tabular-nums">
      <span className="flex min-w-0 items-center gap-2 font-semibold">
        <SideSwatch index={idx} size={10} />
        <span className="truncate">{sideLabel(match, idx)}</span>
      </span>
      {Array.from({ length: count }, (_, i) => {
        const g = mine[i] ?? 0
        const setWon = g > (theirs[i] ?? 0)
        return (
          <span key={i} className={cn('text-center text-base', setWon ? 'font-extrabold text-foreground' : 'text-muted-foreground')}>
            {g}
          </span>
        )
      })}
      <span className={cn('text-right font-display text-lg font-extrabold', sets < otherSets && 'text-ink-faint')}>{sets}</span>
    </div>
  )

  return (
    <section className={cn(cardClass, 'flex flex-col pb-2')}>
      <span className="pb-2.5 text-[15px] font-bold">Set by set</span>
      <div style={grid} className="grid items-center gap-x-1 pb-2 text-[11px] font-bold tracking-[0.4px] text-muted-foreground">
        <span />
        {Array.from({ length: count }, (_, i) => (
          <span key={i} className="text-center">
            S{i + 1}
          </span>
        ))}
        <span className="text-right">Sets</span>
      </div>
      {row(0, sa, sb, setsA, setsB)}
      {row(1, sb, sa, setsB, setsA)}
    </section>
  )
}

/** The Players tab for sports without per-player stats: each side's roster
 *  with Follow buttons, and the result beside each side's name. */
export function RosterTab({
  match,
  currentUserId,
  result,
  footer,
}: {
  match: Match
  currentUserId?: string
  /** "Won" / "Lost" / "Drew" per side id, once there's a result. */
  result?: Record<string, string>
  footer?: React.ReactNode
}) {
  const following = useViewerFollowing(currentUserId)
  const unassigned = match.players.filter((p) => !match.sides.some((s) => s.id === p.side_id))
  const row = (p: MatchPlayer, i: number) => (
    <PlayerRow key={p.member.id} player={p} first={i === 0} currentUserId={currentUserId} followingIds={following.data} />
  )
  const count = (n: number) => `${n} ${n === 1 ? 'player' : 'players'}`
  return (
    <>
      {match.sides.map((side, idx) => {
        const players = match.players.filter((p) => p.side_id === side.id)
        const res = result?.[side.id]
        return (
          <div key={side.id} className="flex flex-col gap-3.5">
            <SideHeading index={idx} name={sideLabel(match, idx)} meta={`${res ? `${res} · ` : ''}${count(players.length)}`} />
            <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
              {players.length === 0 && <p className="px-4 py-5 text-sm text-muted-foreground">No players yet.</p>}
              {players.map(row)}
            </section>
          </div>
        )
      })}
      {unassigned.length > 0 && (
        <div className="flex flex-col gap-3.5">
          <div className="mt-1.5 flex items-center gap-2.5 px-1">
            <span className="flex-1 font-display text-[19px] font-bold">Not on a side yet</span>
            <span className="text-[13px] text-muted-foreground">{count(unassigned.length)}</span>
          </div>
          <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">{unassigned.map(row)}</section>
        </div>
      )}
      {footer}
    </>
  )
}
