import { Check } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Avatar } from './Avatar'
import { initials } from '@/lib/members'
import type { ScorePlayers } from '@/lib/members'
import { sportLabel, SPORT_ICON_TINT, type MatchType } from '@/lib/sports'
import { shortDate } from '@/lib/datetime'
import { cricketFormat, cricketFormatLabel, type CricketFormat } from '@/lib/matchFormat'
import {
  currentMinute,
  eventClockLabel,
  liveClockLabel,
  phaseFromState,
  phaseLabel,
  playerNameFor as footballPlayerNameFor,
  recentEvents,
  type FootballPeriodTimes,
  type FootballScore,
} from '@/lib/liveScore'
import {
  chaseInfoFromState,
  currentInnings,
  currentOverDeliveries,
  deliveryChipLabel,
  formatOvers,
  isChipHighlighted,
  topCricketPerformers,
  type CricketScore,
} from '@/lib/cricketScore'
import {
  currentMinute as netballCurrentMinute,
  describeEvent as describeNetballEvent,
  eventClockLabel as netballEventClockLabel,
  liveClockLabel as netballLiveClockLabel,
  phaseFromState as netballPhaseFromState,
  phaseLabel as netballPhaseLabel,
  recentEvents as recentNetballEvents,
  type NetballScore,
} from '@/lib/netballScore'
import { netballFormat } from '@/lib/matchFormat'
import { type SetsScore } from '@/lib/score'
import { FootballScorersBySide } from './FootballScorersBySide'
import { NetballScorersBySide } from './NetballScorersBySide'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
type MatchSide = components['schemas']['MatchSide']
type MatchLike = Match | FeedMatch | SearchMatch
type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type NetballGoalEvent = components['schemas']['NetballGoalEvent']

// ---------------------------------------------------------------------------
// Shared header — the 40×40 icon/avatar badge, title + subtitle, and the
// live pill on the right, per the "Agon redesign" canvas
// (`Tiles.dc.html`/`Main.dc.html`). Football and cricket each supply their
// own badge (team-initials avatar vs. a sport-icon tile) and subtitle text.
// ---------------------------------------------------------------------------

function LivePill({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex h-6 shrink-0 items-center gap-1.5 rounded-full bg-destructive px-2.5 text-[11px] font-bold tracking-wide text-destructive-foreground">
      <span className="size-1.5 rounded-full bg-destructive-foreground" />
      {children}
    </span>
  )
}

function CardHeader({
  badge,
  title,
  subtitle,
  live,
  onOpen,
}: {
  badge: React.ReactNode
  title: string
  subtitle: string
  live?: React.ReactNode
  onOpen?: () => void
}) {
  return (
    <button type="button" onClick={onOpen} className="flex w-full items-center gap-3 p-3.5 text-left">
      {badge}
      <div className="min-w-0 flex-1">
        <p className="truncate text-base font-bold leading-tight">{title}</p>
        <p className="truncate text-[13px] text-muted-foreground">{subtitle}</p>
      </div>
      {live && <div className="shrink-0">{live}</div>}
    </button>
  )
}

/** Team-initials avatar badge, football's stand-in for a club crest — see
 *  `MatchCard`'s doc comment on why this reuses the side name rather than
 *  inventing a "club" concept the schema doesn't have. */
function TeamInitialsBadge({ side }: { side: MatchSide | undefined }) {
  return (
    <span className="flex size-10 shrink-0 items-center justify-center rounded-2xl bg-primary text-[13px] font-bold text-primary-foreground">
      {initials(side?.name)}
    </span>
  )
}

/** Sport-icon badge (pastel tint + stroke icon) — used for every redesigned
 *  sport except football/netball, which use `TeamInitialsBadge` instead
 *  (team-based sports where a side's own name/crest is the more useful
 *  glyph). */
function SportIconBadge({ sport }: { sport: MatchType }) {
  const tint = SPORT_ICON_TINT[sport] ?? { bg: '#EAEFFC', stroke: '#1E3FA8' }
  return (
    <span
      className="flex size-10 shrink-0 items-center justify-center rounded-2xl"
      style={{ background: tint.bg }}
    >
      <SportGlyph sport={sport} stroke={tint.stroke} />
    </span>
  )
}

/** The bat/ball icon from the redesign canvas's cricket tiles — no lucide
 *  equivalent, so reused verbatim from `Tiles.dc.html`'s inline SVG. */
function CricketBatIcon({ stroke }: { stroke: string }) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke={stroke}
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M17 3l4 4-10 10-4-4z" />
      <path d="M7 13l-3.5 3.5a1.5 1.5 0 0 0 2 2L9 15" />
      <circle cx="18.5" cy="17.5" r="2" />
    </svg>
  )
}

/** The tennis-ball icon from the redesign canvas's tennis tiles — reused
 *  verbatim from `Tiles.dc.html`, same as `CricketBatIcon`. Also stands in
 *  for badminton, which the canvas has no mock for (see
 *  `TennisFeedCardBody`'s doc comment on that judgment call). */
function TennisBallIcon({ stroke }: { stroke: string }) {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke={stroke} strokeWidth="2" strokeLinecap="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M5.5 5.8a9 9 0 0 1 0 12.4M18.5 5.8a9 9 0 0 0 0 12.4" />
    </svg>
  )
}

/** The squash-racket icon from the redesign canvas's squash tile — reused
 *  verbatim from `Tiles.dc.html`. */
function SquashRacketIcon({ stroke }: { stroke: string }) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke={stroke}
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <ellipse cx="14" cy="9" rx="5.5" ry="6.5" transform="rotate(35 14 9)" />
      <path d="M10.2 14.2L4 20.5" />
      <circle cx="19" cy="19" r="1.6" />
    </svg>
  )
}

/** No mock covers badminton or table_tennis (see `RedesignedSportCard.tsx`'s
 *  top-level doc comment / the PR description for that judgment call) — this
 *  is a plain hand-drawn shuttlecock, in the same stroke style as the other
 *  sport badges, standing in until a real mock exists. */
function ShuttlecockIcon({ stroke }: { stroke: string }) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke={stroke}
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="17.5" r="1.8" />
      <path d="M12 15.7L8 6M12 15.7V5M12 15.7l4-9.7" />
      <path d="M8 6h8" />
    </svg>
  )
}

/** Table tennis's paddle-and-ball icon — a rounded rectangular bat (vs.
 *  squash's strung oval) to tell the two apart at a glance; same judgment
 *  call as `ShuttlecockIcon` above. */
function TableTennisIcon({ stroke }: { stroke: string }) {
  return (
    <svg
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke={stroke}
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect x="3.5" y="3" width="9" height="11" rx="2.5" transform="rotate(-25 8 8.5)" />
      <path d="M11.5 13l5.5 5.5" />
      <circle cx="18.5" cy="18.5" r="1.8" />
    </svg>
  )
}

/** Which sport-specific icon `SportIconBadge` draws — every sport that gets a
 *  pastel icon badge rather than `TeamInitialsBadge` (football, netball). */
function SportGlyph({ sport, stroke }: { sport: MatchType; stroke: string }) {
  switch (sport) {
    case 'cricket':
      return <CricketBatIcon stroke={stroke} />
    case 'tennis':
    case 'badminton':
      return sport === 'tennis' ? <TennisBallIcon stroke={stroke} /> : <ShuttlecockIcon stroke={stroke} />
    case 'squash':
      return <SquashRacketIcon stroke={stroke} />
    case 'table_tennis':
      return <TableTennisIcon stroke={stroke} />
    default:
      return <CricketBatIcon stroke={stroke} />
  }
}

/** A side's colored score-row marker — filled primary square for the
 *  leading/winning side, a bordered white square for the other. Shared shape
 *  between football's square markers and could extend to other sports later. */
function SideMarker({ leading }: { leading: boolean }) {
  return (
    <span
      className={cn(
        'size-3.5 shrink-0 rounded-[5px]',
        leading ? 'bg-primary' : 'border-2 border-muted-foreground/30 bg-card',
      )}
    />
  )
}

function WinnerCheck() {
  return <Check className="size-4 shrink-0 text-primary" aria-label="Winner" />
}

// ---------------------------------------------------------------------------
// Football
// ---------------------------------------------------------------------------

export function FootballFeedCardBody({
  match,
  sideA,
  sideB,
  nameA,
  nameB,
  isLive,
  liveState,
  finishedHeadline,
  finishedGoals,
  finishedPlayers,
  finishedPeriodTimes,
  aWon,
  bWon,
  startsAt,
  onOpen,
}: {
  match: MatchLike
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  nameA: string
  nameB: string
  isLive: boolean
  liveState: FootballScore | null
  /** The confirmed/pending goal tally by side id, for a finished match — the
   *  same map `displayScore`/`headlineBySide` already produce for the
   *  generic score box, since a finished football match's score comes from
   *  `match.confirmed_score`/`pending_score`, not the live poll. */
  finishedHeadline?: Record<string, number>
  finishedGoals: FootballGoalEvent[] | null
  finishedPlayers?: ScorePlayers
  finishedPeriodTimes?: FootballPeriodTimes
  aWon?: boolean
  bWon?: boolean
  startsAt: string
  onOpen?: () => void
}) {
  const goalsForLive = (sideId: string | undefined) =>
    liveState && sideId ? (liveState.score[sideId] ?? 0) : 0
  const goalsA = isLive ? goalsForLive(sideA?.id) : (finishedHeadline?.[sideA?.id ?? ''] ?? 0)
  const goalsB = isLive ? goalsForLive(sideB?.id) : (finishedHeadline?.[sideB?.id ?? ''] ?? 0)
  const aLeading = isLive ? goalsA > goalsB : !!aWon
  const bLeading = isLive ? goalsB > goalsA : !!bWon

  const subtitle = isLive
    ? liveState
      ? `${sportLabel('football')} · ${phaseLabel(phaseFromState(liveState))}`
      : sportLabel('football')
    : `${sportLabel('football')} · ${shortDate(startsAt)}`

  const minute = liveState ? currentMinute(liveState) : null
  const progressPct = minute !== null ? Math.max(0, Math.min(100, (minute / 90) * 100)) : null

  const latestGoals = liveState
    ? recentEvents(liveState, 6)
        .filter((e) => e.kind === 'goal' || e.kind === 'own_goal' || e.kind === 'penalty')
        .slice(0, 2)
    : []

  return (
    <>
      <CardHeader
        badge={<TeamInitialsBadge side={sideA} />}
        title={match.name}
        subtitle={subtitle}
        live={isLive ? <LivePill>{liveState ? liveClockLabel(liveState) : 'LIVE'}</LivePill> : undefined}
        onOpen={onOpen}
      />

      <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-4 text-left">
        {!isLive && (
          <p className="mb-1 text-right text-xs font-semibold text-muted-foreground">Full time</p>
        )}
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center gap-3">
            <SideMarker leading={aLeading} />
            <span
              className={cn(
                'flex min-w-0 flex-1 items-center gap-1.5 truncate text-lg',
                aLeading ? 'font-bold' : 'font-medium text-muted-foreground',
              )}
            >
              <span className="truncate">{nameA}</span>
              {!isLive && aWon && <WinnerCheck />}
            </span>
            <span
              className={cn(
                'font-display leading-none',
                aLeading ? 'text-4xl font-extrabold' : 'text-4xl font-extrabold text-muted-foreground',
              )}
            >
              {goalsA}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <SideMarker leading={bLeading} />
            <span
              className={cn(
                'flex min-w-0 flex-1 items-center gap-1.5 truncate text-lg',
                bLeading ? 'font-bold' : 'font-medium text-muted-foreground',
              )}
            >
              <span className="truncate">{nameB}</span>
              {!isLive && bWon && <WinnerCheck />}
            </span>
            <span
              className={cn(
                'font-display leading-none',
                bLeading ? 'text-4xl font-extrabold' : 'text-4xl font-extrabold text-muted-foreground',
              )}
            >
              {goalsB}
            </span>
          </div>
        </div>

        {isLive && progressPct !== null && (
          <div className="mt-3 h-1 overflow-hidden rounded-full bg-muted">
            <div className="h-1 rounded-full bg-destructive" style={{ width: `${progressPct}%` }} />
          </div>
        )}

        {isLive && liveState && latestGoals.length > 0 && (
          <div className="mt-3 flex flex-col gap-2 rounded-xl bg-muted/50 p-3">
            <span className="text-xs font-bold tracking-wide text-muted-foreground">LATEST</span>
            {latestGoals.map((event, i) => {
              const scorer = footballPlayerNameFor(match, event.player_id, liveState.players)
              const assist = footballPlayerNameFor(match, event.assist_player_id, liveState.players)
              const isSideA = event.side_id === sideA?.id
              return (
                <div key={i} className="flex items-center gap-2.5 text-sm">
                  <span className="w-8 shrink-0 font-bold text-muted-foreground">
                    {eventClockLabel(event, liveState.period_times)}
                  </span>
                  <span
                    className={cn(
                      'size-2 shrink-0 rounded-[3px]',
                      isSideA ? 'bg-primary' : 'border border-muted-foreground/40 bg-card',
                    )}
                  />
                  <span className="truncate">
                    <b className="font-semibold">{scorer ?? 'Unknown'}</b> scores
                    {assist && <> · assist {assist}</>}
                  </span>
                </div>
              )
            })}
          </div>
        )}

        {!isLive && finishedGoals && (
          <FootballScorersBySide
            goals={finishedGoals}
            match={match}
            players={finishedPlayers}
            periodTimes={finishedPeriodTimes}
            sideA={sideA}
            sideB={sideB}
            className="mt-3 border-t-0 pt-0 text-[13px]"
          />
        )}
      </button>
    </>
  )
}

// ---------------------------------------------------------------------------
// Cricket
// ---------------------------------------------------------------------------

function BallStrip({ score, format }: { score: CricketScore; format: CricketFormat }) {
  const innings = currentInnings(score)
  if (!innings) return null
  const overBalls = currentOverDeliveries(score.recent_deliveries ?? [])
  const placeholders = Math.max(0, format.balls_per_over - overBalls.length)

  return (
    <div className="mt-3 flex items-center gap-2.5 rounded-xl bg-muted/50 p-3">
      <span className="shrink-0 text-[13px] font-semibold text-muted-foreground">This over</span>
      <div className="flex flex-1 justify-end gap-1.5">
        {overBalls.map((d, i) => {
          const kind = isChipHighlighted(d)
          return (
            <span
              key={i}
              className={cn(
                'flex size-[30px] shrink-0 items-center justify-center rounded-full text-[13px] font-bold',
                kind === 'boundary' && 'bg-primary text-primary-foreground',
                kind === 'wicket' && 'bg-destructive text-destructive-foreground',
                kind === null && (deliveryChipLabel(d) === '·' ? 'bg-card text-muted-foreground' : 'bg-card'),
              )}
            >
              {deliveryChipLabel(d)}
            </span>
          )
        })}
        {Array.from({ length: placeholders }).map((_, i) => (
          <span
            key={`ph-${i}`}
            className="size-[30px] shrink-0 rounded-full border-[1.5px] border-dashed border-muted-foreground/30"
          />
        ))}
      </div>
    </div>
  )
}

export function CricketFeedCardBody({
  match,
  sideA,
  sideB,
  nameA,
  nameB,
  isLive,
  liveState,
  finishedScore,
  aWon,
  bWon,
  description,
  startsAt,
  onOpen,
}: {
  match: MatchLike
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  nameA: string
  nameB: string
  isLive: boolean
  liveState: CricketScore | null
  finishedScore: CricketScore | null
  aWon?: boolean
  bWon?: boolean
  description: string | null
  startsAt: string
  onOpen?: () => void
}) {
  const format = cricketFormat(match.format)
  const activeScore = liveState ?? finishedScore
  const innings = (sideId: string | undefined) =>
    activeScore?.innings.find((i) => i.batting_side_id === sideId)

  const battingSideId = liveState ? currentInnings(liveState)?.batting_side_id : undefined
  const aBatting = isLive && battingSideId === sideA?.id
  const bBatting = isLive && battingSideId === sideB?.id
  const aHeadline = isLive ? aBatting : !!aWon
  const bHeadline = isLive ? bBatting : !!bWon

  const subtitle = isLive
    ? `${sportLabel('cricket')} · ${cricketFormatLabel(format)} · ${
        activeScore ? `${ordinal(activeScore.innings.length)} innings` : 'Live'
      }`
    : `${sportLabel('cricket')} · ${cricketFormatLabel(format)} · ${shortDate(startsAt)}`

  const chase =
    isLive && liveState
      ? chaseInfoFromState(match, { innings: liveState.innings, awaiting_next_innings: liveState.awaiting_next_innings ?? true }, format)
      : null

  const { topBat, topBowler } = finishedScore
    ? topCricketPerformers(finishedScore, match)
    : { topBat: null, topBowler: null }

  const row = (
    side: MatchSide | undefined,
    name: string,
    headline: boolean,
    batting: boolean,
    winner: boolean,
  ) => {
    const inn = innings(side?.id)
    return (
      <div className="flex items-baseline gap-2.5">
        <span
          className={cn(
            'flex min-w-0 flex-1 items-center gap-2 truncate text-[17px]',
            headline ? 'font-bold' : 'font-medium text-muted-foreground',
          )}
        >
          <span className="truncate">{name}</span>
          {!isLive && winner && <WinnerCheck />}
          {batting && <span className="size-2 shrink-0 rounded-[3px] bg-destructive" aria-label="Batting" />}
        </span>
        {inn && (
          <span className="shrink-0 text-[13px] text-muted-foreground">{formatOvers(inn.overs)} ov</span>
        )}
        <span
          className={cn(
            'font-display leading-none',
            headline ? 'text-4xl font-extrabold' : 'text-2xl font-bold text-muted-foreground',
          )}
        >
          {inn ? `${inn.runs}/${inn.wickets}` : 'Yet to bat'}
        </span>
      </div>
    )
  }

  return (
    <>
      <CardHeader
        badge={<SportIconBadge sport="cricket" />}
        title={match.name}
        subtitle={subtitle}
        live={isLive ? <LivePill>LIVE</LivePill> : undefined}
        onOpen={onOpen}
      />

      <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-4 text-left">
        <div className="flex flex-col gap-1.5">
          {row(sideA, nameA, aHeadline, aBatting, !!aWon)}
          {row(sideB, nameB, bHeadline, bBatting, !!bWon)}
        </div>

        {isLive && chase && (
          <p className="mt-2.5 text-sm text-foreground/80">
            Need <b className="font-bold text-foreground">{chase.runsNeeded} runs</b> from{' '}
            {chase.ballsRemaining} balls
          </p>
        )}

        {isLive && liveState && <BallStrip score={liveState} format={format} />}

        {!isLive && description && (
          <span className="mt-3 inline-flex h-[30px] items-center self-start rounded-full bg-primary/10 px-3 text-sm font-bold text-primary">
            {description}
          </span>
        )}

        {!isLive && (topBat || topBowler) && (
          <div className="mt-3 grid grid-cols-2 gap-2">
            {topBat && (
              <div className="flex flex-col gap-0.5 rounded-xl bg-muted/50 p-3">
                <span className="text-xs font-semibold text-muted-foreground">Top bat</span>
                <span className="text-[15px] font-bold">{topBat.name}</span>
                <span className="text-[13px] text-foreground/80">
                  {topBat.runs} off {topBat.balls}
                </span>
              </div>
            )}
            {topBowler && (
              <div className="flex flex-col gap-0.5 rounded-xl bg-muted/50 p-3">
                <span className="text-xs font-semibold text-muted-foreground">Top bowler</span>
                <span className="text-[15px] font-bold">{topBowler.name}</span>
                <span className="text-[13px] text-foreground/80">
                  {topBowler.wickets} for {topBowler.runsConceded}
                </span>
              </div>
            )}
          </div>
        )}
      </button>
    </>
  )
}

// ---------------------------------------------------------------------------
// Netball
// ---------------------------------------------------------------------------
//
// No mock for netball exists in the "Agon redesign" canvas — this reuses
// football's card treatment verbatim (icon badge, colored-square score rows,
// live "LATEST" panel), per James's ask, since netball is structurally the
// same two-side goal-tally sport as football (`NetballScore`/
// `NetballMatchBlock`/`NetballScorersBySide` already mirror their football
// equivalents field-for-field — see those files' own doc comments). The one
// difference from `FootballFeedCardBody`: a netball goal has no assist to
// show, and the progress bar's denominator is the match's own configured
// quarters/length rather football's fixed 90 minutes.

export function NetballFeedCardBody({
  match,
  sideA,
  sideB,
  nameA,
  nameB,
  isLive,
  liveState,
  finishedHeadline,
  finishedGoals,
  finishedPlayers,
  finishedPeriodTimes,
  aWon,
  bWon,
  startsAt,
  onOpen,
}: {
  match: MatchLike
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  nameA: string
  nameB: string
  isLive: boolean
  liveState: NetballScore | null
  finishedHeadline?: Record<string, number>
  finishedGoals: NetballGoalEvent[] | null
  finishedPlayers?: ScorePlayers
  finishedPeriodTimes?: Record<string, string>
  aWon?: boolean
  bWon?: boolean
  startsAt: string
  onOpen?: () => void
}) {
  const goalsForLive = (sideId: string | undefined) =>
    liveState && sideId ? (liveState.score[sideId] ?? 0) : 0
  const goalsA = isLive ? goalsForLive(sideA?.id) : (finishedHeadline?.[sideA?.id ?? ''] ?? 0)
  const goalsB = isLive ? goalsForLive(sideB?.id) : (finishedHeadline?.[sideB?.id ?? ''] ?? 0)
  const aLeading = isLive ? goalsA > goalsB : !!aWon
  const bLeading = isLive ? goalsB > goalsA : !!bWon

  const subtitle = isLive
    ? liveState
      ? `${sportLabel('netball')} · ${netballPhaseLabel(netballPhaseFromState(liveState))}`
      : sportLabel('netball')
    : `${sportLabel('netball')} · ${shortDate(startsAt)}`

  const fmt = netballFormat(match.format)
  const totalMinutes = fmt.num_quarters * fmt.quarter_length_minutes
  const minute = liveState ? netballCurrentMinute(liveState) : null
  const progressPct =
    minute !== null && totalMinutes > 0 ? Math.max(0, Math.min(100, (minute / totalMinutes) * 100)) : null

  const latestGoals = liveState
    ? recentNetballEvents(liveState, 6)
        .filter((e) => e.kind === 'goal' || e.kind === 'two_point_goal')
        .slice(0, 2)
    : []

  return (
    <>
      <CardHeader
        badge={<TeamInitialsBadge side={sideA} />}
        title={match.name}
        subtitle={subtitle}
        live={isLive ? <LivePill>{liveState ? netballLiveClockLabel(liveState) : 'LIVE'}</LivePill> : undefined}
        onOpen={onOpen}
      />

      <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-4 text-left">
        {!isLive && (
          <p className="mb-1 text-right text-xs font-semibold text-muted-foreground">Full time</p>
        )}
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center gap-3">
            <SideMarker leading={aLeading} />
            <span
              className={cn(
                'flex min-w-0 flex-1 items-center gap-1.5 truncate text-lg',
                aLeading ? 'font-bold' : 'font-medium text-muted-foreground',
              )}
            >
              <span className="truncate">{nameA}</span>
              {!isLive && aWon && <WinnerCheck />}
            </span>
            <span
              className={cn(
                'font-display leading-none',
                aLeading ? 'text-4xl font-extrabold' : 'text-4xl font-extrabold text-muted-foreground',
              )}
            >
              {goalsA}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <SideMarker leading={bLeading} />
            <span
              className={cn(
                'flex min-w-0 flex-1 items-center gap-1.5 truncate text-lg',
                bLeading ? 'font-bold' : 'font-medium text-muted-foreground',
              )}
            >
              <span className="truncate">{nameB}</span>
              {!isLive && bWon && <WinnerCheck />}
            </span>
            <span
              className={cn(
                'font-display leading-none',
                bLeading ? 'text-4xl font-extrabold' : 'text-4xl font-extrabold text-muted-foreground',
              )}
            >
              {goalsB}
            </span>
          </div>
        </div>

        {isLive && progressPct !== null && (
          <div className="mt-3 h-1 overflow-hidden rounded-full bg-muted">
            <div className="h-1 rounded-full bg-destructive" style={{ width: `${progressPct}%` }} />
          </div>
        )}

        {isLive && liveState && latestGoals.length > 0 && (
          <div className="mt-3 flex flex-col gap-2 rounded-xl bg-muted/50 p-3">
            <span className="text-xs font-bold tracking-wide text-muted-foreground">LATEST</span>
            {latestGoals.map((event, i) => {
              const isSideA = event.side_id === sideA?.id
              return (
                <div key={i} className="flex items-center gap-2.5 text-sm">
                  <span className="w-8 shrink-0 font-bold text-muted-foreground">
                    {netballEventClockLabel(event, liveState.period_times)}
                  </span>
                  <span
                    className={cn(
                      'size-2 shrink-0 rounded-[3px]',
                      isSideA ? 'bg-primary' : 'border border-muted-foreground/40 bg-card',
                    )}
                  />
                  <span className="truncate">{describeNetballEvent(event, match, liveState.players)}</span>
                </div>
              )
            })}
          </div>
        )}

        {!isLive && finishedGoals && (
          <NetballScorersBySide
            goals={finishedGoals}
            match={match}
            players={finishedPlayers}
            periodTimes={finishedPeriodTimes}
            sideA={sideA}
            sideB={sideB}
            className="mt-3 border-t-0 pt-0 text-[13px]"
          />
        )}
      </button>
    </>
  )
}

// ---------------------------------------------------------------------------
// Racket sports — shared bits
// ---------------------------------------------------------------------------
//
// The schema has no explicit format field for these four sports the way
// football/cricket/netball each get a `MatchFormat` — a `Sets` score is just
// per-side games-per-set, with no "singles/doubles" or "best of N" alongside
// it. `racketFormatLabel` infers singles/doubles off roster size (the same
// `player_count` field `sidePlayerCountLabel` reads); "best of N" has no
// equivalent signal to infer from at all, so squash/table_tennis's subtitle
// just omits that segment (the mock's "Best of 5" is presentational, same
// judgment call as dropping its head-to-head footer note).

/** "Singles"/"Doubles" when both sides' roster size agrees on one of those,
 *  else omitted (a still-filling scheduled side, an uneven side count, or
 *  anything bigger than doubles). */
function racketFormatLabel(sideA: MatchSide | undefined, sideB: MatchSide | undefined): string | undefined {
  const a = sideA?.player_count
  const b = sideB?.player_count
  if (!a || !b || a !== b) return undefined
  if (a === 1) return 'Singles'
  if (a === 2) return 'Doubles'
  return undefined
}

/** Which side had the better set/game score at index `i` — `null` on a tie
 *  (a set/game that's tied or not yet played). Drives the per-column
 *  bold-vs-muted styling in both racket-sport bodies below, independent of
 *  who won the match overall (e.g. a set the eventual loser still took). */
function setLeader(setsA: number[], setsB: number[], i: number): 'a' | 'b' | null {
  const a = setsA[i] ?? 0
  const b = setsB[i] ?? 0
  if (a === b) return null
  return a > b ? 'a' : 'b'
}

const SET_COUNT_WORDS = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine']

/** "three" for 3, etc. — the feed summary reads "won in three sets", not "won
 *  in 3 sets" (matches the mock's copy). Falls back to the numeral past what
 *  a racket-sport match plausibly runs to. */
function setsCountWord(n: number): string {
  return SET_COUNT_WORDS[n] ?? String(n)
}

// ---------------------------------------------------------------------------
// Tennis / badminton — per-set column layout
// ---------------------------------------------------------------------------
//
// Badminton has no mock of its own in the canvas — it reuses tennis's
// treatment verbatim (per-set columns, winner check, "X won in N sets"
// summary) since it's scored the same way (best-of-N sets to a target score),
// per James's ask; see `isSetsSport` in `lib/sports.ts`, which already
// groups the two together.
//
// The mock's "live" variant (a `PTS` column with the current game score and
// a serving dot) has nothing behind it yet: unlike football/cricket/netball,
// there's no live-scoring backend for a `Sets` match, so `MatchCard` never
// has real point-by-point data to pass here and always calls this with
// `isLive: false` (see `MatchCard`'s `isCurrentlyLive`, which `isLiveSport`
// already excludes tennis/badminton from). The `isLive`/`live` props exist so
// the component matches the mock and is ready for when/if that lands — they
// just go unused by any real caller today.

export interface TennisLiveExtras {
  /** The current game's point score per side id, e.g. "30", "40", "AD" —
   *  shown in the `PTS` column badge. */
  points?: Record<string, string>
  /** The side currently serving, for the small dot next to their name. */
  servingSideId?: string
  /** The line under the score, e.g. "Rob serving · leads by a set and a
   *  break" — free text, since the exact phrasing (break of serve, etc.)
   *  needs real point-by-point state this app doesn't have yet. */
  summary?: string
}

export function TennisFeedCardBody({
  match,
  sideA,
  sideB,
  nameA,
  nameB,
  sport,
  score,
  aWon,
  bWon,
  isLive,
  live,
  startsAt,
  onOpen,
}: {
  match: MatchLike
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  nameA: string
  nameB: string
  sport: 'tennis' | 'badminton'
  /** The match's `Sets` score (confirmed/pending) — `null` before any sets
   *  are recorded. */
  score: SetsScore | null
  aWon?: boolean
  bWon?: boolean
  /** See this component's doc comment above — always `false` from the real
   *  `MatchCard` today. */
  isLive?: boolean
  live?: TennisLiveExtras
  startsAt: string
  onOpen?: () => void
}) {
  const setsA = score?.entries[sideA?.id ?? ''] ?? []
  const setsB = score?.entries[sideB?.id ?? ''] ?? []
  const setCount = Math.max(setsA.length, setsB.length)

  const formatLabel = racketFormatLabel(sideA, sideB)
  const subtitle = [
    sportLabel(sport),
    formatLabel,
    isLive ? (setCount > 0 ? `Set ${setCount}` : 'Live') : shortDate(startsAt),
  ]
    .filter(Boolean)
    .join(' · ')

  const winnerName = aWon ? nameA : bWon ? nameB : null

  const scoreRow = (side: MatchSide | undefined, name: string, sets: number[], winner: boolean, mine: 'a' | 'b') => (
    <div className="flex items-center gap-2">
      <span
        className={cn(
          'flex min-w-0 flex-1 items-center gap-1.5 truncate text-lg',
          winner ? 'font-bold' : 'font-medium text-muted-foreground',
        )}
      >
        <span className="truncate">{name}</span>
        {!isLive && winner && <WinnerCheck />}
        {isLive && live?.servingSideId === side?.id && (
          <span
            className="size-2 shrink-0 rounded-[3px] border border-lime-700 bg-lime-500"
            aria-label="Serving"
          />
        )}
      </span>
      {Array.from({ length: setCount }).map((_, i) => (
        <span
          key={i}
          className={cn(
            'font-display w-7 shrink-0 text-center text-[22px] leading-none',
            setLeader(setsA, setsB, i) === mine ? 'font-extrabold text-foreground' : 'font-semibold text-muted-foreground',
          )}
        >
          {sets[i] ?? '–'}
        </span>
      ))}
      {isLive && (
        <span className="flex h-8 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 font-display text-lg font-extrabold text-primary">
          {live?.points?.[side?.id ?? ''] ?? ''}
        </span>
      )}
    </div>
  )

  return (
    <>
      <CardHeader
        badge={<SportIconBadge sport={sport} />}
        title={match.name}
        subtitle={subtitle}
        live={isLive ? <LivePill>LIVE</LivePill> : undefined}
        onOpen={onOpen}
      />

      <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-4 text-left">
        {setCount > 0 && (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2 text-[11px] font-bold tracking-wide text-muted-foreground">
              <span className="min-w-0 flex-1" />
              {Array.from({ length: setCount }).map((_, i) => (
                <span key={i} className="w-7 shrink-0 text-center">{`S${i + 1}`}</span>
              ))}
              {isLive && <span className="w-9 shrink-0 text-center text-primary">PTS</span>}
            </div>
            {scoreRow(sideA, nameA, setsA, !!aWon, 'a')}
            {scoreRow(sideB, nameB, setsB, !!bWon, 'b')}
          </div>
        )}

        {!isLive && winnerName && setCount > 0 && (
          <div className="mt-3 flex items-center gap-2.5">
            <div className="flex">
              <Avatar name={nameA} size="sm" className="border-2 border-card" />
              <Avatar name={nameB} size="sm" className="-ml-2 border-2 border-card" />
            </div>
            <span className="text-sm text-muted-foreground">
              <b className="font-semibold text-foreground">{winnerName}</b> won in {setsCountWord(setCount)} sets
            </span>
          </div>
        )}

        {isLive && live?.summary && <p className="mt-2.5 text-sm text-muted-foreground">{live.summary}</p>}
      </button>
    </>
  )
}

// ---------------------------------------------------------------------------
// Squash / table_tennis — per-game pill layout
// ---------------------------------------------------------------------------
//
// Table tennis has no mock of its own either — it reuses squash's treatment
// (per-game score pills under the name, a standalone games-won total)
// instead of tennis's per-set columns, since it's traditionally scored the
// same way squash is: short race-to-N games rather than tennis's long
// games-within-a-set structure. Per James's ask; see this file's header
// comment / the PR description for the judgment call in one place.
//
// No live variant here — unlike tennis's mock, the canvas has no live squash
// tile, and neither sport has any live-scoring backend to drive one anyway
// (same gap `TennisFeedCardBody` notes above).

export function SquashFeedCardBody({
  match,
  sideA,
  sideB,
  nameA,
  nameB,
  sport,
  score,
  headline,
  aWon,
  bWon,
  startsAt,
  onOpen,
}: {
  match: MatchLike
  sideA: MatchSide | undefined
  sideB: MatchSide | undefined
  nameA: string
  nameB: string
  sport: 'squash' | 'table_tennis'
  /** The match's `Sets` score (confirmed/pending) — `null` before any games
   *  are recorded. Squash/table_tennis's "sets" are individual games. */
  score: SetsScore | null
  /** Games won per side — the same map `MatchCard`'s generic score box
   *  already derives via `headlineBySide` (a `Sets` score's headline is its
   *  count of sets/games won). */
  headline: Record<string, number>
  aWon?: boolean
  bWon?: boolean
  startsAt: string
  onOpen?: () => void
}) {
  const setsA = score?.entries[sideA?.id ?? ''] ?? []
  const setsB = score?.entries[sideB?.id ?? ''] ?? []
  const gameCount = Math.max(setsA.length, setsB.length)
  const subtitle = `${sportLabel(sport)} · ${shortDate(startsAt)}`

  const gameRow = (
    side: MatchSide | undefined,
    name: string,
    games: number[],
    winner: boolean,
    mine: 'a' | 'b',
  ) => (
    <div className="flex items-center gap-3">
      <div className="min-w-0 flex-1">
        <span
          className={cn(
            'flex items-center gap-2 truncate text-lg',
            winner ? 'font-bold' : 'font-medium text-muted-foreground',
          )}
        >
          <span className="truncate">{name}</span>
          {winner && <WinnerCheck />}
        </span>
        {games.length > 0 && (
          <div className="mt-1.5 flex gap-1.5">
            {games.map((v, i) => (
              <span
                key={i}
                className={cn(
                  'flex h-[26px] min-w-[30px] shrink-0 items-center justify-center rounded-lg px-1 text-[13px] font-bold',
                  setLeader(setsA, setsB, i) === mine
                    ? 'bg-foreground text-background'
                    : 'bg-muted text-muted-foreground',
                )}
              >
                {v}
              </span>
            ))}
          </div>
        )}
      </div>
      <span
        className={cn(
          'font-display shrink-0 text-4xl font-extrabold leading-none',
          winner ? 'text-foreground' : 'text-muted-foreground',
        )}
      >
        {headline[side?.id ?? ''] ?? 0}
      </span>
    </div>
  )

  return (
    <>
      <CardHeader
        badge={<SportIconBadge sport={sport} />}
        title={match.name}
        subtitle={subtitle}
        onOpen={onOpen}
      />

      <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-4 text-left">
        {gameCount > 0 && (
          <div className="flex flex-col gap-3.5">
            {gameRow(sideA, nameA, setsA, !!aWon, 'a')}
            <div className="h-px bg-border" />
            {gameRow(sideB, nameB, setsB, !!bWon, 'b')}
          </div>
        )}
      </button>
    </>
  )
}

/** "1st"/"2nd"/"3rd"/"4th" — for the live cricket subtitle's innings ordinal
 *  (small enough here that pulling in a general-purpose ordinal formatter
 *  isn't worth it — cricket never has more than a handful of innings). */
function ordinal(n: number): string {
  if (n % 10 === 1 && n % 100 !== 11) return `${n}st`
  if (n % 10 === 2 && n % 100 !== 12) return `${n}nd`
  if (n % 10 === 3 && n % 100 !== 13) return `${n}rd`
  return `${n}th`
}
