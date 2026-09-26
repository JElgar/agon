import { Check } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
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
import { FootballScorersBySide } from './FootballScorersBySide'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
type MatchSide = components['schemas']['MatchSide']
type MatchLike = Match | FeedMatch | SearchMatch
type FootballGoalEvent = components['schemas']['FootballGoalEvent']

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

/** Sport-icon badge (pastel tint + stroke icon) — used for cricket. */
function SportIconBadge({ sport }: { sport: MatchType }) {
  const tint = SPORT_ICON_TINT[sport] ?? { bg: '#EAEFFC', stroke: '#1E3FA8' }
  return (
    <span
      className="flex size-10 shrink-0 items-center justify-center rounded-2xl"
      style={{ background: tint.bg }}
    >
      <CricketBatIcon stroke={tint.stroke} />
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

/** "1st"/"2nd"/"3rd"/"4th" — for the live cricket subtitle's innings ordinal
 *  (small enough here that pulling in a general-purpose ordinal formatter
 *  isn't worth it — cricket never has more than a handful of innings). */
function ordinal(n: number): string {
  if (n % 10 === 1 && n % 100 !== 11) return `${n}st`
  if (n % 10 === 2 && n % 100 !== 12) return `${n}nd`
  if (n % 10 === 3 && n % 100 !== 13) return `${n}rd`
  return `${n}th`
}
