import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { memberName } from '@/lib/members'
import type { NetballFormat } from '@/lib/matchFormat'
import {
  eventClockLabel,
  eventsFromDetail,
  foulKindLabel,
  type NetballEventSource,
  type NetballEventView,
  type NetballPeriod,
  type NetballScore,
} from '@/lib/netballScore'
import {
  KIT_GREY,
  PlayerRow,
  SideHeading,
  SideSwatch,
  TopPerformerCard,
  cardClass,
  SidesGrid,
  type PlayersLayout,
} from '@/components/agon/football/FootballMatchView'
import { useViewerFollowing } from '@/hooks/useViewerFollowing'

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']
type NetballPosition = components['schemas']['NetballPosition']

const POSITION_SHORT: Record<NetballPosition, string> = {
  goal_shooter: 'GS',
  goal_attack: 'GA',
  wing_attack: 'WA',
  centre: 'C',
  wing_defence: 'WD',
  goal_defence: 'GD',
  goal_keeper: 'GK',
}

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`
}

function sideIndex(match: Match, sideId: string | undefined): number {
  return Math.max(
    0,
    match.sides.findIndex((s) => s.id === sideId),
  )
}

function sideLabel(match: Match, sideId: string | undefined, fallback = 'This side'): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || fallback
}

function nameFor(match: Match, id: string | undefined, players?: NetballEventSource['players']): string | null {
  if (!id) return null
  const resolved = players?.[id]
  if (resolved) return resolved.name
  const p = match.players.find((pl) => pl.member.id === id)
  return p ? memberName(p.member) : null
}

/** Points a goal is worth. */
const goalPoints = (e: { kind: NetballEventView['kind'] }) => (e.kind === 'two_point_goal' ? 2 : e.kind === 'goal' ? 1 : 0)

// ---------------------------------------------------------------------------
// Quarter scores
// ---------------------------------------------------------------------------

/** Quarter-end markers in play order, with their column labels. */
const QUARTER_ENDS: { period: NetballPeriod; label: string }[] = [
  { period: 'quarter_one_end', label: 'Q1' },
  { period: 'quarter_two_end', label: 'Q2' },
  { period: 'quarter_three_end', label: 'Q3' },
  { period: 'full_time', label: 'Q4' },
  { period: 'extra_time_end', label: 'ET' },
]

/** Cumulative score at each recorded quarter end, plus the running total now. */
function quarterTotals(match: Match, score: NetballScore) {
  const [a, b] = match.sides
  const ends = QUARTER_ENDS.map((q) => ({ ...q, entry: score.period_scores?.[q.period] })).filter(
    (q): q is typeof q & { entry: Record<string, number> } => !!q.entry,
  )
  return ends.map((q) => ({ label: q.label, a: q.entry[a?.id ?? ''] ?? 0, b: q.entry[b?.id ?? ''] ?? 0 }))
}

/** "Quarter by quarter": goals each side scored per quarter, as a small table. */
export function QuarterScoresCard({ match, score, live }: { match: Match; score: NetballScore; live: boolean }) {
  const [a, b] = match.sides
  const ends = quarterTotals(match, score)
  // While live, the quarter in play gets a column too, from the running score.
  const nowA = score.score[a?.id ?? ''] ?? 0
  const nowB = score.score[b?.id ?? ''] ?? 0
  const lastEnd = ends.at(-1)
  const cols = [...ends]
  if (live && (!lastEnd || lastEnd.a !== nowA || lastEnd.b !== nowB || ends.length < 4)) {
    const label = ends.length < 4 ? `Q${ends.length + 1}` : 'ET'
    if (!cols.some((c) => c.label === label)) cols.push({ label, a: nowA, b: nowB })
  }
  if (cols.length === 0) return null
  const perQuarter = cols.map((c, i) => ({
    label: c.label,
    a: c.a - (i ? cols[i - 1].a : 0),
    b: c.b - (i ? cols[i - 1].b : 0),
    current: live && i === cols.length - 1 && c.label !== lastEnd?.label,
  }))
  const grid = { gridTemplateColumns: `minmax(0,1fr) repeat(${perQuarter.length}, 34px) 40px` }
  const totalA = cols.at(-1)!.a
  const totalB = cols.at(-1)!.b

  const row = (side: typeof a, idx: number, pick: 'a' | 'b', total: number, otherTotal: number) => (
    <div style={grid} className="grid min-h-10 items-center gap-x-1 border-t border-hairline text-sm tabular-nums">
      <span className="flex min-w-0 items-center gap-2 font-semibold">
        <SideSwatch index={idx} size={10} />
        <span className="truncate">{sideLabel(match, side?.id, idx === 0 ? 'Side A' : 'Side B')}</span>
      </span>
      {perQuarter.map((q) => (
        <span key={q.label} className={cn('text-center', q.current ? 'font-bold text-destructive' : 'text-ink-soft')}>
          {q[pick]}
        </span>
      ))}
      <span className={cn('text-right font-display text-lg font-extrabold', total < otherTotal && 'text-ink-faint')}>{total}</span>
    </div>
  )

  return (
    <section className={cn(cardClass, 'flex flex-col pb-2')}>
      <span className="pb-2.5 text-[15px] font-bold">Quarter by quarter</span>
      <div style={grid} className="grid items-center gap-x-1 pb-2 text-[11px] font-bold tracking-[0.4px] text-muted-foreground">
        <span />
        {perQuarter.map((q) => (
          <span key={q.label} className={cn('text-center', q.current && 'text-destructive')}>
            {q.label}
          </span>
        ))}
        <span className="text-right">T</span>
      </div>
      {row(a, 0, 'a', totalA, totalB)}
      {row(b, 1, 'b', totalB, totalA)}
    </section>
  )
}

/** "How it unfolded": each side's running total at every quarter end, so the
 *  momentum swings read at a glance. Works for both scoring methods, since
 *  both record quarter scores. */
export function NetballScoreFlowCard({ match, score, format, live }: { match: Match; score: NetballScore; format: NetballFormat; live: boolean }) {
  const [a, b] = match.sides
  const ends = quarterTotals(match, score)
  const pts = [{ label: '', a: 0, b: 0 }, ...ends]
  const nowA = score.score[a?.id ?? ''] ?? 0
  const nowB = score.score[b?.id ?? ''] ?? 0
  const liveTail = live && (ends.at(-1)?.a !== nowA || ends.at(-1)?.b !== nowB)
  if (liveTail) pts.push({ label: 'Now', a: nowA, b: nowB })
  if (pts.length < 2 || !a || !b) return null

  const quarters = Math.max(format.num_quarters, ends.length, liveTail ? ends.length + 1 : 0)
  const x0 = 30
  const x1 = 314
  const yBase = 162
  const yTop = 12
  const max = Math.max(...pts.map((p) => Math.max(p.a, p.b)), 1)
  const step = [2, 5, 10, 20, 25, 50].find((s) => Math.ceil(max / s) <= 4) ?? 100
  const yMax = step * Math.max(Math.ceil((max * 1.05) / step), 1)
  // A live partial quarter sits half way through its column.
  const xFor = (i: number) => x0 + ((i === pts.length - 1 && liveTail ? i - 0.5 : i) / quarters) * (x1 - x0)
  const yFor = (v: number) => yBase - (v / yMax) * (yBase - yTop)
  const line = (k: 'a' | 'b') => pts.map((p, i) => `${i ? 'L' : 'M'}${xFor(i).toFixed(1)} ${yFor(p[k]).toFixed(1)}`).join(' ')
  const ticks: number[] = []
  for (let v = 0; v <= yMax; v += step) ticks.push(v)
  const end = pts.length - 1
  const endX = xFor(end)
  const ya = yFor(pts[end].a)
  const yb = yFor(pts[end].b)
  const nameA = sideLabel(match, a.id, 'Side A')
  const nameB = sideLabel(match, b.id, 'Side B')

  return (
    <section className={cn(cardClass, 'flex flex-col gap-3')}>
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">How it unfolded</span>
        {ends.length >= 2 && (
          <span className="text-[13px] text-muted-foreground">
            HT {ends[1].a}–{ends[1].b}
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
      <svg width="100%" viewBox="0 0 322 190" role="img" aria-label={`Score by quarter: ${nameA} ${nowA}, ${nameB} ${nowB}`} className="block font-sans">
        {ticks.map((v) => (
          <g key={v}>
            <line x1={x0} x2={x1} y1={yFor(v)} y2={yFor(v)} stroke="var(--gridline)" strokeWidth="1" />
            <text x={x0 - 8} y={yFor(v) + 4} textAnchor="end" fontSize="11" className="fill-muted-foreground">
              {v}
            </text>
          </g>
        ))}
        {Array.from({ length: quarters }, (_, i) => (
          <text key={i} x={x0 + ((i + 0.5) / quarters) * (x1 - x0)} y="180" textAnchor="middle" fontSize="11" className="fill-muted-foreground">
            {i < 4 ? `Q${i + 1}` : 'ET'}
          </text>
        ))}
        {Array.from({ length: quarters - 1 }, (_, i) => (
          <line key={i} x1={x0 + ((i + 1) / quarters) * (x1 - x0)} x2={x0 + ((i + 1) / quarters) * (x1 - x0)} y1={yTop} y2={yBase} stroke="var(--gridline)" strokeWidth="1" strokeDasharray={i === 1 ? '3 4' : undefined} />
        ))}
        <path d={line('b')} fill="none" stroke={KIT_GREY} strokeWidth="2.5" strokeLinejoin="round" strokeLinecap="round" />
        <path d={line('a')} fill="none" className="stroke-primary" strokeWidth="2.5" strokeLinejoin="round" strokeLinecap="round" />
        {pts.slice(1).map((p, i) => (
          <g key={i}>
            <circle cx={xFor(i + 1)} cy={yFor(p.b)} r="3.5" fill={KIT_GREY} stroke="var(--card)" strokeWidth="1.5" />
            <circle cx={xFor(i + 1)} cy={yFor(p.a)} r="3.5" className="fill-primary" stroke="var(--card)" strokeWidth="1.5" />
          </g>
        ))}
        <text x={endX - 8} y={ya - 9} textAnchor="end" fontSize="13" fontWeight="700" className="fill-primary">
          {pts[end].a}
        </text>
        <text x={endX - 8} y={Math.abs(ya - yb) < 16 ? yb + 20 : yb - 9} textAnchor="end" fontSize="13" fontWeight="700" className="fill-muted-foreground">
          {pts[end].b}
        </text>
      </svg>
    </section>
  )
}

// ---------------------------------------------------------------------------
// Top scorers
// ---------------------------------------------------------------------------

interface ScorerRow {
  key: string
  sideId: string
  name: string
  userId?: string
  goals: number
  points: number
  position?: NetballPosition
}

function scorerRows(match: Match, detail: NetballEventSource): ScorerRow[] {
  const map = new Map<string, ScorerRow>()
  for (const g of detail.goals) {
    const key = g.scorer_player_id ?? `unnamed-${g.side_id}`
    const player = match.players.find((p) => p.member.id === g.scorer_player_id)
    const row = map.get(key) ?? {
      key,
      sideId: g.side_id,
      name: nameFor(match, g.scorer_player_id, detail.players) ?? 'Unnamed player',
      userId: player?.member.type === 'User' ? player.member.user_id : undefined,
      goals: 0,
      points: 0,
    }
    row.goals += 1
    row.points += g.two_points ? 2 : 1
    row.position ??= g.scorer_position
    map.set(key, row)
  }
  return [...map.values()].sort((x, y) => y.points - x.points || y.goals - x.goals)
}

/** "Shooting": each scorer's goals as a bar, their court position beside them. */
export function TopScorersCard({ match, detail }: { match: Match; detail: NetballEventSource }) {
  const [showAll, setShowAll] = useState(false)
  const rows = useMemo(() => scorerRows(match, detail), [match, detail])
  if (rows.length === 0) return null
  const hasTwo = detail.goals.some((g) => g.two_points)
  const max = Math.max(...rows.map((r) => r.points)) + 1
  const visible = showAll ? rows : rows.slice(0, 6)
  const grid = 'grid grid-cols-[124px_minmax(0,1fr)_28px_26px] items-center gap-2'
  return (
    <section className={cn(cardClass, 'flex flex-col gap-2.5')}>
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">Shooting</span>
        <span className="text-xs text-muted-foreground">{hasTwo ? 'Points, 2-pointers count double' : 'Goals'}</span>
      </div>
      {visible.map((r) => (
        <div key={r.key} className={cn(grid, 'min-h-[30px]')}>
          <span className="flex items-center gap-2 overflow-hidden text-sm font-semibold whitespace-nowrap">
            <SideSwatch index={sideIndex(match, r.sideId)} size={10} />
            {r.userId ? (
              <Link to={`/users/${r.userId}`} className="truncate text-foreground hover:underline">
                {r.name}
              </Link>
            ) : (
              <span className="truncate">{r.name}</span>
            )}
          </span>
          <span aria-label={plural(r.points, hasTwo ? 'point' : 'goal')} className="flex h-2">
            <span className={cn('rounded', sideIndex(match, r.sideId) === 0 && 'bg-primary')} style={{ width: `${(r.points / max) * 100}%`, ...(sideIndex(match, r.sideId) === 0 ? {} : { background: KIT_GREY }) }} />
          </span>
          <span className="text-center text-[11px] font-bold text-muted-foreground">{r.position ? POSITION_SHORT[r.position] : ''}</span>
          <span className="text-right font-display text-base font-extrabold">{r.points}</span>
        </div>
      ))}
      {rows.length > 6 && (
        <button type="button" onClick={() => setShowAll((v) => !v)} className="mt-0.5 h-11 border-t border-hairline text-sm font-bold text-primary">
          {showAll ? 'Show fewer' : 'See everyone'}
        </button>
      )}
    </section>
  )
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

type TimelineFilter = 'all' | 'goals' | 'fouls'

function NetIcon({ index, two }: { index: number; two: boolean }) {
  const icon = (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M3.5 9.5c5 1 12 1 17 0M12 3c-2.5 5-2.5 13 0 18" />
    </svg>
  )
  const label = two ? <span className="text-[11px] font-extrabold">2</span> : icon
  return index === 0 ? (
    <span className="flex size-[30px] shrink-0 items-center justify-center rounded-full bg-primary text-white">{label}</span>
  ) : (
    <span className="box-border flex size-[30px] shrink-0 items-center justify-center rounded-full border-2 bg-card text-foreground" style={{ borderColor: KIT_GREY }}>
      {label}
    </span>
  )
}

function FoulIcon() {
  return (
    <span className="flex size-[30px] shrink-0 items-center justify-center rounded-full bg-chip text-ink-soft">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
        <path d="M6 6l12 12M18 6L6 18" />
      </svg>
    </span>
  )
}

const MARKERS: { period: NetballPeriod; label: string }[] = [
  { period: 'start', label: 'Centre pass' },
  { period: 'quarter_one_end', label: 'End of Q1' },
  { period: 'quarter_two_end', label: 'Half time' },
  { period: 'quarter_three_end', label: 'End of Q3' },
  { period: 'full_time', label: 'Full time' },
  { period: 'extra_time_end', label: 'End of extra time' },
]

type Item =
  | { type: 'event'; event: NetballEventView; key: number; running?: [number, number] }
  | { type: 'marker'; label: string; key: number; score?: [number, number] }

/** Goals and fouls, newest first, with quarter markers — the football
 *  timeline's layout (`EventsFootball.dc.html`) with netball's events. */
export function NetballTimeline({
  match,
  detail,
  currentUserId,
  finished,
}: {
  match: Match
  detail: NetballEventSource
  currentUserId?: string
  finished: boolean
}) {
  const [filter, setFilter] = useState<TimelineFilter>('all')
  const [expanded, setExpanded] = useState(false)
  const [sideA, sideB] = match.sides
  const events = eventsFromDetail(detail)
  const pt = detail.period_times
  const myPlayerId = match.players.find((p) => p.member.type === 'User' && p.member.user_id === currentUserId)?.member.id

  const items: Item[] = []
  let a = 0
  let b = 0
  const keyOf = (e: NetballEventView, i: number) =>
    e.occurred_at ? new Date(e.occurred_at).getTime() : e.minute !== undefined ? e.minute * 60_000 + i : i
  events.forEach((event, i) => {
    let running: [number, number] | undefined
    const pts = goalPoints(event)
    if (pts) {
      if (event.side_id === sideA?.id) a += pts
      else if (event.side_id === sideB?.id) b += pts
      running = [a, b]
    }
    items.push({ type: 'event', event, key: keyOf(event, i), running })
  })

  const timed = events.length > 0 && events.every((e) => e.occurred_at)
  const scoreAt = (t: number): [number, number] => {
    let sa = 0
    let sb = 0
    for (const e of events) {
      if (!e.occurred_at || new Date(e.occurred_at).getTime() > t) continue
      if (e.side_id === sideA?.id) sa += goalPoints(e)
      else if (e.side_id === sideB?.id) sb += goalPoints(e)
    }
    return [sa, sb]
  }
  if (timed && pt) {
    for (const { period, label } of MARKERS) {
      const at = pt[period]
      if (!at) continue
      const t = new Date(at).getTime()
      items.push({ type: 'marker', label, key: t + (period === 'start' ? -1 : 1), score: period === 'start' ? undefined : scoreAt(t) })
    }
  } else if (finished) {
    items.push({ type: 'marker', label: 'Full time', key: Number.MAX_SAFE_INTEGER, score: [a, b] })
  }

  // Which quarter an event fell in, for the small label under its clock.
  const quarterOf = (at: string | undefined) => {
    if (!at || !pt) return null
    const t = new Date(at).getTime()
    const starts = ['start', 'quarter_two_start', 'quarter_three_start', 'quarter_four_start', 'extra_time_start']
    let label: string | null = null
    starts.forEach((k, i) => {
      if (pt[k] && new Date(pt[k]).getTime() <= t) label = i < 4 ? `Q${i + 1}` : 'ET'
    })
    return label
  }

  const matches = (item: Item) => {
    if (item.type === 'marker') return filter === 'all'
    if (filter === 'all') return true
    if (filter === 'goals') return item.event.kind !== 'foul'
    return item.event.kind === 'foul'
  }
  const filtered = items.sort((x, y) => y.key - x.key).filter(matches)
  const PAGE = 12
  const shown = expanded ? filtered : filtered.slice(0, PAGE)
  const hidden = filtered.length - shown.length

  const available: { id: TimelineFilter; label: string }[] = [{ id: 'all', label: 'All' }]
  if (events.some((e) => e.kind !== 'foul')) available.push({ id: 'goals', label: 'Goals' })
  if (events.some((e) => e.kind === 'foul')) available.push({ id: 'fouls', label: 'Fouls' })

  if (events.length === 0) {
    return (
      <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
        This match was scored by quarter, so there are no goal-by-goal events.
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
                    {item.score && <span className="ml-1 font-medium text-ink-ghost">{`· ${item.score[0]}–${item.score[1]}`}</span>}
                  </span>
                </div>
              </div>
            )
          }
          const { event, running } = item
          const idx = sideIndex(match, event.side_id)
          const isGoal = event.kind !== 'foul'
          const who = nameFor(match, event.player_id, detail.players)
          const mine = !!myPlayerId && isGoal && event.player_id === myPlayerId
          const kind = event.kind === 'two_point_goal' ? '2-point goal' : event.kind === 'goal' ? 'Goal' : 'Foul'
          const title = `${kind}${who ? ` · ${who}` : isGoal ? ' · Unnamed player' : ''}${mine ? ' · you' : ''}`
          const sub = event.kind === 'foul' && event.foul_kind ? foulKindLabel(event.foul_kind) : undefined
          return (
            <div
              key={`e${i}`}
              className={cn('grid grid-cols-[44px_32px_minmax(0,1fr)_auto] items-stretch gap-x-2.5 rounded-[14px] pr-2.5 pl-1.5', mine && 'bg-accent')}
            >
              <div className="flex flex-col items-end justify-center py-3">
                <span className="text-sm font-bold tabular-nums">{eventClockLabel(event, pt)}</span>
                {quarterOf(event.occurred_at) && (
                  <span className="text-[11px] text-muted-foreground">{quarterOf(event.occurred_at)}</span>
                )}
              </div>
              <div className="flex flex-col items-center">
                <span className={cn('w-0.5 flex-1', first ? 'bg-transparent' : 'bg-rule')} />
                {isGoal ? <NetIcon index={idx} two={event.kind === 'two_point_goal'} /> : <FoulIcon />}
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
        <button type="button" onClick={() => setExpanded(true)} className="h-12 rounded-[14px] border border-edge bg-card text-sm font-bold">
          Show earlier · {hidden} more
        </button>
      )}
    </>
  )
}

// ---------------------------------------------------------------------------
// Players tab
// ---------------------------------------------------------------------------

export function NetballPlayersTab({
  match,
  detail,
  currentUserId,
  scoreA,
  scoreB,
  finished,
  footer,
  layout = 'stack',
}: {
  match: Match
  detail: NetballEventSource | null
  currentUserId?: string
  scoreA: number
  scoreB: number
  finished: boolean
  footer?: React.ReactNode
  /** `columns` (desktop) puts the sides side by side and leaves the top
   *  performer out; `top` renders only the top performer card. */
  layout?: PlayersLayout
}) {
  const following = useViewerFollowing(currentUserId)
  const scorers = useMemo(() => (detail ? scorerRows(match, detail) : []), [match, detail])
  const byPlayer = new Map(scorers.map((r) => [r.key, r]))
  const fouls = new Map<string, number>()
  for (const f of detail?.fouls ?? []) if (f.player_id) fouls.set(f.player_id, (fouls.get(f.player_id) ?? 0) + 1)

  const top = scorers.find((r) => !r.key.startsWith('unnamed-'))
  const topPlayer: MatchPlayer | undefined = top ? match.players.find((p) => p.member.id === top.key) : undefined

  const result = (idx: number) => {
    if (!finished) return null
    if (scoreA === scoreB) return 'Drew'
    return (idx === 0) === scoreA > scoreB ? 'Won' : 'Lost'
  }
  const lineFor = (id: string) => {
    const s = byPlayer.get(id)
    const parts: string[] = []
    if (s) parts.push(`${plural(s.goals, 'goal')}${s.position ? ` · ${POSITION_SHORT[s.position]}` : ''}`)
    const f = fouls.get(id)
    if (f) parts.push(plural(f, 'foul'))
    return parts.length ? parts.join(' · ') : null
  }
  const unassigned = match.players.filter((p) => !match.sides.some((s) => s.id === p.side_id))
  const row = (p: MatchPlayer, i: number) => (
    <PlayerRow key={p.member.id} player={p} first={i === 0} currentUserId={currentUserId} followingIds={following.data} line={lineFor(p.member.id)} />
  )

  const topCard = top && topPlayer ? (
    <TopPerformerCard
      player={topPlayer}
      detail={`${plural(top.goals, 'goal')}${top.position ? ` at ${POSITION_SHORT[top.position]}` : ''} for ${sideLabel(match, top.sideId)}`}
      value={top.points}
    />
  ) : null
  if (layout === 'top') return topCard

  return (
    <>
      {layout === 'stack' && topCard}
      <SidesGrid columns={layout === 'columns'}>
        {match.sides.slice(0, 2).map((side, idx) => {
          const players = match.players.filter((p) => p.side_id === side.id)
          const res = result(idx)
          const unnamed = byPlayer.get(`unnamed-${side.id}`)
          return (
            <div key={side.id} className="flex flex-col gap-3.5">
              <SideHeading
                index={idx}
                name={sideLabel(match, side.id, idx === 0 ? 'Side A' : 'Side B')}
                meta={`${res ? `${res} · ` : ''}${plural(players.length, 'player')}`}
              />
              <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
                {players.length === 0 && !unnamed && <p className="px-4 py-5 text-sm text-muted-foreground">No players yet.</p>}
                {players.map(row)}
                {unnamed && (
                  <div className={cn('flex min-h-16 items-center gap-3 px-4 py-2.5', players.length > 0 && 'border-t border-hairline')}>
                    <span className="box-border flex size-10 shrink-0 items-center justify-center rounded-full border-[1.5px] border-dashed text-base font-bold text-muted-foreground" style={{ borderColor: KIT_GREY }}>
                      ?
                    </span>
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="text-[15px] font-semibold">Unnamed player</span>
                      <span className="text-[13px] text-muted-foreground">{plural(unnamed.goals, 'goal')}</span>
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
            <span className="text-[13px] text-muted-foreground">{plural(unassigned.length, 'player')}</span>
          </div>
          <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">{unassigned.map(row)}</section>
        </div>
      )}
      {footer}
    </>
  )
}
