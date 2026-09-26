import { useMemo, useState } from 'react'
import { ChevronDown, ChevronUp } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { memberAvatarUrl, type ScorePlayers } from '@/lib/members'
import type { CricketFormat } from '@/lib/matchFormat'
import {
  cricketStateDescription,
  currentInnings,
  deliveryChipLabel,
  dismissalLabel,
  formatOvers,
  isChipHighlighted,
  isLegalDelivery,
  matchTotalsBySide,
  playerNameFor,
  runProgression,
  runRate,
  sideNameFor,
  wicketsInHand,
  type CricketBattingEntry,
  type CricketDelivery,
  type CricketInningsWithDeliveries,
  type CricketScore,
  type CricketScoreInnings,
  type Overs,
} from '@/lib/cricketScore'
import {
  KIT_GREY,
  PersonAvatar,
  PlayerRow,
  SideHeading,
  SideSwatch,
  TopPerformerCard,
  cardClass,
} from '@/components/agon/football/FootballMatchView'
import { useViewerFollowing } from '@/hooks/useViewerFollowing'
import { plural } from './cricketMeta'

type Match = components['schemas']['Match']
type MatchPlayer = components['schemas']['MatchPlayer']
type CricketDeliveryWicket = components['schemas']['CricketDeliveryWicket']
type CricketDismissal = components['schemas']['CricketDismissal']

// Mock colours with no theme token: the wicket tint and its text, the
// muted chip fills and the timeline rail.
const WICKET_TINT = '#FBE4DA'
const WICKET_TEXT = '#9A3210'
const LOSER_GREY = '#7D8190'
const RAIL = '#E4E0D7'

function sideIndex(match: Match, sideId: string | undefined): number {
  return Math.max(
    0,
    match.sides.findIndex((s) => s.id === sideId),
  )
}

function playerById(match: Match, id: string | undefined): MatchPlayer | undefined {
  return id ? match.players.find((p) => p.member.id === id) : undefined
}

/** "3" for whole overs, "3.2" otherwise — how bowling figures read. */
function oversShort(overs: Overs): string {
  return overs.balls === 0 ? String(overs.overs) : formatOvers(overs)
}

/** Scorecard-style dismissal: "b Jane Doe", "c Sam Lee b Jane Doe", "run out (Sam Lee)". */
function dismissalText(
  d: CricketDismissal | CricketDeliveryWicket,
  nameOf: (id: string | undefined) => string | null,
): string {
  const bowler = nameOf(d.bowler_player_id)
  const fielder = nameOf(d.fielder_player_id)
  const b = bowler ? ` b ${bowler}` : ''
  switch (d.kind) {
    case 'bowled':
      return bowler ? `b ${bowler}` : 'bowled'
    case 'caught':
      if (fielder && fielder === bowler) return `c & b ${bowler}`
      return `${fielder ? `c ${fielder}` : 'caught'}${b}`
    case 'leg_before_wicket':
      return `lbw${b}`
    case 'run_out':
      return fielder ? `run out (${fielder})` : 'run out'
    case 'stumped':
      return `${fielder ? `st ${fielder}` : 'stumped'}${b}`
    case 'hit_wicket':
      return `hit wicket${b}`
    case 'retired_out':
      return 'retired out'
    case 'retired_hurt':
      return 'retired hurt'
  }
}

// ---------------------------------------------------------------------------
// Ball chips
// ---------------------------------------------------------------------------

type ChipTone = 'dot' | 'runs' | 'boundary' | 'wicket' | 'extra'

function chipTone(d: CricketDelivery): ChipTone {
  const hl = isChipHighlighted(d)
  if (hl) return hl
  if (d.extra?.kind === 'wide' || d.extra?.kind === 'no_ball') return 'extra'
  if (!d.extra && d.runs_off_bat === 0) return 'dot'
  return 'runs'
}

/** One ball as a round chip, coloured like the mocks: dot, runs, boundary,
 *  wicket, or a ringed wide/no-ball. */
function BallChip({ d, size, dotFill = '#EEEAE2' }: { d: CricketDelivery; size: number; dotFill?: string }) {
  const tone = chipTone(d)
  const raw = deliveryChipLabel(d)
  const label = raw === '·' ? '•' : raw
  const small = label.length > 1
  return (
    <span
      style={{
        width: size,
        height: size,
        fontSize: small ? Math.round(size * 0.36) : Math.round(size * (size > 32 ? 0.4 : 0.43)),
        ...(tone === 'dot' ? { background: dotFill } : {}),
        ...(tone === 'extra' ? { borderColor: KIT_GREY } : {}),
      }}
      className={cn(
        'box-border flex shrink-0 items-center justify-center rounded-full font-extrabold',
        tone === 'dot' && 'text-muted-foreground',
        tone === 'runs' && 'bg-[#F1EEE7] text-foreground',
        tone === 'boundary' && 'bg-primary text-primary-foreground',
        tone === 'wicket' && 'bg-destructive text-white',
        tone === 'extra' && 'border-[1.5px] bg-card text-[#3D404A]',
      )}
    >
      {label}
    </span>
  )
}

/** Balls of the over in progress (or the last completed one), plus how many
 *  legal balls are still to come. */
function overInPlay(score: CricketScore, fmt: CricketFormat) {
  const recent = score.recent_deliveries ?? []
  const next = score.next_ball_context
  const currentOver = next?.over ?? recent.at(-1)?.over ?? 0
  const current = recent.filter((d) => d.over === currentOver)
  if (current.length > 0 || !recent.length) {
    const legal = current.filter((d) => isLegalDelivery(d, fmt)).length
    return { label: 'This over', balls: current, remaining: Math.max(fmt.balls_per_over - legal, 0) }
  }
  const lastOver = recent.at(-1)!.over
  return { label: 'Last over', balls: recent.filter((d) => d.over === lastOver), remaining: 0 }
}

// ---------------------------------------------------------------------------
// Header pieces
// ---------------------------------------------------------------------------

function inningsSummary(match: Match, inn: CricketScoreInnings, fmt: CricketFormat): string {
  const allOut = wicketsInHand(match, inn.batting_side_id, inn.wickets) === 0
  const full = !!fmt.overs_per_innings && inn.overs.overs >= fmt.overs_per_innings && inn.overs.balls === 0
  const ov = full
    ? `${fmt.overs_per_innings} overs`
    : `${formatOvers(inn.overs)}${fmt.overs_per_innings ? ` of ${fmt.overs_per_innings}` : ''} overs`
  if (inn.declared) return `Declared · ${ov}`
  return allOut ? `All out · ${ov}` : ov
}

/** The cricket score card from `Match.dc.html`: batting side and score, the
 *  other side, then "This over". Also covers the innings break and a
 *  finished match (both sides with a result pill). */
export function CricketHeroCard({
  match,
  score,
  format,
  live,
}: {
  match: Match
  score: CricketScore
  format: CricketFormat
  live: boolean
}) {
  const open = live ? currentInnings(score) : null
  const progress = { innings: score.innings, awaiting_next_innings: open ? false : true }
  const description = cricketStateDescription(match, progress, format)
  const totals = matchTotalsBySide(progress)

  // Sides in batting order, the side batting now first while live.
  const order: string[] = []
  if (open) order.push(open.batting_side_id)
  for (const inn of score.innings) if (!order.includes(inn.batting_side_id)) order.push(inn.batting_side_id)
  for (const s of match.sides) if (!order.includes(s.id)) order.push(s.id)

  const finished = !live && score.innings.length > 0
  const winnerId =
    finished && description && !description.startsWith('Match tied')
      ? Object.entries(totals).sort((a, b) => b[1] - a[1])[0]?.[0]
      : undefined

  const inningsOf = (sideId: string) => score.innings.filter((i) => i.batting_side_id === sideId)
  const scoreText = (list: CricketScoreInnings[]) => list.map((i) => `${i.runs}/${i.wickets}${i.declared ? 'd' : ''}`).join(' & ')

  const over = open ? overInPlay(score, format) : null

  return (
    <section className="flex flex-col gap-3.5 rounded-[20px] border bg-card p-[18px]">
      {!live && <span className="-mb-1 text-xs font-semibold text-muted-foreground">Result</span>}
      {live && !open && <span className="-mb-1 text-xs font-bold text-destructive">Innings break</span>}
      {order.map((sideId, i) => {
        const list = inningsOf(sideId)
        const name = sideNameFor(match, sideId)
        const isBatting = open?.batting_side_id === sideId
        const last = list.at(-1)
        if (isBatting && open) {
          return (
            <div key={sideId} className="flex items-center gap-2.5">
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-lg font-bold">{name}</span>
                <span className="text-[13px] text-muted-foreground">
                  Batting · {formatOvers(open.overs)}
                  {format.overs_per_innings ? ` of ${format.overs_per_innings}` : ''} overs
                </span>
              </div>
              <span className="font-display text-[40px] leading-none font-extrabold">
                {scoreText(list.length ? list : [open])}
              </span>
            </div>
          )
        }
        if (live) {
          return (
            <div
              key={sideId}
              className={cn('flex items-center gap-2.5', (open || i > 0) && 'border-t border-[#F0ECE4] pt-3')}
            >
              <span className="min-w-0 flex-1 truncate text-base font-medium text-muted-foreground">{name}</span>
              <span className="shrink-0 text-sm text-muted-foreground">
                {last ? `${scoreText(list)} · ${formatOvers(last.overs)} ov` : 'Yet to bat'}
              </span>
            </div>
          )
        }
        const lost = !!winnerId && winnerId !== sideId
        return (
          <div key={sideId} className={cn('flex items-center gap-2.5', i > 0 && 'border-t border-[#F0ECE4] pt-3')}>
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className={cn('truncate', lost ? 'text-base font-medium text-muted-foreground' : 'text-lg font-bold')}>
                {name}
              </span>
              <span className="text-[13px] text-muted-foreground">
                {last ? inningsSummary(match, last, format) : 'Did not bat'}
              </span>
            </div>
            <span
              className={cn('font-display leading-none font-extrabold', lost ? 'text-[32px]' : 'text-[40px]')}
              style={lost ? { color: LOSER_GREY } : undefined}
            >
              {list.length ? scoreText(list) : '–'}
            </span>
          </div>
        )
      })}
      {description && (
        <span className="flex h-[30px] items-center self-start rounded-full bg-accent px-3 text-sm font-bold text-accent-foreground">
          {description}
        </span>
      )}
      {over && (over.balls.length > 0 || over.remaining > 0) && (
        <div className="flex flex-col gap-2 pt-1">
          <span className="text-[13px] font-semibold text-muted-foreground">{over.label}</span>
          <div className="flex flex-wrap gap-2">
            {over.balls.map((d, i) => (
              <BallChip key={i} d={d} size={38} dotFill="#F1EEE7" />
            ))}
            {Array.from({ length: over.remaining }, (_, i) => (
              <span key={`r${i}`} className="box-border size-[38px] rounded-full border-[1.5px] border-dashed border-[#CFC9BD]" />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}

/** Compact score strip that replaces the hero on the other tabs (`EventsCricket.dc.html`). */
export function CricketScoreStrip({ match, score, live }: { match: Match; score: CricketScore; live: boolean }) {
  const open = live ? currentInnings(score) : null
  const shown = open ?? score.innings.at(-1)
  if (!shown) return null
  if (!live && score.innings.length > 1) {
    // Finished: both sides, loser muted.
    const totals = matchTotalsBySide({ innings: score.innings, awaiting_next_innings: true })
    const [a, b] = match.sides
    const ta = totals[a?.id ?? ''] ?? 0
    const tb = totals[b?.id ?? ''] ?? 0
    const text = (sideId: string | undefined) =>
      score.innings
        .filter((i) => i.batting_side_id === sideId)
        .map((i) => `${i.runs}/${i.wickets}`)
        .join(' & ') || '–'
    const row = (side: typeof a, idx: number, lost: boolean) => (
      <div className={cn('flex items-center gap-2 text-[15px]', lost ? 'font-medium text-muted-foreground' : 'font-bold')}>
        <SideSwatch index={idx} size={12} />
        <span className="min-w-0 flex-1 truncate">{sideNameFor(match, side?.id ?? '')}</span>
        <span className="shrink-0 font-display text-xl font-extrabold" style={lost ? { color: LOSER_GREY } : undefined}>
          {text(side?.id)}
        </span>
      </div>
    )
    return (
      <div className="flex flex-col gap-1.5 rounded-2xl border bg-card px-4 py-3">
        {row(a, 0, tb > ta)}
        {row(b, 1, ta > tb)}
      </div>
    )
  }
  return (
    <div className="flex items-center gap-3 rounded-2xl border bg-card px-4 py-3">
      {live && (
        <span className="flex h-[22px] shrink-0 items-center gap-[5px] rounded-full bg-destructive px-2 text-[10px] font-bold tracking-[0.5px] text-white">
          <span className="size-[5px] rounded-full bg-white" />
          LIVE
        </span>
      )}
      <span className="min-w-0 flex-1 truncate text-[15px] font-bold">{sideNameFor(match, shown.batting_side_id)}</span>
      <span className="shrink-0 text-[13px] text-muted-foreground">{formatOvers(shown.overs)} ov</span>
      <span className="shrink-0 font-display text-2xl font-extrabold">
        {shown.runs}/{shown.wickets}
      </span>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Summary tab
// ---------------------------------------------------------------------------

/** "At the crease": both batters and the current bowler (`Match.dc.html`). */
export function AtTheCreaseCard({ match, score }: { match: Match; score: CricketScore }) {
  const open = currentInnings(score)
  const next = score.next_ball_context
  if (!open || !next) return null
  const lastBall = score.recent_deliveries?.at(-1)
  const strikerId = next.striker_player_id ?? lastBall?.striker_player_id
  const nonStrikerId = next.non_striker_player_id ?? lastBall?.non_striker_player_id
  const bowlerId = next.bowler_player_id ?? lastBall?.bowler_player_id
  const nameOf = (id: string | undefined) => playerNameFor(match, id, score.players)
  const batting = (id: string | undefined) => open.batting?.find((b) => b.player_id === id)
  const bowling = open.bowling?.find((b) => b.player_id === bowlerId)
  const batters = [strikerId, nonStrikerId].filter((id): id is string => !!id)
  if (batters.length === 0 && !bowlerId) return null

  const avatar = (id: string, name: string) => (
    <PersonAvatar name={name} imageUrl={(() => { const p = playerById(match, id); return p ? memberAvatarUrl(p.member) : undefined })()} size={36} />
  )

  return (
    <section className="flex flex-col rounded-[20px] border bg-card px-[18px] py-1.5">
      <span className="pt-3 pb-1.5 text-[13px] font-semibold text-muted-foreground">At the crease</span>
      {batters.map((id, i) => {
        const name = nameOf(id) ?? 'Unknown batter'
        const entry = batting(id)
        const onStrike = id === strikerId
        const boundaryNote = entry?.sixes ? plural(entry.sixes, 'six', 'sixes') : entry?.fours ? plural(entry.fours, 'four') : null
        return (
          <div key={id} className={cn('flex items-center gap-3 py-2.5', i > 0 && 'border-t border-[#F0ECE4]')}>
            {avatar(id, name)}
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-base font-semibold">{name}</span>
              {onStrike && (
                <span className="text-[13px] text-muted-foreground">
                  On strike{boundaryNote ? ` · ${boundaryNote}` : ''}
                </span>
              )}
            </div>
            <span className="font-display text-[22px] font-extrabold">{entry?.runs ?? 0}</span>
            <span className="w-[34px] text-[13px] text-muted-foreground">({entry?.balls_faced ?? 0})</span>
          </div>
        )
      })}
      {bowlerId && (
        <>
          <span className="border-t border-[#F0ECE4] pt-3.5 pb-1.5 text-[13px] font-semibold text-muted-foreground">Bowling</span>
          <div className="flex items-center gap-3 pt-2.5 pb-3.5">
            {avatar(bowlerId, nameOf(bowlerId) ?? 'Unknown bowler')}
            <span className="min-w-[72px] flex-1 truncate text-base font-semibold">{nameOf(bowlerId) ?? 'Unknown bowler'}</span>
            <span className="text-right text-[15px] font-semibold">
              {bowling
                ? `${oversShort(bowling.overs)} ov · ${plural(bowling.runs_conceded, 'run')} · ${plural(bowling.wickets, 'wkt')}`
                : 'First over'}
            </span>
          </div>
        </>
      )}
    </section>
  )
}

interface FallOfWicket {
  runs: number
  wicket: number
  overs?: Overs
}

function fallOfWickets(inn: CricketInningsWithDeliveries, fmt: CricketFormat): FallOfWicket[] {
  if (inn.fall_of_wickets?.length) return inn.fall_of_wickets
  return runProgression(inn.deliveries, fmt)
    .filter((p) => p.isWicket)
    .map((p) => ({ runs: p.runs, wicket: p.wickets, overs: p.overs }))
}

function FallOfWicketPills({ items }: { items: FallOfWicket[] }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {items.map((f) => (
        <span
          key={f.wicket}
          className="flex h-7 items-center rounded-full px-2.5 text-xs font-bold whitespace-nowrap"
          style={{ background: WICKET_TINT, color: WICKET_TEXT }}
        >
          {f.runs}/{f.wicket}
          {f.overs && <span className="ml-1 font-medium">({formatOvers(f.overs)})</span>}
        </span>
      ))}
    </div>
  )
}

/** Pick a round y-axis step so the chart has at most four gridlines. */
function niceStep(max: number): number {
  for (const step of [5, 10, 20, 25, 40, 50, 100, 200, 250, 500]) if (Math.ceil(max / step) <= 4) return step
  return 1000
}

/** "Runs over time": each innings' run progression with wicket markers, a
 *  projected line while live, and the fall of wickets (`Match.dc.html`). */
export function RunsOverTimeCard({
  match,
  innings,
  format,
  live,
}: {
  match: Match
  innings: CricketInningsWithDeliveries[]
  format: CricketFormat
  live: boolean
}) {
  const series = innings
    .map((inn, i) => ({ inn, i, points: runProgression(inn.deliveries, format) }))
    .filter((s) => s.points.length > 0)
  if (series.length === 0) return null

  const last = series.at(-1)!
  const isOpenLast = live && last.i === innings.length - 1
  const rr = runRate(last.inn.runs, last.inn.overs, format.balls_per_over)
  const lastOvers = last.points.at(-1)!.overDecimal
  const maxOvers = Math.max(format.overs_per_innings ?? 0, ...series.map((s) => Math.ceil(s.points.at(-1)!.overDecimal)), 1)
  const projected =
    isOpenLast && format.overs_per_innings && lastOvers < format.overs_per_innings
      ? last.inn.runs + rr * (format.overs_per_innings - lastOvers)
      : null
  const maxRuns = Math.max(...series.map((s) => s.inn.runs), projected ?? 0, 1)
  const yStep = niceStep(maxRuns * 1.05)
  const yMax = yStep * Math.max(Math.ceil((maxRuns * 1.05) / yStep), 1)

  // Plot area mirrors the mock's 322×190 viewBox.
  const x0 = 34
  const x1 = 314
  const yBase = 162
  const yTop = 12
  const xFor = (o: number) => x0 + (o / maxOvers) * (x1 - x0)
  const yFor = (r: number) => yBase - (r / yMax) * (yBase - yTop)
  const xStep = maxOvers <= 25 ? 5 : maxOvers <= 50 ? 10 : 20
  const xTicks: number[] = []
  for (let o = 0; o <= maxOvers; o += xStep) xTicks.push(o)
  const yTicks: number[] = []
  for (let v = 0; v <= yMax; v += yStep) yTicks.push(v)

  // End labels sit above each line's last point; a second label that would
  // collide with the first drops below its point instead.
  const endYs = series.map((s) => yFor(s.inn.runs))
  const labelY = (si: number, y: number) =>
    endYs.some((o, oi) => oi < si && Math.abs(o - y) < 22) ? y + 22 : y - 12
  const colorFor = (sideId: string) => (sideIndex(match, sideId) === 0 ? 'var(--primary)' : KIT_GREY)

  return (
    <section className="flex flex-col gap-3 rounded-[20px] border bg-card px-[18px] py-4">
      <div className="flex items-baseline justify-between">
        <span className="text-[15px] font-bold">Runs over time</span>
        <span className="text-[13px] text-muted-foreground">Run rate {rr.toFixed(2)}</span>
      </div>
      <div className="flex flex-wrap gap-3.5 text-xs text-muted-foreground">
        {series.map((s) => (
          <span key={s.i} className="flex items-center gap-1.5">
            <span className="h-[3px] w-3.5 rounded-sm" style={{ background: colorFor(s.inn.batting_side_id) }} />
            {sideNameFor(match, s.inn.batting_side_id)}
            {series.filter((o) => o.inn.batting_side_id === s.inn.batting_side_id).length > 1 && ` (${s.i + 1})`}
          </span>
        ))}
        <span className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-full bg-destructive" />
          Wicket
        </span>
        {projected !== null && (
          <span className="flex items-center gap-1.5">
            <span className="w-3.5 border-t-[1.5px] border-dashed border-primary opacity-60" />
            Projected
          </span>
        )}
      </div>
      <svg
        width="100%"
        viewBox="0 0 322 190"
        role="img"
        aria-label={`Runs over time: ${series.map((s) => `${sideNameFor(match, s.inn.batting_side_id)} ${s.inn.runs} for ${s.inn.wickets}`).join(', ')}`}
        className="block font-sans"
      >
        {yTicks.map((v) => (
          <g key={v}>
            <line x1={x0} x2={x1} y1={yFor(v)} y2={yFor(v)} stroke="#EEEAE2" strokeWidth="1" />
            <text x={x0 - 8} y={yFor(v) + 4} textAnchor="end" fontSize="11" className="fill-muted-foreground">
              {v}
            </text>
          </g>
        ))}
        {xTicks.map((o) => (
          <text key={o} x={xFor(o)} y="180" textAnchor="middle" fontSize="11" className="fill-muted-foreground">
            {o}
          </text>
        ))}
        {series.map((s, si) => {
          const color = colorFor(s.inn.batting_side_id)
          const pts = [{ x: x0, y: yBase }, ...s.points.map((p) => ({ x: xFor(p.overDecimal), y: yFor(p.runs) }))]
          const line = pts.map((p, k) => `${k ? 'L' : 'M'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' ')
          const end = pts.at(-1)!
          const primary = s === last
          return (
            <g key={s.i}>
              {primary && <path d={`${line} L${end.x.toFixed(1)} ${yBase} L${x0} ${yBase} Z`} fill={color} fillOpacity="0.08" />}
              <path d={line} fill="none" stroke={color} strokeWidth="2.5" strokeLinejoin="round" strokeLinecap="round" />
              {primary && projected !== null && (
                <line
                  x1={end.x}
                  x2={x1}
                  y1={end.y}
                  y2={yFor(projected)}
                  stroke={color}
                  strokeWidth="1.5"
                  strokeDasharray="3 4"
                  opacity="0.5"
                />
              )}
              {s.points
                .filter((p) => p.isWicket)
                .map((p, k) => (
                  <circle key={k} cx={xFor(p.overDecimal)} cy={yFor(p.runs)} r="5.5" fill="#D2461B" stroke="#FFFFFF" strokeWidth="2">
                    <title>
                      Wicket at {p.runs}/{p.wickets} ({formatOvers(p.overs)} ov)
                    </title>
                  </circle>
                ))}
              <circle cx={end.x} cy={end.y} r="5.5" fill={color} stroke="#FFFFFF" strokeWidth="2" />
              <text
                x={end.x - 8}
                y={labelY(si, end.y)}
                textAnchor="end"
                fontSize="13"
                fontWeight="700"
                className="fill-foreground"
              >
                {s.inn.runs}/{s.inn.wickets}
              </text>
            </g>
          )
        })}
      </svg>
      {series.some((s) => fallOfWickets(s.inn, format).length > 0) && (
        <div className="flex flex-col gap-2 border-t border-[#F0ECE4]">
          {series.map((s) => {
            const fow = fallOfWickets(s.inn, format)
            if (!fow.length) return null
            return (
              <div key={s.i} className="flex flex-col gap-2">
                <span className="pt-3 text-[13px] font-semibold text-muted-foreground">
                  Fall of wickets{series.length > 1 ? ` · ${sideNameFor(match, s.inn.batting_side_id)}` : ''}
                </span>
                <FallOfWicketPills items={fow} />
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

// ---------------------------------------------------------------------------
// Scorecard tab
// ---------------------------------------------------------------------------

const BAT_GRID = 'grid grid-cols-[minmax(0,1fr)_30px_30px_26px_26px_44px] items-center gap-x-1.5'
const BOWL_GRID = 'grid grid-cols-[minmax(0,1fr)_34px_26px_30px_26px_40px] items-center gap-x-1.5'

function extrasLine(inn: CricketScoreInnings): { total: number; detail: string } {
  const e = inn.extras
  if (!e) return { total: 0, detail: '' }
  const parts: string[] = []
  if (e.byes) parts.push(`b ${e.byes}`)
  if (e.leg_byes) parts.push(`lb ${e.leg_byes}`)
  if (e.wides) parts.push(`w ${e.wides}`)
  if (e.no_balls) parts.push(`nb ${e.no_balls}`)
  if (e.penalty) parts.push(`p ${e.penalty}`)
  return { total: e.byes + e.leg_byes + e.wides + e.no_balls + e.penalty, detail: parts.join(', ') }
}

/** Full batting and bowling cards per innings, in the redesign's card style. */
export function CricketScorecardTab({
  match,
  innings,
  players,
  format,
}: {
  match: Match
  innings: CricketInningsWithDeliveries[]
  players?: ScorePlayers
  format: CricketFormat
}) {
  if (innings.length === 0) {
    return (
      <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
        The scorecard fills in ball by ball once scoring starts.
      </section>
    )
  }
  const nameOf = (id: string | undefined) => playerNameFor(match, id, players)
  return (
    <>
      {innings.map((inn, idx) => {
        const batting = [...(inn.batting ?? [])].sort((a, b) => (a.batting_position ?? 99) - (b.batting_position ?? 99))
        const bowling = inn.bowling ?? []
        const extras = extrasLine(inn)
        const didNotBat = match.players.filter(
          (p) => p.side_id === inn.batting_side_id && !batting.some((b) => b.player_id === p.member.id),
        )
        const fow = fallOfWickets(inn, format)
        const rr = runRate(inn.runs, inn.overs, format.balls_per_over)
        return (
          <div key={`${inn.batting_side_id}-${idx}`} className="flex flex-col gap-3.5">
            <SideHeading
              index={sideIndex(match, inn.batting_side_id)}
              name={sideNameFor(match, inn.batting_side_id)}
              meta={`${inn.runs}/${inn.wickets}${inn.declared ? 'd' : ''} · ${formatOvers(inn.overs)} ov`}
            />
            <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
              <div className={cn(BAT_GRID, 'px-4 pt-3.5 pb-2 text-xs font-semibold text-muted-foreground')}>
                <span>Batter</span>
                <span className="text-right">R</span>
                <span className="text-right">B</span>
                <span className="text-right">4s</span>
                <span className="text-right">6s</span>
                <span className="text-right">SR</span>
              </div>
              {batting.length === 0 && <p className="px-4 pb-3 text-sm text-muted-foreground">No batting recorded.</p>}
              {batting.map((b: CricketBattingEntry, bi) => (
                <div key={`${b.player_id}-${bi}`} className={cn(BAT_GRID, 'border-t border-[#F0ECE4] px-4 py-2.5 tabular-nums')}>
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate text-[15px] font-semibold">
                      {nameOf(b.player_id) ?? 'Unknown'}
                      {!b.dismissal && '*'}
                    </span>
                    <span className="truncate text-[13px] text-muted-foreground">
                      {b.dismissal ? dismissalText(b.dismissal, nameOf) : 'not out'}
                    </span>
                  </span>
                  <span className="text-right font-display text-base font-extrabold">{b.runs}</span>
                  <span className="text-right text-sm text-muted-foreground">{b.balls_faced}</span>
                  <span className="text-right text-sm">{b.fours}</span>
                  <span className="text-right text-sm">{b.sixes}</span>
                  <span className="text-right text-sm text-muted-foreground">
                    {b.balls_faced ? ((b.runs / b.balls_faced) * 100).toFixed(0) : '–'}
                  </span>
                </div>
              ))}
              {inn.extras && (
                <div className="flex items-center gap-2 border-t border-[#F0ECE4] px-4 py-2.5 text-sm">
                  <span className="font-semibold">Extras</span>
                  <span className="flex-1 truncate text-[13px] text-muted-foreground">{extras.detail && `(${extras.detail})`}</span>
                  <span className="font-semibold tabular-nums">{extras.total}</span>
                </div>
              )}
              <div className="flex items-center gap-2 border-t border-[#F0ECE4] bg-muted/60 px-4 py-3">
                <span className="text-[15px] font-bold">Total</span>
                <span className="flex-1 text-[13px] text-muted-foreground">
                  {formatOvers(inn.overs)} ov · RR {rr.toFixed(2)}
                </span>
                <span className="font-display text-lg font-extrabold tabular-nums">
                  {inn.runs}/{inn.wickets}
                </span>
              </div>
              {didNotBat.length > 0 && inn.batting && inn.batting.length > 0 && (
                <p className="border-t border-[#F0ECE4] px-4 py-2.5 text-[13px] text-muted-foreground">
                  <span className="font-semibold text-foreground">Yet to bat · </span>
                  {didNotBat.map((p) => nameOf(p.member.id)).filter(Boolean).join(', ')}
                </p>
              )}
            </section>

            {bowling.length > 0 && (
              <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
                <div className={cn(BOWL_GRID, 'px-4 pt-3.5 pb-2 text-xs font-semibold text-muted-foreground')}>
                  <span>Bowler</span>
                  <span className="text-right">O</span>
                  <span className="text-right">M</span>
                  <span className="text-right">R</span>
                  <span className="text-right">W</span>
                  <span className="text-right">Econ</span>
                </div>
                {bowling.map((bw, bi) => {
                  const balls = bw.overs.overs * format.balls_per_over + bw.overs.balls
                  return (
                    <div key={`${bw.player_id}-${bi}`} className={cn(BOWL_GRID, 'border-t border-[#F0ECE4] px-4 py-3 tabular-nums')}>
                      <span className="truncate text-[15px] font-semibold">{nameOf(bw.player_id) ?? 'Unknown'}</span>
                      <span className="text-right text-sm">{formatOvers(bw.overs)}</span>
                      <span className="text-right text-sm text-muted-foreground">{bw.maidens}</span>
                      <span className="text-right text-sm">{bw.runs_conceded}</span>
                      <span className="text-right font-display text-base font-extrabold">{bw.wickets}</span>
                      <span className="text-right text-sm text-muted-foreground">
                        {balls ? ((bw.runs_conceded / balls) * format.balls_per_over).toFixed(1) : '–'}
                      </span>
                    </div>
                  )
                })}
              </section>
            )}

            {fow.length > 0 && (
              <section className={cn(cardClass, 'flex flex-col gap-2')}>
                <span className="text-[13px] font-semibold text-muted-foreground">Fall of wickets</span>
                <FallOfWicketPills items={fow} />
              </section>
            )}
          </div>
        )
      })}
    </>
  )
}

// ---------------------------------------------------------------------------
// Timeline tab
// ---------------------------------------------------------------------------

type BallFilter = 'all' | 'wickets' | 'boundaries' | 'overs'
const FILTERS: { id: BallFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'wickets', label: 'Wickets' },
  { id: 'boundaries', label: 'Boundaries' },
  { id: 'overs', label: 'Overs' },
]

interface BallView {
  d: CricketDelivery
  runs: number
  wickets: number
  changed: boolean
}

interface OverGroup {
  key: string
  inningsIndex: number
  over: number
  bowlerId: string
  balls: BallView[]
  /** Score at the end of the over before this one. */
  before: { runs: number; wickets: number }
}

function overGroups(innings: CricketInningsWithDeliveries[]): OverGroup[] {
  const groups: OverGroup[] = []
  innings.forEach((inn, ii) => {
    let runs = 0
    let wickets = 0
    let group: OverGroup | null = null
    for (const d of inn.deliveries) {
      if (!group || group.over !== d.over) {
        group = { key: `${ii}-${d.over}`, inningsIndex: ii, over: d.over, bowlerId: d.bowler_player_id, balls: [], before: { runs, wickets } }
        groups.push(group)
      }
      const add = d.runs_off_bat + (d.extra?.runs ?? 0)
      runs += add
      if (d.wicket) wickets += 1
      group.balls.push({ d, runs, wickets, changed: add > 0 || !!d.wicket })
    }
  })
  return groups
}

function ballTitle(d: CricketDelivery, nameOf: (id: string | undefined) => string | null): string {
  const striker = nameOf(d.striker_player_id) ?? 'Batter'
  if (d.wicket) {
    const out = nameOf(d.wicket.dismissed_player_id) ?? striker
    return `WICKET · ${out} ${dismissalText(d.wicket, nameOf)}`
  }
  if (d.extra) {
    const n = d.extra.runs
    switch (d.extra.kind) {
      case 'wide':
        return n > 1 ? `${n} wides` : 'Wide'
      case 'no_ball':
        return d.runs_off_bat ? `No ball · ${plural(d.runs_off_bat, 'run')} to ${striker}` : 'No ball'
      case 'bye':
        return plural(n, 'bye')
      case 'leg_bye':
        return plural(n, 'leg bye')
      case 'penalty':
        return `${plural(n, 'penalty run')}`
    }
  }
  if (d.runs_off_bat === 4) return `FOUR · ${striker}`
  if (d.runs_off_bat === 6) return `SIX · ${striker}`
  if (d.runs_off_bat === 0) return 'Dot ball'
  return `${plural(d.runs_off_bat, 'run')} · ${striker}`
}

function clock(iso: string | undefined): string | null {
  if (!iso) return null
  return new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
}

/** Ball-by-ball, newest first, grouped by over (`EventsCricket.dc.html`). */
export function CricketTimeline({
  match,
  innings,
  players,
  format,
  live,
}: {
  match: Match
  innings: CricketInningsWithDeliveries[]
  players?: ScorePlayers
  format: CricketFormat
  live: boolean
}) {
  const [filter, setFilter] = useState<BallFilter>('all')
  const [toggled, setToggled] = useState<Set<string>>(new Set())
  const [limit, setLimit] = useState(8)
  const nameOf = (id: string | undefined) => playerNameFor(match, id, players)

  const groups = useMemo(() => overGroups(innings).reverse(), [innings])
  if (groups.length === 0) {
    return (
      <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
        {live ? 'Balls will show up here as they are scored.' : 'This match was not scored ball by ball.'}
      </section>
    )
  }

  const latestKey = groups[0].key
  const matches = (b: BallView) =>
    filter === 'wickets' ? !!b.d.wicket : filter === 'boundaries' ? isChipHighlighted(b.d) === 'boundary' : true
  const visible = groups.filter((g) => filter === 'all' || filter === 'overs' || g.balls.some(matches))
  const shown = visible.slice(0, limit)

  const isOpen = (g: OverGroup) => {
    if (filter === 'overs') return false
    if (filter !== 'all') return true
    const byDefault = g.key === latestKey || g.balls.some((b) => b.d.wicket)
    return toggled.has(g.key) ? !byDefault : byDefault
  }
  const toggle = (key: string) =>
    setToggled((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })

  const overMeta = (g: OverGroup, current: boolean) => {
    const last = g.balls.at(-1)!
    const runs = last.runs - g.before.runs
    const wkts = last.wickets - g.before.wickets
    const wides = g.balls.filter((b) => b.d.extra?.kind === 'wide').length
    const nbs = g.balls.filter((b) => b.d.extra?.kind === 'no_ball').length
    if (current) return `${plural(runs, 'run')} so far · ${last.runs}/${last.wickets}`
    const legal = g.balls.filter((b) => isLegalDelivery(b.d, format)).length
    const parts = [runs === 0 && legal >= format.balls_per_over ? 'Maiden' : plural(runs, 'run')]
    if (wides) parts.push(plural(wides, 'wide'))
    if (nbs) parts.push(plural(nbs, 'no ball'))
    if (wkts) parts.push(plural(wkts, 'wicket'))
    return parts.join(' · ')
  }

  const multiInnings = innings.length > 1

  return (
    <>
      <div role="group" aria-label="Filter timeline" className="flex flex-wrap items-center gap-2">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            type="button"
            aria-pressed={filter === f.id}
            onClick={() => setFilter(f.id)}
            className={cn(
              'box-border h-9 rounded-full border px-3.5 text-sm font-bold',
              filter === f.id ? 'border-foreground bg-foreground text-background' : 'border-[#DCD7CC] bg-card text-foreground',
            )}
          >
            {f.label}
          </button>
        ))}
        <span className="ml-auto text-[13px] text-muted-foreground">Newest first</span>
      </div>

      <section aria-label="Ball by ball" className="flex flex-col rounded-[20px] border bg-card px-1.5 pt-1 pb-2.5">
        {shown.length === 0 && (
          <p className="px-3 py-5 text-sm text-muted-foreground">
            {filter === 'wickets' ? 'No wickets yet.' : 'No boundaries yet.'}
          </p>
        )}
        {shown.map((g, gi) => {
          const current = live && g.key === latestKey
          const open = isOpen(g)
          const title = `Over ${g.over + 1} · ${nameOf(g.bowlerId) ?? 'Unknown'} bowling`
          const inningsLabel =
            multiInnings && (gi === 0 || shown[gi - 1].inningsIndex !== g.inningsIndex)
              ? `${sideNameFor(match, innings[g.inningsIndex].batting_side_id)} innings`
              : null
          const prevOver = groups.find((o) => o.inningsIndex === g.inningsIndex && o.over === g.over - 1)
          const balls = [...g.balls].reverse().filter((b) => filter === 'all' || matches(b))
          return (
            <div key={g.key} className="flex flex-col">
              {inningsLabel && (
                <span className={cn('px-3 pt-3 text-xs font-bold tracking-[0.5px] text-muted-foreground uppercase', gi > 0 && 'mt-2 border-t border-[#F0ECE4] pt-4')}>
                  {inningsLabel}
                </span>
              )}
              {open ? (
                <>
                  <button
                    type="button"
                    onClick={() => filter === 'all' && !current && toggle(g.key)}
                    className="flex items-center gap-2.5 px-3 pt-3 pb-1.5 text-left"
                  >
                    <span className="flex min-w-0 flex-1 flex-col gap-px">
                      <span className="truncate text-[15px] font-bold">{title}</span>
                      <span className="text-[13px] text-muted-foreground">{overMeta(g, current)}</span>
                    </span>
                    {current ? (
                      <span className="flex h-[22px] shrink-0 items-center gap-[5px] rounded-full bg-destructive px-2 text-[10px] font-bold tracking-[0.5px] text-white">
                        <span className="size-[5px] rounded-full bg-white" />
                        LIVE
                      </span>
                    ) : (
                      filter === 'all' && <ChevronUp className="size-5 shrink-0 text-muted-foreground" />
                    )}
                  </button>
                  {balls.map((b, bi) => {
                    const time = clock(b.d.occurred_at)
                    const sub = ballSubtitle(b.d, innings[g.inningsIndex], nameOf)
                    const lastRow = bi === balls.length - 1
                    return (
                      <div
                        key={bi}
                        className="grid grid-cols-[44px_32px_minmax(0,1fr)_auto] items-stretch gap-x-2.5 rounded-[14px] pr-2.5 pl-1.5"
                        style={b.d.wicket ? { background: WICKET_TINT } : undefined}
                      >
                        <div className="flex flex-col items-end justify-center py-3">
                          <span className="text-sm font-bold tabular-nums">
                            {b.d.over}.{b.d.ball}
                          </span>
                          {time && <span className="text-[11px] text-muted-foreground">{time}</span>}
                        </div>
                        <div className="flex flex-col items-center">
                          <span className="w-0.5 flex-1" style={{ background: bi === 0 ? 'transparent' : RAIL }} />
                          <BallChip d={b.d} size={30} />
                          <span className="w-0.5 flex-1" style={{ background: lastRow && !prevOver ? 'transparent' : RAIL }} />
                        </div>
                        <div className="flex min-w-0 flex-col justify-center gap-px py-3">
                          <span className="text-[15px] font-semibold">{ballTitle(b.d, nameOf)}</span>
                          {sub && <span className="text-[13px] text-muted-foreground">{sub}</span>}
                        </div>
                        <div className="flex items-center">
                          {b.changed && (
                            <span className="font-display text-base font-extrabold whitespace-nowrap">
                              {b.runs}/{b.wickets}
                            </span>
                          )}
                        </div>
                      </div>
                    )
                  })}
                  {filter === 'all' && prevOver && (
                    <div className="grid grid-cols-[44px_32px_minmax(0,1fr)] gap-x-2.5 pr-2.5 pl-1.5">
                      <span />
                      <div className="flex flex-col items-center">
                        <span className="h-2.5 w-0.5" style={{ background: RAIL }} />
                        <span className="size-3 rounded-full bg-foreground" />
                        <span className="h-2.5 w-0.5" style={{ background: RAIL }} />
                      </div>
                      <div className="flex items-center">
                        <span className="flex h-7 items-center rounded-full bg-foreground px-3 text-[13px] font-bold text-background">
                          End of over {prevOver.over + 1}
                          <span className="font-medium text-[#C9CCD4]">
                            &nbsp;· {g.before.runs}/{g.before.wickets}
                          </span>
                        </span>
                      </div>
                    </div>
                  )}
                </>
              ) : (
                <button
                  type="button"
                  onClick={() => filter === 'all' && toggle(g.key)}
                  className="my-1.5 flex flex-col items-stretch gap-2.5 border-y border-[#F0ECE4] p-3 text-left"
                >
                  <span className="flex items-center gap-2.5">
                    <span className="flex min-w-0 flex-1 flex-col gap-px">
                      <span className="truncate text-[15px] font-bold">{title}</span>
                      <span className="text-[13px] text-muted-foreground">{overMeta(g, current)}</span>
                    </span>
                    {filter === 'all' && <ChevronDown className="size-5 shrink-0 text-muted-foreground" />}
                  </span>
                  <span className="flex flex-wrap gap-1.5">
                    {g.balls.map((b, bi) => (
                      <BallChip key={bi} d={b.d} size={30} />
                    ))}
                  </span>
                </button>
              )}
            </div>
          )
        })}
      </section>
      {visible.length > limit && (
        <button
          type="button"
          onClick={() => setLimit((n) => n + 10)}
          className="box-border h-12 rounded-[14px] border border-[#DCD7CC] bg-card text-sm font-bold"
        >
          Show earlier overs
        </button>
      )}
    </>
  )
}

function ballSubtitle(
  d: CricketDelivery,
  inn: CricketInningsWithDeliveries,
  nameOf: (id: string | undefined) => string | null,
): string | null {
  if (d.wicket) {
    const entry = inn.batting?.find((b) => b.player_id === d.wicket!.dismissed_player_id)
    const parts = [`${dismissalLabel(d.wicket.kind)}${entry ? ` for ${entry.runs}` : ''}`]
    // The new batter is whoever first appears at the crease after this ball.
    const idx = inn.deliveries.indexOf(d)
    const pair = new Set([d.striker_player_id, d.non_striker_player_id])
    const after = inn.deliveries.slice(idx + 1).find((n) => !pair.has(n.striker_player_id) || !pair.has(n.non_striker_player_id))
    const incoming = after ? [after.striker_player_id, after.non_striker_player_id].find((id) => !pair.has(id)) : undefined
    const incomingName = nameOf(incoming)
    if (incomingName) parts.push(`${incomingName} comes in`)
    return parts.join(' · ')
  }
  if (!d.extra && d.runs_off_bat === 0) {
    const striker = nameOf(d.striker_player_id)
    return striker ? `To ${striker}` : null
  }
  if (d.extra && d.extra.kind !== 'no_ball') {
    const striker = nameOf(d.striker_player_id)
    return striker ? `To ${striker}` : null
  }
  return null
}

// ---------------------------------------------------------------------------
// Players tab
// ---------------------------------------------------------------------------

interface CricketLine {
  runs: number
  balls: number
  notOut: boolean
  batted: boolean
  wickets: number
  conceded: number
  bowledBalls: number
}

export function CricketPlayersTab({
  match,
  innings,
  currentUserId,
  format,
  finished,
  footer,
}: {
  match: Match
  innings: CricketScoreInnings[]
  currentUserId?: string
  format: CricketFormat
  finished: boolean
  footer?: React.ReactNode
}) {
  const following = useViewerFollowing(currentUserId)
  const stats = useMemo(() => {
    const map = new Map<string, CricketLine>()
    const get = (id: string) => {
      let s = map.get(id)
      if (!s) {
        s = { runs: 0, balls: 0, notOut: false, batted: false, wickets: 0, conceded: 0, bowledBalls: 0 }
        map.set(id, s)
      }
      return s
    }
    for (const inn of innings) {
      for (const b of inn.batting ?? []) {
        const s = get(b.player_id)
        s.runs += b.runs
        s.balls += b.balls_faced
        s.batted = true
        s.notOut = !b.dismissal
      }
      for (const bw of inn.bowling ?? []) {
        const s = get(bw.player_id)
        s.wickets += bw.wickets
        s.conceded += bw.runs_conceded
        s.bowledBalls += bw.overs.overs * format.balls_per_over + bw.overs.balls
      }
    }
    return map
  }, [innings, format.balls_per_over])

  const bowlingFigure = (s: CricketLine) => {
    const ov = { overs: Math.floor(s.bowledBalls / format.balls_per_over), balls: s.bowledBalls % format.balls_per_over }
    return `${s.wickets}/${s.conceded} (${oversShort(ov)} ov)`
  }
  const lineFor = (s: CricketLine | undefined) => {
    if (!s) return null
    const parts: string[] = []
    if (s.batted) parts.push(`${s.runs}${s.notOut ? '*' : ''} (${s.balls})`)
    if (s.bowledBalls > 0) parts.push(bowlingFigure(s))
    return parts.length ? parts.join(' · ') : null
  }

  // Top performer: runs plus 20 a wicket, so a three-for rivals a fifty.
  let top: { player: MatchPlayer; s: CricketLine; impact: number } | null = null
  for (const p of match.players) {
    const s = stats.get(p.member.id)
    if (!s) continue
    const impact = s.runs + s.wickets * 20
    if (impact > 0 && (!top || impact > top.impact)) top = { player: p, s, impact }
  }

  const progress = { innings, awaiting_next_innings: true }
  const totals = matchTotalsBySide(progress)
  const description = finished ? cricketStateDescription(match, progress, format) : null
  const result = (sideId: string) => {
    if (!description) return null
    if (description.startsWith('Match tied')) return 'Tied'
    const winner = Object.entries(totals).sort((a, b) => b[1] - a[1])[0]?.[0]
    return winner === sideId ? 'Won' : 'Lost'
  }
  const unassigned = match.players.filter((p) => !match.sides.some((s) => s.id === p.side_id))

  const row = (p: MatchPlayer, i: number) => (
    <PlayerRow
      key={p.member.id}
      player={p}
      first={i === 0}
      currentUserId={currentUserId}
      followingIds={following.data}
      line={lineFor(stats.get(p.member.id))}
    />
  )

  return (
    <>
      {top && (
        <TopPerformerCard
          player={top.player}
          detail={[
            top.s.batted ? `${top.s.runs} off ${top.s.balls}` : null,
            top.s.wickets ? `${top.s.wickets} for ${top.s.conceded}` : null,
          ]
            .filter(Boolean)
            .join(' · ')}
          value={top.s.runs >= top.s.wickets * 20 ? top.s.runs : `${top.s.wickets}/${top.s.conceded}`}
        />
      )}
      {match.sides.slice(0, 2).map((side, idx) => {
        const players = match.players.filter((p) => p.side_id === side.id)
        const res = result(side.id)
        return (
          <div key={side.id} className="flex flex-col gap-3.5">
            <SideHeading
              index={idx}
              name={sideNameFor(match, side.id)}
              meta={`${res ? `${res} · ` : ''}${plural(players.length, 'player')}`}
            />
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
            <span className="text-[13px] text-muted-foreground">{plural(unassigned.length, 'player')}</span>
          </div>
          <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">{unassigned.map(row)}</section>
        </div>
      )}
      {footer}
    </>
  )
}
