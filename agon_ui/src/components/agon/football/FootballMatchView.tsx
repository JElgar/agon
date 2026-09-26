import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { ChevronRight, Flame, MessageCircle, Plus, Share } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { fetchClient } from '@/lib/api-client'
import { relativeTime } from '@/lib/datetime'
import { footballFormat } from '@/lib/matchFormat'
import {
  eventClockLabel,
  eventsFromDetail,
  goalContributions,
  minuteAt,
  playerInfoFor,
  sortGoalContributions,
  type FootballEventKind,
  type FootballEventSource,
  type FootballEventView,
} from '@/lib/liveScore'
import { initials, memberAvatarUrl, memberName, type ScorePlayers } from '@/lib/members'
import { FollowButton } from '@/components/agon/FollowButton'
import { useViewerFollowing } from '@/hooks/useViewerFollowing'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type Comment = components['schemas']['Comment']
type CommentPage = components['schemas']['CommentPage']

// Redesign accents: the neutral "second kit" grey and the assists teal. Both
// are theme tokens in index.css so they follow dark mode.
export const KIT_GREY = 'var(--kit-grey)'
const ASSIST_TEAL = 'var(--assist)'
const ASSIST_TEAL_TEXT = 'var(--assist-foreground)'

// Pastel avatar backgrounds the mocks use for initials — picked stably per name.
// The dark board keeps the same pastels, so these stay fixed (initials use --avatar-ink).
const AVATAR_TINTS = ['#D8DDF7', '#CFE3D4', '#F2E3B3', '#D5ECEC', '#F9D9C9', '#E6D5F2']
function tintFor(name: string): string {
  let h = 0
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return AVATAR_TINTS[h % AVATAR_TINTS.length]
}

/** Initials-on-pastel (or photo) avatar at an exact pixel size, as the mocks draw them. */
export function PersonAvatar({
  name,
  imageUrl,
  size,
  className,
}: {
  name: string
  imageUrl?: string
  size: number
  className?: string
}) {
  const style = { width: size, height: size, fontSize: Math.round(size * 0.34) }
  if (imageUrl) {
    return <img src={imageUrl} alt="" style={style} className={cn('shrink-0 rounded-full object-cover', className)} />
  }
  return (
    <span
      aria-hidden
      style={{ ...style, background: tintFor(name) }}
      className={cn('flex shrink-0 items-center justify-center rounded-full font-bold text-avatar-ink', className)}
    >
      {initials(name)}
    </span>
  )
}

/** A side's kit mark: the team crest when it has one, else side A's solid
 *  blue disc / side B's white disc with a grey ring. */
export function SideDisc({ side, index, size = 44 }: { side: MatchSide | undefined; index: number; size?: number }) {
  const style = { width: size, height: size }
  if (side?.team_logo?.image_url) {
    return <img src={side.team_logo.image_url} alt="" style={style} className="shrink-0 rounded-full border object-cover" />
  }
  return index === 0 ? (
    <span style={style} className="shrink-0 rounded-full bg-primary" />
  ) : (
    <span style={{ ...style, borderColor: KIT_GREY }} className="box-border shrink-0 rounded-full border-[3px] bg-card" />
  )
}

/** Small rounded-square kit swatch used in legends, rows and headings. */
export function SideSwatch({ index, size = 10 }: { index: number; size?: number }) {
  const radius = size >= 12 ? Math.round(size / 3) : Math.max(2, Math.round(size / 3.5))
  return index === 0 ? (
    <span style={{ width: size, height: size, borderRadius: radius }} className="inline-block shrink-0 bg-primary" />
  ) : (
    <span
      style={{ width: size, height: size, borderRadius: radius, borderColor: KIT_GREY, borderWidth: size >= 10 ? 2 : 1.5 }}
      className="box-border inline-block shrink-0 border-solid bg-card"
    />
  )
}

function sideIndex(match: Match, sideId: string | undefined): number {
  return match.sides.findIndex((s) => s.id === sideId)
}

function sideLabel(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/** Numeric match minute for a goal/event: its live-clock minute when it was
 *  live-scored, else the manually entered minute. */
function numericMinute(ev: { minute?: number; occurred_at?: string }, periodTimes?: Record<string, string>): number | null {
  if (ev.occurred_at) {
    const m = minuteAt(periodTimes, ev.occurred_at)
    if (m !== null) return m
  }
  return ev.minute ?? null
}

// ---------------------------------------------------------------------------
// Header pieces
// ---------------------------------------------------------------------------

/** The "Full time" hero card from `MatchFootball.dc.html`, plus the live and
 *  scheduled states in the same layout. */
export function FootballHeroCard({
  match,
  goalsA,
  goalsB,
  state,
  liveLabel,
  kickoffLabel,
  finishedLabel = 'Full time',
  avatars,
  resultText,
}: {
  match: Match
  goalsA: number
  goalsB: number
  state: 'finished' | 'live' | 'scheduled' | 'cancelled'
  liveLabel?: string
  kickoffLabel?: string
  finishedLabel?: string
  /** Replace the kit discs, e.g. profile photos for a 1v1 match. */
  avatars?: [React.ReactNode, React.ReactNode]
  /** Replace the default "X won by N" result pill text. */
  resultText?: string
}) {
  const [sideA, sideB] = match.sides
  const nameA = sideLabel(sideA, 'Side A')
  const nameB = sideLabel(sideB, 'Side B')
  const aWon = state === 'finished' && goalsA > goalsB
  const bWon = state === 'finished' && goalsB > goalsA
  const hasScore = state === 'finished' || state === 'live'

  const label =
    state === 'finished' ? finishedLabel : state === 'live' ? liveLabel : state === 'cancelled' ? 'Cancelled' : kickoffLabel

  const result =
    state !== 'finished' ? null : resultText ? resultText : aWon ? `${nameA} won by ${goalsA - goalsB}` : bWon ? `${nameB} won by ${goalsB - goalsA}` : 'Draw'

  const nameClass = (won: boolean, lost: boolean) =>
    cn('line-clamp-2 max-w-full text-center text-[17px] leading-tight break-words', won || !lost ? 'font-bold' : 'font-medium text-muted-foreground')

  return (
    <section className="flex flex-col items-center gap-3 rounded-[20px] border bg-card px-[18px] pt-5 pb-[18px]">
      {label &&
        (state === 'live' ? (
          <span className="flex items-center gap-1.5 text-xs font-bold text-destructive">
            <span className="size-1.5 animate-pulse rounded-full bg-destructive" />
            {label}
          </span>
        ) : (
          <span className="text-xs font-semibold text-muted-foreground">{label}</span>
        ))}
      <div className="grid w-full grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-3">
        <div className="flex min-w-0 flex-col items-center gap-2">
          {avatars ? avatars[0] : <SideDisc side={sideA} index={0} />}
          <span className={nameClass(aWon, bWon)}>{nameA}</span>
          {state === 'scheduled' && <span className="-mt-1.5 text-[13px] text-muted-foreground">{sideA?.player_count ?? 0} going</span>}
        </div>
        {hasScore ? (
          <div
            className={cn(
              'flex items-center gap-2.5 font-display leading-none font-extrabold',
              Math.max(goalsA, goalsB) >= 10 ? 'text-[40px]' : 'text-5xl',
            )}
          >
            <span className={cn(bWon && 'text-ink-faint')}>{goalsA}</span>
            <span className="text-[28px]" style={{ color: KIT_GREY }}>
              –
            </span>
            <span className={cn(aWon && 'text-ink-faint')}>{goalsB}</span>
          </div>
        ) : (
          <span className="font-display text-[28px] font-extrabold" style={{ color: KIT_GREY }}>
            vs
          </span>
        )}
        <div className="flex min-w-0 flex-col items-center gap-2">
          {avatars ? avatars[1] : <SideDisc side={sideB} index={1} />}
          <span className={nameClass(bWon, aWon)}>{nameB}</span>
          {state === 'scheduled' && <span className="-mt-1.5 text-[13px] text-muted-foreground">{sideB?.player_count ?? 0} going</span>}
        </div>
      </div>
      {result && (
        <span className="flex h-[30px] items-center rounded-full bg-accent px-3 text-sm font-bold text-accent-foreground">
          {result}
        </span>
      )}
    </section>
  )
}

/** Compact one-line score strip that replaces the hero on the Timeline and
 *  Players tabs (`EventsFootball.dc.html` / `PlayersFootball.dc.html`). */
export function FootballScoreStrip({ match, goalsA, goalsB, hasScore }: { match: Match; goalsA: number; goalsB: number; hasScore: boolean }) {
  const [sideA, sideB] = match.sides
  const bLost = hasScore && goalsA > goalsB
  const aLost = hasScore && goalsB > goalsA
  return (
    <div className="flex items-center gap-3 rounded-2xl border bg-card px-4 py-3">
      <span className={cn('flex min-w-0 flex-1 items-center gap-2 text-[15px]', aLost ? 'font-medium text-muted-foreground' : 'font-bold')}>
        <SideSwatch index={0} size={12} />
        <span className="truncate">{sideLabel(sideA, 'Side A')}</span>
      </span>
      <span className="font-display text-2xl font-extrabold">{hasScore ? `${goalsA}–${goalsB}` : 'vs'}</span>
      <span
        className={cn(
          'flex min-w-0 flex-1 items-center justify-end gap-2 text-[15px]',
          bLost ? 'font-medium text-muted-foreground' : 'font-bold',
        )}
      >
        <span className="truncate">{sideLabel(sideB, 'Side B')}</span>
        <SideSwatch index={1} size={12} />
      </span>
    </div>
  )
}

export type FootballTab = 'summary' | 'timeline' | 'players'
const TABS: { id: FootballTab; label: string }[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'players', label: 'Players' },
]

export function FootballTabBar({ value, onChange }: { value: FootballTab; onChange: (tab: FootballTab) => void }) {
  return <MatchTabBar tabs={TABS} value={value} onChange={onChange} />
}

/** The segmented Summary/…/Players tab bar every redesigned match page uses. */
export function MatchTabBar<T extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: { id: T; label: string }[]
  value: T
  onChange: (tab: T) => void
}) {
  return (
    <div
      role="tablist"
      aria-label="Match sections"
      style={{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }}
      className="grid gap-1 rounded-[14px] bg-border p-1"
    >
      {tabs.map((tab) => {
        const active = value === tab.id
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(tab.id)}
            className={cn(
              'flex h-10 items-center justify-center rounded-[10px] text-sm transition-colors',
              active ? 'bg-card font-bold text-foreground' : 'font-semibold text-ink-mid hover:text-foreground',
            )}
          >
            {tab.label}
          </button>
        )
      })}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Summary tab
// ---------------------------------------------------------------------------

export const cardClass = 'rounded-[20px] border bg-card px-[18px] py-4'

/** "Your game · Whites / 2 goals / 52' and 61' · 3 assists" — the viewer's own line. */
export function YourGameCard({
  match,
  me,
  detail,
}: {
  match: Match
  me: MatchPlayer
  detail: FootballEventSource
}) {
  const myId = me.member.id
  const myGoals = detail.goals.filter((g) => !g.own_goal && g.scorer_player_id === myId)
  const myAssists = detail.goals.filter((g) => g.assist_player_id === myId).length
  const minutes = myGoals
    .map((g) => eventClockLabel({ kind: 'goal', side_id: g.side_id, minute: g.minute, occurred_at: g.occurred_at }, detail.period_times))
    .filter(Boolean)
  const side = match.sides.find((s) => s.id === me.side_id)
  const name = memberName(me.member)

  const headline = myGoals.length > 0 ? `${myGoals.length} ${myGoals.length === 1 ? 'goal' : 'goals'}` : myAssists > 0 ? `${myAssists} ${myAssists === 1 ? 'assist' : 'assists'}` : 'Played'
  const detailParts: string[] = []
  if (minutes.length > 0) detailParts.push(minutes.length === 1 ? minutes[0] : `${minutes.slice(0, -1).join(', ')} and ${minutes[minutes.length - 1]}`)
  if (myGoals.length > 0 && myAssists > 0) detailParts.push(`${myAssists} ${myAssists === 1 ? 'assist' : 'assists'}`)
  if (myGoals.length === 0 && myAssists === 0) detailParts.push('No goal involvements this time')

  return (
    <section className={cn(cardClass, 'flex items-center gap-3.5')}>
      <PersonAvatar name={name} imageUrl={memberAvatarUrl(me.member)} size={48} />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-[13px] font-semibold text-muted-foreground">
          Your game{side ? ` · ${sideLabel(side, '')}` : ''}
        </span>
        <span className="text-[17px] font-bold">{headline}</span>
        <span className="text-[13px] text-muted-foreground">{detailParts.join(' · ')}</span>
      </div>
      <span className="font-display text-4xl font-extrabold">{myGoals.length || myAssists}</span>
    </section>
  )
}

/** "Goals & assists" leaderboard with split goal/assist bars. */
export function GoalsAssistsCard({ match, detail }: { match: Match; detail: FootballEventSource }) {
  const [showAll, setShowAll] = useState(false)
  const rows = useMemo(
    () => sortGoalContributions(goalContributions(detail.goals, match, detail.players), 'goals'),
    [detail, match],
  )
  if (rows.length === 0) return null
  const max = Math.max(...rows.map((r) => r.goals + r.assists)) + 1
  const visible = showAll ? rows : rows.slice(0, 6)
  const grid = 'grid grid-cols-[124px_minmax(0,1fr)_22px_22px] items-center gap-2'

  return (
    <section className={cn(cardClass, 'flex flex-col gap-2.5')}>
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">Goals &amp; assists</span>
        <div className="flex gap-3 text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-full bg-primary" />
            Goals
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-full" style={{ background: ASSIST_TEAL }} />
            Assists
          </span>
        </div>
      </div>
      <div className={cn(grid, 'text-[11px] font-bold tracking-[0.4px] text-muted-foreground')}>
        <span />
        <span />
        <span className="text-right">G</span>
        <span className="text-right">A</span>
      </div>
      {visible.map((r) => (
        <div key={r.key} className={cn(grid, 'min-h-[30px]')}>
          <span className="flex items-center gap-2 overflow-hidden text-sm font-semibold whitespace-nowrap">
            <SideSwatch index={sideIndex(match, r.side_id)} size={10} />
            {r.userId ? (
              <Link to={`/users/${r.userId}`} className="truncate text-foreground hover:underline">
                {r.name}
              </Link>
            ) : (
              <span className="truncate">{r.name}</span>
            )}
          </span>
          <span aria-label={`${r.goals} goals, ${r.assists} assists`} className="flex h-2 gap-0.5">
            {r.goals > 0 && <span className="rounded bg-primary" style={{ width: `${(r.goals / max) * 100}%` }} />}
            {r.assists > 0 && (
              <span className="rounded" style={{ width: `${(r.assists / max) * 100}%`, background: ASSIST_TEAL }} />
            )}
          </span>
          <span className="text-right font-display text-base font-extrabold">{r.goals}</span>
          <span className="text-right font-display text-base font-extrabold" style={{ color: ASSIST_TEAL_TEXT }}>
            {r.assists}
          </span>
        </div>
      ))}
      {rows.length > 6 && (
        <button
          type="button"
          onClick={() => setShowAll((v) => !v)}
          className="mt-0.5 h-11 border-t border-hairline text-sm font-bold text-primary"
        >
          {showAll ? 'Show fewer' : 'See everyone'}
        </button>
      )}
    </section>
  )
}

/** "How it unfolded" — cumulative goals per side as a step chart, with a
 *  dashed half-time marker. */
export function ScoreFlowCard({
  match,
  detail,
  nowMinute,
}: {
  match: Match
  detail: FootballEventSource
  /** While live, lines stop at the current minute instead of running to full time. */
  nowMinute?: number
}) {
  const [sideA, sideB] = match.sides
  const fmt = footballFormat(match.format)
  const halfLen = fmt.half_length_minutes
  const goals = detail.goals
    .map((g) => ({ side: g.side_id, minute: numericMinute(g, detail.period_times) }))
    .filter((g): g is { side: string; minute: number } => g.minute !== null)
    .sort((a, b) => a.minute - b.minute)
  if (goals.length === 0 || !sideA || !sideB) return null

  const fullLen = Math.max(fmt.num_halves * halfLen, ...goals.map((g) => g.minute))
  const totalA = goals.filter((g) => g.side === sideA.id).length
  const totalB = goals.filter((g) => g.side === sideB.id).length
  const htA = goals.filter((g) => g.side === sideA.id && g.minute <= halfLen).length
  const htB = goals.filter((g) => g.side === sideB.id && g.minute <= halfLen).length
  const showHt = fmt.num_halves >= 2 && fullLen > halfLen && (nowMinute === undefined || nowMinute >= halfLen)

  // Plot area mirrors the mock's 322×190 viewBox.
  const x0 = 30
  const x1 = 314
  const yBase = 162
  const yTop = 12
  const maxGoals = Math.max(totalA, totalB, 1)
  const yStep = maxGoals <= 4 ? 1 : maxGoals <= 8 ? 2 : maxGoals <= 20 ? 5 : 10
  const yMax = Math.ceil(maxGoals / yStep) * yStep + (maxGoals % yStep === 0 ? yStep : 0)
  const xFor = (m: number) => x0 + (Math.min(m, fullLen) / fullLen) * (x1 - x0)
  const yFor = (v: number) => yBase - (v / yMax) * (yBase - yTop)
  const endX = nowMinute !== undefined ? xFor(nowMinute) : x1
  const xStep = fullLen <= 30 ? 5 : fullLen <= 60 ? 10 : 20
  const xTicks: number[] = []
  for (let m = 0; m <= fullLen; m += xStep) xTicks.push(m)
  const yTicks: number[] = []
  for (let v = 0; v <= yMax; v += yStep) yTicks.push(v)

  const path = (sideId: string) => {
    let d = `M${x0} ${yBase}`
    let count = 0
    for (const g of goals) {
      if (g.side !== sideId) continue
      count += 1
      d += ` H${xFor(g.minute).toFixed(1)} V${yFor(count).toFixed(1)}`
    }
    return `${d} H${nowMinute !== undefined ? xFor(nowMinute).toFixed(1) : x1}`
  }
  const nameA = sideLabel(sideA, 'Side A')
  const nameB = sideLabel(sideB, 'Side B')

  return (
    <section className={cn(cardClass, 'flex flex-col gap-3')}>
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">How it unfolded</span>
        {showHt && (
          <span className="text-[13px] text-muted-foreground">
            HT {htA}–{htB}
          </span>
        )}
      </div>
      <div className="flex gap-3.5 text-xs text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <span className="h-[3px] w-3.5 rounded-sm bg-primary" />
          {nameA}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="h-[3px] w-3.5 rounded-sm" style={{ background: KIT_GREY }} />
          {nameB}
        </span>
      </div>
      <svg
        width="100%"
        viewBox="0 0 322 190"
        role="img"
        aria-label={`Score flow: ${nameA} ${totalA}, ${nameB} ${totalB}`}
        className="block font-sans"
      >
        {yTicks.map((v) => (
          <g key={v}>
            <line x1={x0} x2={x1} y1={yFor(v)} y2={yFor(v)} stroke="var(--gridline)" strokeWidth="1" />
            <text x={x0 - 8} y={yFor(v) + 4} textAnchor="end" fontSize="11" className="fill-muted-foreground">
              {v}
            </text>
          </g>
        ))}
        {xTicks.map((m) => (
          <text key={m} x={xFor(m)} y="180" textAnchor="middle" fontSize="11" className="fill-muted-foreground">
            {m}'
          </text>
        ))}
        {showHt && (
          <>
            <line x1={xFor(halfLen)} x2={xFor(halfLen)} y1={yTop} y2={yBase} stroke={KIT_GREY} strokeWidth="1" strokeDasharray="3 4" />
            <text x={xFor(halfLen) + 5} y="22" fontSize="11" fontWeight="700" className="fill-muted-foreground">
              HT
            </text>
          </>
        )}
        <path d={path(sideB.id)} fill="none" stroke={KIT_GREY} strokeWidth="2.5" strokeLinejoin="round" />
        <path d={path(sideA.id)} fill="none" className="stroke-primary" strokeWidth="2.5" strokeLinejoin="round" />
        <text x={endX - 4} y={yFor(totalA) - 7} textAnchor="end" fontSize="13" fontWeight="700" className="fill-primary">
          {totalA}
        </text>
        <text
          x={endX - 4}
          y={yFor(totalB) + (Math.abs(yFor(totalB) - yFor(totalA)) < 16 ? 16 : -7)}
          textAnchor="end"
          fontSize="13"
          fontWeight="700"
          className="fill-muted-foreground"
        >
          {totalB}
        </text>
      </svg>
    </section>
  )
}

/** "Who played" — overlapping avatar stacks per side. */
export function WhoPlayedCard({ match, title = 'Who played' }: { match: Match; title?: string }) {
  const rows = match.sides.slice(0, 2).map((side) => ({
    side,
    players: match.players.filter((p) => p.side_id === side.id && p.member.invitation?.status !== 'pending'),
  }))
  if (rows.every((r) => r.players.length === 0)) return null
  return (
    <section className={cn(cardClass, 'flex flex-col gap-3')}>
      <span className="text-[15px] font-bold">{title}</span>
      {rows.map(({ side, players }) => (
        <div key={side.id} className="flex items-center gap-3">
          <div className="flex">
            {players.slice(0, 5).map((p, i) => (
              <PersonAvatar
                key={p.member.id}
                name={memberName(p.member)}
                imageUrl={memberAvatarUrl(p.member)}
                size={30}
                className={cn('box-border border-2 border-card', i > 0 && '-ml-2')}
              />
            ))}
            {players.length > 5 && (
              <span className="-ml-2 box-border flex size-[30px] items-center justify-center rounded-full border-2 border-card bg-muted text-[11px] font-bold">
                +{players.length - 5}
              </span>
            )}
          </div>
          <span className="text-sm text-muted-foreground">
            {sideLabel(side, '')} · {players.length} {players.length === 1 ? 'player' : 'players'}
          </span>
        </div>
      ))}
    </section>
  )
}

/** Comments preview: newest comment + an "Add a comment…" entry, both opening the full thread. */
export function CommentsPreviewCard({
  match,
  viewerName,
  viewerAvatar,
  onOpen,
}: {
  match: Match
  viewerName?: string
  viewerAvatar?: string
  onOpen: () => void
}) {
  const query = useQuery({
    queryKey: ['comments', match.id, 'preview'],
    queryFn: async (): Promise<CommentPage> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/comments', {
        params: { path: { match_id: match.id }, query: { limit: 1 } },
      })
      if (error || !data) throw new Error('Failed to load comments')
      return data
    },
  })
  const latest: Comment | undefined = query.data?.items.find((c) => !c.deleted_at && c.author && c.text)
  const count = match.social.comment_count

  return (
    <section className={cn(cardClass, 'flex flex-col gap-3')}>
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">Comments</span>
        {count > 0 && (
          <button type="button" onClick={onOpen} className="text-sm font-bold text-primary">
            See all {count}
          </button>
        )}
      </div>
      {latest?.author && (
        <button type="button" onClick={onOpen} className="flex gap-2.5 text-left">
          <PersonAvatar name={latest.author.name} imageUrl={latest.author.profile_image?.image_url} size={32} />
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="text-sm">
              <b className="font-bold">{latest.author.name}</b>{' '}
              <span className="text-muted-foreground">· {relativeTime(latest.created_at)}</span>
            </span>
            <span className="line-clamp-3 text-sm text-ink-soft">{latest.text}</span>
          </span>
        </button>
      )}
      <button
        type="button"
        onClick={onOpen}
        className="flex h-11 items-center gap-2.5 rounded-full bg-background pr-1.5 pl-1 text-left text-sm text-muted-foreground"
      >
        <PersonAvatar name={viewerName ?? 'You'} imageUrl={viewerAvatar} size={32} />
        Add a comment…
      </button>
    </section>
  )
}

/** "Match rules" row from `Match.dc.html`: the format summary with a chevron. */
export function MatchRulesRow({ summary, onClick }: { summary: string; onClick?: () => void }) {
  const Comp = onClick ? 'button' : 'div'
  return (
    <Comp
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      className="flex min-h-14 items-center gap-3 rounded-[20px] border bg-card px-[18px] py-3 text-left"
    >
      <span className="flex flex-1 flex-col gap-0.5">
        <span className="text-[15px] font-semibold">Match rules</span>
        <span className="text-[13px] text-muted-foreground">{summary}</span>
      </span>
      {onClick && <ChevronRight className="size-5 text-muted-foreground" />}
    </Comp>
  )
}

// ---------------------------------------------------------------------------
// Timeline tab
// ---------------------------------------------------------------------------

type TimelineFilter = 'all' | 'goals' | 'cards' | 'subs'
const GOAL_KINDS: FootballEventKind[] = ['goal', 'own_goal', 'penalty']
const CARD_KINDS: FootballEventKind[] = ['yellow_card', 'red_card']

function BallIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7l4 3-1.5 4.5h-5L8 10z" />
    </svg>
  )
}

function SubIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M7 4v14M3 14l4 4 4-4M17 20V6M13 10l4-4 4 4" />
    </svg>
  )
}

function EventIcon({ kind, index }: { kind: FootballEventKind; index: number }) {
  if (GOAL_KINDS.includes(kind)) {
    return index === 0 ? (
      <span className="flex size-[30px] shrink-0 items-center justify-center rounded-full bg-primary text-white">
        <BallIcon />
      </span>
    ) : (
      <span
        className="box-border flex size-[30px] shrink-0 items-center justify-center rounded-full border-2 bg-card text-foreground"
        style={{ borderColor: KIT_GREY }}
      >
        <BallIcon />
      </span>
    )
  }
  if (CARD_KINDS.includes(kind)) {
    return (
      <span className="box-border flex size-[30px] shrink-0 items-center justify-center rounded-full border border-rule bg-card">
        <span
          className="h-[15px] w-[11px] rotate-[8deg] rounded-[2px]"
          style={{ background: kind === 'yellow_card' ? 'var(--gold)' : 'var(--destructive)' }}
        />
      </span>
    )
  }
  return (
    <span className="flex size-[30px] shrink-0 items-center justify-center rounded-full bg-chip text-ink-soft">
      <SubIcon />
    </span>
  )
}

const KIND_TITLE: Record<FootballEventKind, string> = {
  goal: 'Goal',
  own_goal: 'Own goal',
  penalty: 'Penalty',
  yellow_card: 'Yellow card',
  red_card: 'Red card',
  substitution: 'Sub',
}

type TimelineItem =
  | { type: 'event'; event: FootballEventView; key: number; running?: [number, number] }
  | { type: 'marker'; label: string; key: number; score?: [number, number] }

export function FootballTimeline({
  match,
  detail,
  currentUserId,
  finished,
}: {
  match: Match
  detail: FootballEventSource
  currentUserId?: string
  finished: boolean
}) {
  const [filter, setFilter] = useState<TimelineFilter>('all')
  const [expanded, setExpanded] = useState(false)
  const [sideA, sideB] = match.sides
  const events = eventsFromDetail(detail)
  const players: ScorePlayers = detail.players
  const pt = detail.period_times

  const myPlayerId = match.players.find((p) => p.member.type === 'User' && p.member.user_id === currentUserId)?.member.id

  // Running score after each goal, in chronological order.
  const items: TimelineItem[] = []
  let a = 0
  let b = 0
  const keyOf = (e: FootballEventView, i: number) =>
    e.occurred_at ? new Date(e.occurred_at).getTime() : e.minute !== undefined ? e.minute * 60_000 : i
  events.forEach((event, i) => {
    let running: [number, number] | undefined
    if (GOAL_KINDS.includes(event.kind)) {
      if (event.side_id === sideA?.id) a += 1
      else if (event.side_id === sideB?.id) b += 1
      running = [a, b]
    }
    items.push({ type: 'event', event, key: keyOf(event, i), running })
  })

  // Period markers, only when events are wall-clock timed (live-scored) so
  // they can be interleaved truthfully; a manual result just gets Full time.
  const timed = events.length > 0 && events.every((e) => e.occurred_at)
  const scoreAt = (t: number): [number, number] => {
    let sa = 0
    let sb = 0
    for (const e of events) {
      if (!GOAL_KINDS.includes(e.kind) || !e.occurred_at || new Date(e.occurred_at).getTime() > t) continue
      if (e.side_id === sideA?.id) sa += 1
      else if (e.side_id === sideB?.id) sb += 1
    }
    return [sa, sb]
  }
  if (timed && pt) {
    const markers: [string, string][] = [
      ['kick_off', 'Kick-off'],
      ['half_time', 'Half time'],
      ['full_time', 'Full time'],
    ]
    for (const [period, label] of markers) {
      const at = pt[period]
      if (!at) continue
      const t = new Date(at).getTime()
      items.push({ type: 'marker', label, key: t, score: period === 'kick_off' ? undefined : scoreAt(t) })
    }
  } else if (finished) {
    items.push({ type: 'marker', label: 'Full time', key: Number.MAX_SAFE_INTEGER, score: [a, b] })
  }

  const matches = (item: TimelineItem) => {
    if (item.type === 'marker') return filter === 'all'
    if (filter === 'all') return true
    if (filter === 'goals') return GOAL_KINDS.includes(item.event.kind)
    if (filter === 'cards') return CARD_KINDS.includes(item.event.kind)
    return item.event.kind === 'substitution'
  }
  const filtered = items.sort((x, y) => y.key - x.key).filter(matches)
  const PAGE = 12
  const shown = expanded ? filtered : filtered.slice(0, PAGE)
  const hidden = filtered.length - shown.length

  const available: { id: TimelineFilter; label: string }[] = [{ id: 'all', label: 'All' }]
  if (events.some((e) => GOAL_KINDS.includes(e.kind))) available.push({ id: 'goals', label: 'Goals' })
  if (events.some((e) => CARD_KINDS.includes(e.kind))) available.push({ id: 'cards', label: 'Cards' })
  if (events.some((e) => e.kind === 'substitution')) available.push({ id: 'subs', label: 'Subs' })

  const nameOf = (id?: string) => playerInfoFor(match, id, players)?.name

  if (events.length === 0) {
    return (
      <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
        No events recorded for this match yet.
      </section>
    )
  }

  return (
    <>
      <div role="group" aria-label="Filter timeline" className="flex flex-wrap items-center gap-2">
        {available.map((f) => (
          <button
            key={f.id}
            type="button"
            aria-pressed={filter === f.id}
            onClick={() => setFilter(f.id)}
            className={cn(
              'box-border h-9 rounded-full border px-3.5 text-sm font-bold',
              filter === f.id ? 'border-foreground bg-foreground text-background' : 'border-edge bg-card text-foreground',
            )}
          >
            {f.label}
          </button>
        ))}
        <span className="ml-auto text-[13px] text-muted-foreground">Newest first</span>
      </div>

      <section aria-label="Match timeline" className="flex flex-col rounded-[20px] border bg-card px-1.5 py-2.5">
        {shown.map((item, i) => {
          const first = i === 0
          const last = i === shown.length - 1 && hidden === 0
          if (item.type === 'marker') {
            return (
              <div key={`m${i}`} className="grid grid-cols-[44px_32px_minmax(0,1fr)] gap-x-2.5 pr-2.5 pl-1.5">
                <span />
                <div className="flex flex-col items-center">
                  <span className={cn('h-2.5 w-0.5', first ? 'bg-transparent' : 'bg-rule')} />
                  <span className="size-3 rounded-full bg-foreground" />
                  <span className={cn('h-2.5 w-0.5', last ? 'bg-transparent' : 'bg-rule')} />
                </div>
                <div className="flex items-center">
                  <span className="flex h-7 items-center rounded-full bg-foreground px-3 text-[13px] font-bold text-background">
                    {item.label}
                    {item.score && (
                      <span className="ml-1 font-medium text-ink-ghost">{`· ${item.score[0]}–${item.score[1]}`}</span>
                    )}
                  </span>
                </div>
              </div>
            )
          }
          const { event, running } = item
          const idx = sideIndex(match, event.side_id)
          const isGoal = GOAL_KINDS.includes(event.kind)
          const scorerName = event.kind === 'own_goal' ? nameOf(event.player_id) : nameOf(event.player_id)
          const mine = !!myPlayerId && isGoal && event.kind !== 'own_goal' && event.player_id === myPlayerId
          const title = `${KIND_TITLE[event.kind]}${scorerName ? ` · ${scorerName}` : isGoal ? ' · Unnamed player' : ''}${mine ? ' · you' : ''}`
          let sub: string | undefined
          if (isGoal && event.assist_player_id) sub = `Assist ${nameOf(event.assist_player_id) ?? 'unknown'}`
          if (event.kind === 'substitution' && event.substituted_player_id) sub = `Off: ${nameOf(event.substituted_player_id) ?? 'unknown'}`
          return (
            <div
              key={`e${i}`}
              className={cn(
                'grid grid-cols-[44px_32px_minmax(0,1fr)_auto] items-stretch gap-x-2.5 rounded-[14px] pr-2.5 pl-1.5',
                mine && 'bg-accent',
              )}
            >
              <div className="flex flex-col items-end justify-center py-3">
                <span className="text-sm font-bold">{eventClockLabel(event, pt)}</span>
              </div>
              <div className="flex flex-col items-center">
                <span className={cn('w-0.5 flex-1', first ? 'bg-transparent' : 'bg-rule')} />
                <EventIcon kind={event.kind} index={idx} />
                <span className={cn('w-0.5 flex-1', last ? 'bg-transparent' : 'bg-rule')} />
              </div>
              <div className="flex min-w-0 flex-col justify-center gap-px py-3">
                <span className="flex items-baseline text-[15px] leading-snug font-semibold">
                  <span className="mr-1.5 inline-flex shrink-0 -translate-y-px">
                    <SideSwatch index={idx} size={8} />
                  </span>
                  <span className="min-w-0">{title}</span>
                </span>
                {sub && <span className="truncate text-[13px] text-muted-foreground">{sub}</span>}
              </div>
              <div className="flex items-center">
                {running && (
                  <span className="font-display text-base font-extrabold whitespace-nowrap">
                    {running[0]}–{running[1]}
                  </span>
                )}
              </div>
            </div>
          )
        })}
      </section>
      {hidden > 0 && (
        <>
          <div className="-mt-3.5 grid grid-cols-[44px_32px_minmax(0,1fr)] gap-x-2.5 pr-2.5 pl-3">
            <span />
            <div className="flex justify-center">
              <span className="h-5 w-0.5 bg-[linear-gradient(#E4E0D7_50%,transparent_50%)] bg-[length:2px_6px]" />
            </div>
            <span />
          </div>
          <button
            type="button"
            onClick={() => setExpanded(true)}
            className="-mt-2.5 h-12 rounded-[14px] border border-edge bg-card text-sm font-bold"
          >
            Show earlier · {hidden} more
          </button>
        </>
      )}
    </>
  )
}

// ---------------------------------------------------------------------------
// Players tab
// ---------------------------------------------------------------------------

function statsLine(goals: number, assists: number): string | null {
  const parts: string[] = []
  if (goals > 0) parts.push(`${goals} ${goals === 1 ? 'goal' : 'goals'}`)
  if (assists > 0) parts.push(`${assists} ${assists === 1 ? 'assist' : 'assists'}`)
  return parts.length ? parts.join(' · ') : null
}

export function FootballPlayersTab({
  match,
  detail,
  currentUserId,
  goalsA,
  goalsB,
  finished,
  footer,
  layout = 'stack',
}: {
  match: Match
  detail: FootballEventSource | null
  currentUserId?: string
  goalsA: number
  goalsB: number
  finished: boolean
  footer?: React.ReactNode
  /** `columns` (desktop) puts the sides side by side and leaves the top
   *  performer out; `top` renders only the top performer card. */
  layout?: PlayersLayout
}) {
  const following = useViewerFollowing(currentUserId)
  const stats = useMemo(() => {
    const map = new Map<string, { goals: number; assists: number }>()
    if (detail) {
      for (const r of goalContributions(detail.goals, match, detail.players)) map.set(r.key, { goals: r.goals, assists: r.assists })
    }
    return map
  }, [detail, match])

  const cardsByPlayer = useMemo(() => {
    const map = new Map<string, ('yellow' | 'red')[]>()
    for (const c of detail?.cards ?? []) map.set(c.player_id, [...(map.get(c.player_id) ?? []), c.color])
    return map
  }, [detail])

  const unnamedBySide = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const g of detail?.goals ?? ([] as FootballGoalEvent[])) {
      if (!g.own_goal && !g.scorer_player_id) counts[g.side_id] = (counts[g.side_id] ?? 0) + 1
    }
    return counts
  }, [detail])

  // Top performer: most goal involvements across both sides.
  let top: { player: MatchPlayer; total: number } | null = null
  for (const p of match.players) {
    const s = stats.get(p.member.id)
    const total = (s?.goals ?? 0) + (s?.assists ?? 0)
    if (total > 0 && (!top || total > top.total)) top = { player: p, total }
  }
  const topSide = top ? match.sides.find((s) => s.id === top!.player.side_id) : undefined

  const result = (idx: number) => {
    if (!finished) return null
    if (goalsA === goalsB) return 'Drew'
    return (idx === 0) === goalsA > goalsB ? 'Won' : 'Lost'
  }

  const unassigned = match.players.filter((p) => !match.sides.some((s) => s.id === p.side_id))

  const renderRow = (p: MatchPlayer, i: number) => {
    const s = stats.get(p.member.id)
    const cards = cardsByPlayer.get(p.member.id) ?? []
    return (
      <PlayerRow
        key={p.member.id}
        player={p}
        first={i === 0}
        currentUserId={currentUserId}
        followingIds={following.data}
        line={statsLine(s?.goals ?? 0, s?.assists ?? 0)}
        badges={cards.map((color, ci) => (
          <span
            key={ci}
            aria-label={color === 'yellow' ? 'Yellow card' : 'Red card'}
            className="ml-1.5 inline-block h-3 w-[9px] rounded-[2px] align-[-1px]"
            style={{ background: color === 'yellow' ? 'var(--gold)' : 'var(--destructive)' }}
          />
        ))}
      />
    )
  }

  const topCard = top ? (
    <TopPerformerCard
      player={top.player}
      detail={`${top.total} goal ${top.total === 1 ? 'involvement' : 'involvements'}${topSide ? ` for ${sideLabel(topSide, '')}` : ''}`}
      value={top.total}
    />
  ) : null
  if (layout === 'top') return topCard

  return (
    <>
      {layout === 'stack' && topCard}

      <SidesGrid columns={layout === 'columns'}>
        {match.sides.slice(0, 2).map((side, idx) => {
          const players = match.players.filter((p) => p.side_id === side.id)
          const unnamed = unnamedBySide[side.id] ?? 0
          const res = result(idx)
          return (
            <div key={side.id} className="flex flex-col gap-3.5">
              <SideHeading
                index={idx}
                name={sideLabel(side, idx === 0 ? 'Side A' : 'Side B')}
                meta={`${res ? `${res} · ` : ''}${players.length} ${players.length === 1 ? 'player' : 'players'}`}
              />
              <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
                {players.length === 0 && unnamed === 0 && (
                  <p className="px-4 py-5 text-sm text-muted-foreground">No players yet.</p>
                )}
                {players.map(renderRow)}
                {unnamed > 0 && (
                  <div className={cn('flex min-h-16 items-center gap-3 px-4 py-2.5', players.length > 0 && 'border-t border-hairline')}>
                    <span
                      className="box-border flex size-10 shrink-0 items-center justify-center rounded-full border-[1.5px] border-dashed text-base font-bold text-muted-foreground"
                      style={{ borderColor: KIT_GREY }}
                    >
                      ?
                    </span>
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="text-[15px] font-semibold">Unnamed player</span>
                      <span className="text-[13px] text-muted-foreground">
                        {unnamed} {unnamed === 1 ? 'goal' : 'goals'}
                      </span>
                    </div>
                  </div>
                )}
              </section>
            </div>
          )
        })}
      </SidesGrid>

      {unassigned.length > 0 && (
        <div className="flex flex-col gap-3.5">
          <div className="mt-1.5 flex items-center gap-2.5 px-1">
            <span className="flex-1 font-display text-[19px] font-bold">Not on a side yet</span>
            <span className="text-[13px] text-muted-foreground">
              {unassigned.length} {unassigned.length === 1 ? 'player' : 'players'}
            </span>
          </div>
          <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">{unassigned.map(renderRow)}</section>
        </div>
      )}

      {Object.keys(unnamedBySide).length > 0 && (
        <span className="px-1 text-[13px] text-muted-foreground">
          Unnamed players are goals nobody claimed. The organiser can name the scorer by editing the result.
        </span>
      )}
      {footer}
    </>
  )
}

export type PlayersLayout = 'stack' | 'columns' | 'top'

/** Wraps a Players tab's per-side lists: stacked on phones, side by side on
 *  the desktop board. */
export function SidesGrid({ columns, children }: { columns: boolean; children: React.ReactNode }) {
  return columns ? <div className="grid grid-cols-2 items-start gap-6">{children}</div> : <>{children}</>
}

/** One roster row: avatar, name (+ badges and a You/organiser tag), a stat
 *  line and a Follow button. Shared by every sport's Players tab. */
export function PlayerRow({
  player,
  first,
  currentUserId,
  followingIds,
  line,
  badges,
}: {
  player: MatchPlayer
  first: boolean
  currentUserId?: string
  followingIds?: Set<string>
  line?: string | null
  badges?: React.ReactNode
}) {
  const name = memberName(player.member)
  const isMe = player.member.type === 'User' && player.member.user_id === currentUserId
  const pending = player.member.invitation?.status === 'pending'
  const roleTag = [isMe ? 'You' : null, player.role === 'owner' ? 'organiser' : player.role === 'admin' ? 'admin' : null]
    .filter(Boolean)
    .join(' · ')
  const userId = player.member.type === 'User' ? player.member.user_id : undefined
  const subline = pending ? 'Invited' : line
  return (
    <div className={cn('flex min-h-16 items-center gap-3 px-4 py-2.5', !first && 'border-t border-hairline')}>
      {userId ? (
        <Link to={`/users/${userId}`} className="shrink-0">
          <PersonAvatar name={name} imageUrl={memberAvatarUrl(player.member)} size={40} />
        </Link>
      ) : (
        <PersonAvatar name={name} imageUrl={memberAvatarUrl(player.member)} size={40} />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[15px] font-semibold">
          {userId ? (
            <Link to={`/users/${userId}`} className="text-foreground hover:underline">
              {name}
            </Link>
          ) : (
            name
          )}
          {badges}
          {roleTag && (
            <span className="ml-1.5 inline-flex h-5 items-center rounded-full bg-muted px-[7px] align-[1px] text-[11px] font-bold text-ink-soft">
              {roleTag.charAt(0).toUpperCase() + roleTag.slice(1)}
            </span>
          )}
        </span>
        {subline && <span className="text-[13px] text-muted-foreground">{subline}</span>}
      </div>
      {userId && !isMe && followingIds && (
        <FollowButton
          userId={userId}
          isFollowing={followingIds.has(userId)}
          tone="soft"
          className="h-9 rounded-full px-3.5 text-[13px] font-bold"
        />
      )}
    </div>
  )
}

/** The dark "Top performer" banner at the top of a Players tab. */
export function TopPerformerCard({ player, detail, value }: { player: MatchPlayer; detail: string; value: React.ReactNode }) {
  return (
    <section className="flex items-center gap-3.5 rounded-[20px] bg-spotlight px-[18px] py-4 text-spotlight-foreground">
      <PersonAvatar
        name={memberName(player.member)}
        imageUrl={memberAvatarUrl(player.member)}
        size={48}
        className="shadow-[0_0_0_3px_var(--gold)]"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-xs font-bold tracking-[0.5px] text-gold">TOP PERFORMER</span>
        <span className="truncate text-[17px] font-bold">{memberName(player.member)}</span>
        <span className="text-[13px] text-spotlight-muted">{detail}</span>
      </div>
      <span className="font-display text-[32px] font-extrabold">{value}</span>
    </section>
  )
}

/** Kit swatch + side name + a short meta line, above a side's roster card. */
export function SideHeading({ index, name, meta }: { index: number; name: string; meta?: string }) {
  return (
    <div className="mt-1.5 flex items-center gap-2.5 px-1">
      <SideSwatch index={index} size={14} />
      <span className="flex-1 truncate font-display text-[19px] font-bold">{name}</span>
      {meta && <span className="text-[13px] text-muted-foreground">{meta}</span>}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Bottom action bar
// ---------------------------------------------------------------------------

export function MatchActionBar({
  primary,
  commentCount,
  onComments,
  onShare,
}: {
  primary: React.ReactNode
  commentCount: number
  onComments: () => void
  onShare: () => void
}) {
  return (
    <div className="sticky bottom-[72px] z-10 -mx-4 mt-2 flex gap-2.5 border-t bg-card px-4 py-3 md:bottom-0 md:mx-0 md:rounded-2xl md:border">
      {primary}
      <button
        type="button"
        aria-label={`Comments, ${commentCount}`}
        onClick={onComments}
        className="relative box-border flex size-[52px] shrink-0 items-center justify-center rounded-2xl border border-edge bg-card"
      >
        <MessageCircle className="size-[22px]" />
        {commentCount > 0 && (
          <span className="absolute -top-1.5 -right-1.5 box-border flex h-5 min-w-5 items-center justify-center rounded-full bg-foreground px-[5px] text-[11px] font-bold text-background">
            {commentCount}
          </span>
        )}
      </button>
      <button
        type="button"
        aria-label="Share"
        onClick={onShare}
        className="box-border flex size-[52px] shrink-0 items-center justify-center rounded-2xl border border-edge bg-card"
      >
        <Share className="size-[22px]" />
      </button>
    </div>
  )
}

export const primaryActionClass =
  'flex h-[52px] flex-1 items-center justify-center gap-2 rounded-2xl bg-primary text-base font-bold text-primary-foreground transition-opacity hover:opacity-90'

export function KudosButton({ liked, onToggle }: { liked: boolean; onToggle: () => void }) {
  return (
    <button type="button" aria-pressed={liked} onClick={onToggle} className={primaryActionClass}>
      <Flame className={cn('size-5', liked && 'fill-current')} />
      {liked ? 'Kudos given' : 'Give kudos'}
    </button>
  )
}

export function AddEventButton({ to }: { to: string }) {
  return (
    <Link
      to={to}
      className="flex h-[52px] flex-1 items-center justify-center gap-2 rounded-2xl border border-edge bg-card text-base font-bold text-foreground"
    >
      <Plus className="size-5" strokeWidth={2.5} />
      Add event
    </Link>
  )
}
