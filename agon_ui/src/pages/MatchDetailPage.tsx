import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { CalendarPlus, ChevronLeft, Clock, Link2, MailOpen, MapPin, MoreHorizontal, Pencil, Radio, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { scheduledDateTime } from '@/lib/datetime'
import { directionsUrl } from '@/lib/location'
import { downloadMatchIcs } from '@/lib/calendar'
import { Button } from '@/components/ui/button'
import { Avatar } from '@/components/agon/Avatar'
import { MatchHeaderCarousel } from '@/components/agon/MatchHeaderCarousel'
import { ScoreConfirmationBar } from '@/components/agon/ScoreConfirmationBar'
import { useLiveEvents } from '@/hooks/useLiveScore'
import { useMatchScore } from '@/hooks/useMatchScore'
import {
  footballScoreFrom,
  footballEventSourceFromScore,
  currentMinute,
  liveClockLabel,
} from '@/lib/liveScore'
import { netballScoreFrom, netballEventSourceFromScore } from '@/lib/netballScore'
import { cricketInningsFor, cricketScoreFrom, inningsDeliveriesFromEvents } from '@/lib/cricketScore'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import {
  displayScore,
  headlineBySide,
} from '@/lib/score'
import {
  canManageMatch,
  isMatchOwner,
  isParticipant,
  memberAvatarUrl,
  memberName,
  myPendingInvitation,
  mySideId,
  orderSidesForViewer,
  withInvitationStatus,
} from '@/lib/members'
import { respondToInvitation } from '@/lib/invitations'
import { MatchDetailsEditor } from '@/components/agon/MatchDetailsEditor'
import { MatchFormatCard } from '@/components/agon/MatchFormatCard'
import { MatchJoinSettingsEditor } from '@/components/agon/MatchJoinSettingsEditor'
import { MatchJoinLinksDialog } from '@/components/agon/MatchJoinLinksDialog'
import { TeamJoinBanner } from '@/components/agon/TeamJoinBanner'
import { JoinLinkBanner } from '@/components/agon/JoinLinkBanner'
import { MatchResultEditor } from '@/components/agon/MatchResultEditor'
import { MatchRosterEditor } from '@/components/agon/MatchRosterEditor'
import { InvitePlayers } from '@/components/agon/InvitePlayers'
import { MatchComments } from '@/components/agon/MatchComments'
import { useToggleLike } from '@/hooks/useToggleLike'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { InvitePromptDialog } from '@/components/agon/InvitePromptDialog'
import { useInvitePrompt } from '@/hooks/useInvitePrompt'
import { cricketFormat, footballFormat, netballFormat } from '@/lib/matchFormat'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import {
  AddEventButton,
  CommentsPreviewCard,
  FootballHeroCard,
  FootballPlayersTab,
  FootballScoreStrip,
  FootballTimeline,
  GoalsAssistsCard,
  KudosButton,
  MatchActionBar,
  PersonAvatar,
  MatchRulesRow,
  ScoreFlowCard,
  SideDisc,
  WhoPlayedCard,
  MatchTabBar,
  YourGameCard,
  primaryActionClass,
} from '@/components/agon/football/FootballMatchView'
import {
  AtTheCreaseCard,
  CricketHeroCard,
  CricketPlayersTab,
  CricketScoreStrip,
  CricketScorecardTab,
  CricketTimeline,
  RunsOverTimeCard,
} from '@/components/agon/cricket/CricketMatchView'
import { CRICKET_TABS, cricketRulesSummary } from '@/components/agon/cricket/cricketMeta'
import {
  NetballPlayersTab,
  NetballScoreFlowCard,
  NetballTimeline,
  QuarterScoresCard,
  TopScorersCard,
} from '@/components/agon/netball/NetballMatchView'
import { netballLiveLabel, netballRulesSummary } from '@/components/agon/netball/netballMeta'
import { RosterTab, SetsCard } from '@/components/agon/sets/SetsMatchView'
import { singlesPlayers } from '@/components/agon/sets/singles'
import { sportLabel as sportName } from '@/lib/sports'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Display label for a side: the server-resolved name (always present), or a
 *  neutral fallback for the unlikely case it's missing. */
function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}


type MatchTab = 'summary' | 'scorecard' | 'timeline' | 'players'
const BASIC_TABS: { id: MatchTab; label: string }[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'players', label: 'Players' },
]
const FOOTBALL_TABS: { id: MatchTab; label: string }[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'players', label: 'Players' },
]


/** Full match view: score (with confirm/dispute when pending), sides + rosters.
 *  Participants get inline editing of details/result, plus invite and cancel. */
/** "2 × 40min · extra time · penalties" — the football "Match rules" row. */
function footballRulesSummary(match: Match): string {
  const fmt = footballFormat(match.format)
  const parts = [`${fmt.num_halves} × ${fmt.half_length_minutes} min`]
  if (fmt.extra_time) parts.push('extra time')
  if (fmt.penalties) parts.push('penalties')
  return parts.join(' · ')
}

/** Native share sheet, else copy the link (else show it to copy by hand). */
async function shareMatch(match: Match) {
  const url = `${window.location.origin}/matches/${match.id}`
  if (navigator.share) {
    try {
      await navigator.share({ title: match.name, url })
      return
    } catch {
      // Dismissed or failed; fall through to copying.
    }
  }
  try {
    await navigator.clipboard.writeText(url)
  } catch {
    window.prompt('Copy this match link:', url)
  }
}

export function MatchDetailPage() {
  const { matchId } = useParams()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const query = useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  if (query.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-2xl border bg-card" aria-hidden />
      </div>
    )
  }

  if (query.isError || !query.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this match.</p>
        <Button variant="outline" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  return (
    <MatchDetail
      match={query.data}
      currentUserId={currentUserId}
      onBack={() => navigate(-1)}
    />
  )
}

/** The loaded match view. Split out so editing state can use hooks without the
 *  loading/error guards sitting above them (hooks can't be conditional). */
function MatchDetail({
  match,
  currentUserId,
  onBack,
}: {
  match: Match
  currentUserId?: string
  onBack: () => void
}) {
  const [editingDetails, setEditingDetails] = useState(false)
  const [editingResult, setEditingResult] = useState(false)
  const [editingRoster, setEditingRoster] = useState(false)
  const [inviting, setInviting] = useState(false)
  // The tab bar: Summary/Timeline/Players for football and netball, cricket
  // adds Scorecard, and sports without live scoring get Summary/Players.
  const [matchTab, setMatchTab] = useState<MatchTab>('summary')
  const [commentsOpen, setCommentsOpen] = useState(false)
  const [rulesOpen, setRulesOpen] = useState(false)
  const toggleLike = useToggleLike(match)
  const navigate = useNavigate()

  // Owner or admin only, mirroring the server's `caller_is_match_admin` — an
  // ordinary player is read-only on the match itself; `LeaveMatch` below is
  // the one action left to them.
  const canEdit = canManageMatch(match, currentUserId)
  const iAmOwner = isMatchOwner(match, currentUserId)
  const iAmParticipant = isParticipant(match, currentUserId)
  const cancelled = match.status === 'cancelled'
  const isLiveSport =
    match.match_type === 'football' || match.match_type === 'cricket' || match.match_type === 'netball'

  // The viewer's own side, when they're playing, reads first in the
  // calendar entry's "A vs B" — mirrors the feed card (`MatchCard`). The page
  // itself keeps the stored side order so each side's kit colour is stable.
  const orderedSides = orderSidesForViewer(match.sides, mySideId(match, currentUserId))
  const [sideA, sideB] = orderedSides
  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')
  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}

  // Live, in-progress score only — a completed match reads its scorecard
  // straight off `confirmed_score`/`pending_score` instead (see
  // `footballEventSource`/`cricketInnings` below), which carries the same
  // goals/cards/subs (football) or batting/bowling/extras (cricket) a
  // live-scored or manually-entered result produces — it's the same `Score`
  // shape either way. Takes over the score block below (and the entry
  // button becomes "Continue scoring") while a match is actually in
  // progress — see `footballState`/`cricketState`.
  const scoreQuery = useMatchScore(match.id, {
    enabled: isLiveSport && match.status === 'in_progress',
    refetchInterval: match.status === 'in_progress' ? 15000 : undefined,
  })
  const isCurrentlyLive = isLiveSport && match.status === 'in_progress'
  const footballState = isCurrentlyLive ? footballScoreFrom(scoreQuery.data) : null
  const cricketState = isCurrentlyLive ? cricketScoreFrom(scoreQuery.data) : null
  const netballState = isCurrentlyLive ? netballScoreFrom(scoreQuery.data) : null
  const hasLiveState = !!footballState || !!cricketState || !!netballState
  // Same "live while in progress, else confirmed/pending" source as
  // `cricketScoreInnings` below — the hero card and the score's resolved
  // player names.
  const cricketScore = cricketScoreFrom(isCurrentlyLive ? scoreQuery.data : scoreInfo?.score)
  // The timeline's events: the live running score while the
  // match is in progress, else straight off the confirmed/pending score —
  // stays visible once the match is completed, unlike `footballState` above.
  const footballEventSource = footballEventSourceFromScore(
    isCurrentlyLive ? scoreQuery.data : scoreInfo?.score,
  )
  const netballEventSource = netballEventSourceFromScore(
    isCurrentlyLive ? scoreQuery.data : scoreInfo?.score,
  )
  // Quarter-by-quarter breakdown reads the same way regardless of which of
  // netball's two live-scoring methods produced the score — shown even for a
  // quarter-only-scored match with no goal-by-goal detail at all.
  const netballQuarterScore = isCurrentlyLive ? netballState : netballScoreFrom(scoreInfo?.score ?? null)
  // Football's and netball's setup screens also gate starting the clock;
  // cricket has no equivalent preferences step, so it goes straight into
  // scoring. Netball's "which live-scoring method" choice lives inline on
  // its own live page instead of a separate setup route (see
  // `NetballLiveScoringPage`).
  const liveEntryPath =
    match.match_type === 'cricket' || match.match_type === 'netball'
      ? `/matches/${match.id}/live`
      : `/matches/${match.id}/live/setup`

  // Each cricket innings' deliveries, folded in from the raw live event log
  // by matching innings order, for the run-rate graph — the score's own
  // `recent_deliveries` is bounded to the current innings' last 18 balls,
  // not the full match, so the graph needs the raw log instead. The event
  // log has no size ceiling and stays fully readable regardless of match
  // status, so it's fetched here independently of `scoreQuery`.
  const liveEvents = useLiveEvents(match.id, { enabled: match.match_type === 'cricket' })
  const cricketScoreInnings = cricketInningsFor(isCurrentlyLive ? scoreQuery.data : scoreInfo?.score)
  const liveInningsDeliveries = liveEvents.data ? inningsDeliveriesFromEvents(liveEvents.data) : undefined
  const cricketInnings = cricketScoreInnings?.map((inn, i) => ({
    ...inn,
    deliveries: liveInningsDeliveries?.[i]?.deliveries ?? [],
  }))

  const isCricket = match.match_type === 'cricket'
  const isNetball = match.match_type === 'netball'
  const sportLabel = sportName(match.match_type)
  const cricketFmt = cricketFormat(match.format)
  const matchView: 'finished' | 'live' | 'scheduled' | 'cancelled' = cancelled
    ? 'cancelled'
    : footballState || cricketState || netballState || match.status === 'in_progress'
      ? 'live'
      : scoreInfo
        ? 'finished'
        : 'scheduled'
  // Football keeps the sides in their stored order (not viewer-first) so each
  // side's kit colour stays the same for everyone looking at the match.
  const [kitSideA, kitSideB] = match.sides
  // Goals for the hero card: football's or netball's live tally, else the result.
  const liveTally = footballState?.score ?? netballState?.score
  const footballGoalsA = (liveTally ?? headline)[kitSideA?.id ?? ''] ?? 0
  const footballGoalsB = (liveTally ?? headline)[kitSideB?.id ?? ''] ?? 0
  // Sports without live scoring (racket sports, other) have just a result:
  // sets or points, entered by hand.
  const singles = singlesPlayers(match)
  const setsResult = (() => {
    if (!scoreInfo || scoreInfo.score.type !== 'Sets' || !kitSideA || !kitSideB) return undefined
    const a = headline[kitSideA.id] ?? 0
    const b = headline[kitSideB.id] ?? 0
    if (a === b) return 'Level on sets'
    const [w, hi, lo] = a > b ? [kitSideA, a, b] : [kitSideB, b, a]
    return `${sideName(w, 'Winner')} won ${hi}–${lo}`
  })()
  const sideResults: Record<string, string> | undefined =
    scoreInfo && kitSideA && kitSideB
      ? footballGoalsA === footballGoalsB
        ? { [kitSideA.id]: 'Drew', [kitSideB.id]: 'Drew' }
        : footballGoalsA > footballGoalsB
          ? { [kitSideA.id]: 'Won', [kitSideB.id]: 'Lost' }
          : { [kitSideA.id]: 'Lost', [kitSideB.id]: 'Won' }
      : undefined
  const myPlayer = match.players.find((p) => p.member.type === 'User' && p.member.user_id === currentUserId)
  const metaTeamSide = match.sides.find((s) => s.team_name)

  const moreMenu = (variant: 'overlay' | 'plain') => (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="More options"
          className={cn(
            'flex size-11 items-center justify-center rounded-full',
            variant === 'overlay' ? 'bg-card/95 text-foreground' : 'text-foreground hover:bg-muted',
          )}
        >
          <MoreHorizontal className="size-[22px]" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuItem
          onSelect={() => downloadMatchIcs(match, { title: match.name, description: `${nameA} vs ${nameB}` })}
        >
          <CalendarPlus className="size-4" /> Add to calendar
        </DropdownMenuItem>
        {canEdit && !cancelled && (
          <DropdownMenuItem onSelect={() => setEditingDetails(true)}>
            <Pencil className="size-4" /> Edit match details
          </DropdownMenuItem>
        )}
        {canEdit && isLiveSport && !cancelled && match.status !== 'completed' && (
          <DropdownMenuItem onSelect={() => navigate(liveEntryPath)}>
            <Radio className="size-4" /> {hasLiveState ? 'Continue scoring' : 'Score live'}
          </DropdownMenuItem>
        )}
        {canEdit && !cancelled && !hasLiveState && (
          <DropdownMenuItem
            onSelect={() => {
              setMatchTab('summary')
              setEditingResult(true)
            }}
          >
            <Pencil className="size-4" /> {scoreInfo ? 'Edit result' : 'Add result'}
          </DropdownMenuItem>
        )}
        {canEdit && !cancelled && (
          <DropdownMenuItem
            onSelect={() => {
              setMatchTab('players')
              setEditingRoster(true)
            }}
          >
            <UserPlus className="size-4" /> Edit roster
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )

  // Roster admin under every redesigned Players tab.
  const playersFooter = (
    <>
      {canEdit && !cancelled && (
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" className="gap-1.5 rounded-full" onClick={() => setEditingRoster(true)}>
            <Pencil className="size-4" /> Edit roster
          </Button>
          {!inviting && (
            <Button variant="outline" className="gap-1.5 rounded-full" onClick={() => setInviting(true)}>
              <UserPlus className="size-4" /> Invite players
            </Button>
          )}
          <MatchJoinLinksDialog match={match}>
            <Button variant="outline" className="gap-1.5 rounded-full">
              <Link2 className="size-4" /> Join links
            </Button>
          </MatchJoinLinksDialog>
        </div>
      )}
      {canEdit && !cancelled && inviting && (
        <InvitePlayers match={match} onDone={() => setInviting(false)} />
      )}
      {!cancelled && (
        <WaitlistSection match={match} currentUserId={currentUserId} canManage={canEdit} />
      )}
    </>
  )

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
        <>
          {/* Every sport follows the redesign mocks: `Match.dc.html` (photo
              hero, live cricket), `MatchFootball.dc.html` /
              `EventsFootball.dc.html` / `PlayersFootball.dc.html` and
              `EventsCricket.dc.html`; sports without mocks reuse the same
              pieces. Admin actions live in the ⋯ menu instead of crowding the
              page. */}
          {match.header_photos.length > 0 && matchTab === 'summary' ? (
            <div className="relative -mx-4 -mt-8 h-[250px] overflow-hidden md:mx-0 md:mt-0 md:rounded-[20px]">
              <MatchHeaderCarousel photos={match.header_photos} hero />
              <button
                type="button"
                onClick={onBack}
                aria-label="Back"
                className="absolute top-4 left-4 flex size-11 items-center justify-center rounded-full bg-card/95 text-foreground"
              >
                <ChevronLeft className="size-[22px]" strokeWidth={2.2} />
              </button>
              <div className="absolute top-4 right-4">{moreMenu('overlay')}</div>
            </div>
          ) : (
            <div className="-mx-2 -mt-5 flex items-center justify-between">
              <button
                type="button"
                onClick={onBack}
                aria-label="Back"
                className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted"
              >
                <ChevronLeft className="size-[22px]" strokeWidth={2.2} />
              </button>
              {moreMenu('plain')}
            </div>
          )}

          {editingDetails && <MatchDetailsEditor match={match} onDone={() => setEditingDetails(false)} />}

          {matchTab === 'summary' ? (
            <>
              <div className={cn('flex flex-col px-1', match.header_photos.length > 0 ? 'gap-1.5 pt-1' : 'gap-2')}>
                <div className="flex items-center gap-2.5">
                  {matchView === 'live' ? (
                    <span className="flex h-6 items-center gap-1.5 rounded-full bg-destructive px-[9px] text-[11px] font-bold tracking-[0.6px] text-destructive-foreground">
                      <span className="size-1.5 rounded-full bg-destructive-foreground" />
                      LIVE
                    </span>
                  ) : (
                    metaTeamSide && (
                      <SideDisc side={metaTeamSide} index={match.sides.indexOf(metaTeamSide)} size={32} />
                    )
                  )}
                  <span className="truncate text-sm text-muted-foreground">
                    {metaTeamSide?.team_name ?? sportLabel} · {scheduledDateTime(match.starts_at)}
                  </span>
                </div>
                <h1 className="font-display text-[30px] leading-tight font-extrabold tracking-[-0.4px]">{match.name}</h1>
                {match.location && (
                  <p className="flex items-center gap-1 text-sm text-muted-foreground">
                    <MapPin className="size-3.5 shrink-0" />
                    <span className="truncate">{match.location.text}</span>
                    {directionsUrl(match.location) && (
                      <a
                        href={directionsUrl(match.location)}
                        target="_blank"
                        rel="noreferrer"
                        className="shrink-0 font-semibold text-link hover:underline"
                      >
                        Directions
                      </a>
                    )}
                  </p>
                )}
              </div>
              {isCricket && cricketScore && (matchView === 'live' || matchView === 'finished') ? (
                <CricketHeroCard match={match} score={cricketScore} format={cricketFmt} live={matchView === 'live'} />
              ) : (
              <FootballHeroCard
                match={match}
                goalsA={footballGoalsA}
                goalsB={footballGoalsB}
                state={!isLiveSport && matchView === 'live' ? (scoreInfo ? 'finished' : 'scheduled') : matchView}
                finishedLabel={isCricket ? 'Result' : isLiveSport ? undefined : 'Final'}
                resultText={setsResult}
                avatars={
                  singles
                    ? [
                        <PersonAvatar key="a" name={memberName(singles[0].member)} imageUrl={memberAvatarUrl(singles[0].member)} size={44} />,
                        <PersonAvatar key="b" name={memberName(singles[1].member)} imageUrl={memberAvatarUrl(singles[1].member)} size={44} />,
                      ]
                    : undefined
                }
                liveLabel={
                  footballState ? liveClockLabel(footballState) : netballState ? netballLiveLabel(netballState) : 'Live'
                }
                kickoffLabel={`${match.match_type === 'football' ? 'Kick-off' : isNetball ? 'Centre pass' : 'Starts'} ${new Date(match.starts_at).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}`}
              />
              )}
            </>
          ) : isCricket && cricketScore && (matchView === 'live' || matchView === 'finished') ? (
            <CricketScoreStrip match={match} score={cricketScore} live={matchView === 'live'} />
          ) : (
            <FootballScoreStrip
              match={match}
              goalsA={footballGoalsA}
              goalsB={footballGoalsB}
              hasScore={matchView === 'finished' || matchView === 'live'}
            />
          )}

          <MatchTabBar tabs={isCricket ? CRICKET_TABS : isLiveSport ? FOOTBALL_TABS : BASIC_TABS} value={matchTab} onChange={setMatchTab} />

          {matchTab === 'summary' && (
            <>
              {myPendingInvitation(match, currentUserId) ? (
                <InviteBanner match={match} currentUserId={currentUserId} />
              ) : (
                match.pending_score && (
                  <ScoreConfirmationBar match={match} currentUserId={currentUserId} variant="detail" />
                )
              )}
              {!cancelled && <TeamJoinBanner match={match} />}
              {!cancelled && <JoinLinkBanner match={match} />}
              {editingResult && <MatchResultEditor match={match} onDone={() => setEditingResult(false)} />}

              {isCricket && cricketState && <AtTheCreaseCard match={match} score={cricketState} />}
              {isCricket && cricketInnings && (
                <RunsOverTimeCard match={match} innings={cricketInnings} format={cricketFmt} live={matchView === 'live'} />
              )}
              {scoreInfo && !isLiveSport && <SetsCard match={match} score={scoreInfo.score} />}
              {isNetball && netballQuarterScore && (
                <QuarterScoresCard match={match} score={netballQuarterScore} live={matchView === 'live'} />
              )}
              {isNetball && netballEventSource && <TopScorersCard match={match} detail={netballEventSource} />}
              {isNetball && netballQuarterScore && (
                <NetballScoreFlowCard
                  match={match}
                  score={netballQuarterScore}
                  format={netballFormat(match.format)}
                  live={matchView === 'live'}
                />
              )}
              {match.match_type === 'football' && myPlayer?.side_id && footballEventSource && (matchView === 'finished' || matchView === 'live') && (
                <YourGameCard match={match} me={myPlayer} detail={footballEventSource} />
              )}
              {match.match_type === 'football' && footballEventSource && <GoalsAssistsCard match={match} detail={footballEventSource} />}
              {match.match_type === 'football' && footballEventSource && (
                <ScoreFlowCard
                  match={match}
                  detail={footballEventSource}
                  nowMinute={footballState ? (currentMinute(footballState) ?? undefined) : undefined}
                />
              )}
              {!singles && <WhoPlayedCard match={match} title={matchView === 'scheduled' ? "Who's playing" : 'Who played'} />}
              <CommentsPreviewCard
                match={match}
                viewerName={myPlayer ? memberName(myPlayer.member) : undefined}
                viewerAvatar={myPlayer ? memberAvatarUrl(myPlayer.member) : undefined}
                onOpen={() => setCommentsOpen(true)}
              />
              {!isLiveSport ? null : rulesOpen ? (
                <MatchFormatCard match={match} canEdit={canEdit && !cancelled} />
              ) : (
                <MatchRulesRow
                  summary={
                    isCricket
                      ? cricketRulesSummary(cricketFmt)
                      : isNetball
                        ? netballRulesSummary(netballFormat(match.format))
                        : footballRulesSummary(match)
                  }
                  onClick={canEdit && !cancelled ? () => setRulesOpen(true) : undefined}
                />
              )}
              {canEdit && !cancelled && <MatchJoinSettingsEditor match={match} canManage={canEdit} />}
              {canEdit && !cancelled && <CancelMatch match={match} />}
              {iAmParticipant && !cancelled && <LeaveMatch match={match} isOwner={iAmOwner} />}
            </>
          )}

          {matchTab === 'scorecard' && isCricket && (
            <CricketScorecardTab
              match={match}
              innings={cricketInnings ?? []}
              players={cricketScore?.players}
              format={cricketFmt}
            />
          )}

          {matchTab === 'timeline' &&
            (isCricket ? (
              <CricketTimeline
                match={match}
                innings={cricketInnings ?? []}
                players={cricketScore?.players}
                format={cricketFmt}
                live={matchView === 'live'}
              />
            ) : isNetball ? (
              netballEventSource ? (
                <NetballTimeline
                  match={match}
                  detail={netballEventSource}
                  currentUserId={currentUserId}
                  finished={matchView === 'finished'}
                />
              ) : (
                <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
                  {matchView === 'scheduled'
                    ? 'Goals and fouls will show up here once the match starts.'
                    : 'No goal-by-goal events were recorded for this match.'}
                </section>
              )
            ) : footballEventSource ? (
              <FootballTimeline
                match={match}
                detail={footballEventSource}
                currentUserId={currentUserId}
                finished={matchView === 'finished'}
              />
            ) : (
              <section className="rounded-[20px] border bg-card px-[18px] py-6 text-center text-sm text-muted-foreground">
                {matchView === 'scheduled'
                  ? 'Goals, cards and subs will show up here once the match kicks off.'
                  : 'No events were recorded for this match.'}
              </section>
            ))}

          {matchTab === 'players' &&
            (editingRoster ? (
              <MatchRosterEditor match={match} onDone={() => setEditingRoster(false)} />
            ) : !isLiveSport ? (
              <RosterTab match={match} currentUserId={currentUserId} result={sideResults} footer={playersFooter} />
            ) : isNetball ? (
              <NetballPlayersTab
                match={match}
                detail={netballEventSource}
                currentUserId={currentUserId}
                scoreA={footballGoalsA}
                scoreB={footballGoalsB}
                finished={matchView === 'finished'}
                footer={playersFooter}
              />
            ) : isCricket ? (
              <CricketPlayersTab
                match={match}
                innings={cricketScore?.innings ?? []}
                currentUserId={currentUserId}
                format={cricketFmt}
                finished={matchView === 'finished'}
                footer={playersFooter}
              />
            ) : (
              <FootballPlayersTab
                match={match}
                detail={footballEventSource}
                currentUserId={currentUserId}
                goalsA={footballGoalsA}
                goalsB={footballGoalsB}
                finished={matchView === 'finished'}
                footer={playersFooter}
              />
            ))}

          <MatchActionBar
            primary={
              canEdit && !cancelled && !isLiveSport && !scoreInfo ? (
                <button
                  type="button"
                  className={primaryActionClass}
                  onClick={() => {
                    setMatchTab('summary')
                    setEditingResult(true)
                  }}
                >
                  <Pencil className="size-5" /> Add result
                </button>
              ) : !isLiveSport ? (
                matchView === 'scheduled' ? (
                  <button
                    type="button"
                    className={primaryActionClass}
                    onClick={() => downloadMatchIcs(match, { title: match.name, description: `${nameA} vs ${nameB}` })}
                  >
                    <CalendarPlus className="size-5" /> Add to calendar
                  </button>
                ) : (
                  <KudosButton liked={match.social.i_liked} onToggle={() => toggleLike.mutate(!match.social.i_liked)} />
                )
              ) : canEdit && !cancelled && !isCricket && matchTab === 'timeline' && match.status === 'in_progress' ? (
                <AddEventButton to={liveEntryPath} />
              ) : canEdit && !cancelled && match.status === 'in_progress' ? (
                <Link to={liveEntryPath} className={primaryActionClass}>
                  Update score
                </Link>
              ) : canEdit && !cancelled && match.status === 'scheduled' ? (
                <Link to={liveEntryPath} className={primaryActionClass}>
                  <Radio className="size-5" /> Start scoring
                </Link>
              ) : matchView === 'scheduled' ? (
                <button
                  type="button"
                  className={primaryActionClass}
                  onClick={() => downloadMatchIcs(match, { title: match.name, description: `${nameA} vs ${nameB}` })}
                >
                  <CalendarPlus className="size-5" /> Add to calendar
                </button>
              ) : (
                <KudosButton liked={match.social.i_liked} onToggle={() => toggleLike.mutate(!match.social.i_liked)} />
              )
            }
            commentCount={match.social.comment_count}
            onComments={() => setCommentsOpen(true)}
            onShare={() => shareMatch(match)}
          />

          <Sheet open={commentsOpen} onOpenChange={setCommentsOpen}>
            <SheetContent side="bottom" className="max-h-[85vh] overflow-y-auto rounded-t-[20px] bg-background p-4">
              <SheetHeader className="p-0">
                <SheetTitle className="font-display text-xl">Comments</SheetTitle>
              </SheetHeader>
              <MatchComments matchId={match.id} currentUserId={currentUserId} />
            </SheetContent>
          </Sheet>
        </>
    </div>
  )
}

/**
 * Shown when the signed-in viewer has a pending invitation to this match: a
 * prominent Accept/Decline banner, plus (the first time this match page is
 * opened while the invite is pending — see `useInvitePrompt`) a popup
 * fronting the same choice immediately. Both open the shared response
 * dialog, wired to `POST /invitations/:id/respond`; accepting also offers to
 * confirm the match's score in the same step when one is already pending on
 * the viewer's side. On success it refreshes the match (so the roster/badge/
 * score update) and the notification badge (the matching invite notification
 * is now handled).
 */
function InviteBanner({
  match,
  currentUserId,
}: {
  match: Match
  currentUserId?: string
}) {
  const queryClient = useQueryClient()
  const invitation = myPendingInvitation(match, currentUserId)
  const matchKey = ['match', match.id]
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)
  const [promptOpen, setPromptOpen] = useInvitePrompt(invitation?.id ?? null)

  const respond = useMutation({
    mutationFn: async (
      response: components['schemas']['InvitationResponse'],
    ) => {
      if (!invitation) return
      await respondToInvitation(invitation.id, response)
    },
    // Optimistically flip the viewer's invitation status in the match cache so
    // the banner (and the "You're invited" badge) disappear immediately, without
    // waiting for the round-trip or a refresh.
    onMutate: async (response) => {
      if (!currentUserId) return
      await queryClient.cancelQueries({ queryKey: matchKey })
      const previous = queryClient.getQueryData<Match>(matchKey)
      const status = response === 'accepted' ? 'accepted' : 'declined'
      if (previous) {
        queryClient.setQueryData<Match>(
          matchKey,
          withInvitationStatus(previous, currentUserId, status),
        )
      }
      return { previous }
    },
    // Roll back the optimistic patch if the request fails.
    onError: (_err, _response, context) => {
      if (context?.previous) {
        queryClient.setQueryData(matchKey, context.previous)
      }
    },
    // Reconcile with the server regardless of outcome, and refresh notifications
    // (the invite notification is now handled) and the feed (roster changed).
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: matchKey })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({
        queryKey: ['notifications-unread-count'],
      })
    },
  })

  if (!invitation) return null

  // Shared by both the banner's confirm dialog and the first-open popup —
  // covers the score-confirm sub-step, which the mutation above doesn't know
  // about (it only reconciles the invitation response itself).
  const handleResponded = () => {
    setAction(null)
    queryClient.invalidateQueries({ queryKey: matchKey })
    queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
  }

  return (
    <>
      <div className="rounded-2xl border border-primary/30 bg-primary/5 p-4">
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
            <MailOpen className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium">You've been invited to this match</p>
            <p className="text-xs text-muted-foreground">
              Accept to join the roster, or decline if you can't make it.
            </p>
            <div className="mt-3 flex gap-2">
              <Button size="sm" onClick={() => setAction('accept')}>
                Accept
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAction('decline')}
              >
                Decline
              </Button>
            </div>
          </div>
        </div>
      </div>

      <InvitationResponseDialog
        open={action !== null}
        onOpenChange={(open) => !open && setAction(null)}
        action={action}
        name={match.name}
        matchId={match.id}
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={handleResponded}
      />

      <InvitePromptDialog
        open={promptOpen}
        onOpenChange={setPromptOpen}
        name={match.name}
        matchId={match.id}
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={handleResponded}
      />
    </>
  )
}

/**
 * Who's queued for a spot on this match/side that wasn't free when they
 * tried to join or accept an invite onto it — `GET /matches/:id/waitlist`,
 * longest-waiting first. Renders nothing while empty; a match with no one
 * waiting shouldn't carry an always-there "no one waiting" placeholder.
 *
 * `canManage` (mirrors `MatchDetailPage`'s own `canEdit`) gates "Move in" —
 * admin-only, and only works once a spot is actually free (the same typed
 * conflict `join` returns). Removing an entry ("Leave"/"Remove") is open to
 * the waiting player themselves or a match admin, same split the server
 * enforces.
 */
function WaitlistSection({
  match,
  currentUserId,
  canManage,
}: {
  match: Match
  currentUserId?: string
  canManage: boolean
}) {
  const queryClient = useQueryClient()
  const waitlistKey = ['waitlist', match.id]

  const query = useQuery({
    queryKey: waitlistKey,
    queryFn: async (): Promise<components['schemas']['WaitlistEntry'][]> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/waitlist', {
        params: { path: { match_id: match.id } },
      })
      if (error || !data) throw new Error('Failed to load the waitlist')
      return data
    },
  })

  const moveIn = useMutation({
    mutationFn: async (userId: string) => {
      const { error } = await fetchClient.POST(
        '/matches/{match_id}/waitlist/{user_id}/move-in',
        { params: { path: { match_id: match.id, user_id: userId } } },
      )
      if (error) throw new Error('Failed to move that player in')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: waitlistKey })
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })

  const leave = useMutation({
    mutationFn: async (userId: string) => {
      const { error } = await fetchClient.DELETE('/matches/{match_id}/waitlist/{user_id}', {
        params: { path: { match_id: match.id, user_id: userId } },
      })
      if (error) throw new Error('Failed to remove that waitlist entry')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: waitlistKey })
    },
  })

  const entries = query.data ?? []
  if (entries.length === 0) return null

  return (
    <div className="rounded-2xl border p-4">
      <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
        <Clock className="size-4 text-muted-foreground" />
        Waiting list
      </div>
      <div className="space-y-2">
        {entries.map((entry) => {
          const side = match.sides.find((s) => s.id === entry.side_id)
          const isMe = entry.user_id === currentUserId
          const canRemove = isMe || canManage
          return (
            <div
              key={entry.user_id}
              className="flex items-center justify-between gap-2 rounded-lg border bg-muted/30 px-2.5 py-2"
            >
              <div className="flex min-w-0 items-center gap-2">
                <Avatar name={entry.name} imageUrl={entry.avatar_url} size="sm" />
                <div className="min-w-0">
                  <span className="block truncate text-sm font-medium">
                    {entry.name}
                    {isMe && ' (you)'}
                  </span>
                  <span className="block truncate text-xs text-muted-foreground">
                    #{entry.position} · {sideName(side, 'Unassigned')}
                  </span>
                </div>
              </div>
              <div className="flex shrink-0 gap-1">
                {canManage && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={moveIn.isPending}
                    onClick={() => moveIn.mutate(entry.user_id)}
                  >
                    Move in
                  </Button>
                )}
                {canRemove && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={leave.isPending}
                    onClick={() => leave.mutate(entry.user_id)}
                  >
                    {isMe ? 'Leave' : 'Remove'}
                  </Button>
                )}
              </div>
            </div>
          )
        })}
      </div>
      {moveIn.isError && (
        <p className="mt-2 text-xs text-destructive">
          Couldn't move that player in — check there's actually a free spot.
        </p>
      )}
      {leave.isError && (
        <p className="mt-2 text-xs text-destructive">Something went wrong. Try again.</p>
      )}
    </div>
  )
}

/**
 * "Cancel match" action: a two-step confirm (to avoid an accidental cancel),
 * then `PATCH { status: "cancelled" }`. On success refreshes the match (its
 * badge flips to Cancelled and the edit affordances disappear) and the feed.
 */
function CancelMatch({ match }: { match: Match }) {
  const queryClient = useQueryClient()
  const [confirming, setConfirming] = useState(false)

  const cancel = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.PATCH('/matches/{match_id}', {
        params: { path: { match_id: match.id } },
        body: { status: 'cancelled' },
      })
      if (error) throw new Error('Failed to cancel the match')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })

  if (!confirming) {
    return (
      <Button
        variant="ghost"
        className="text-sm text-destructive hover:text-destructive"
        onClick={() => setConfirming(true)}
      >
        Cancel match
      </Button>
    )
  }

  return (
    <div className="rounded-2xl border border-destructive/30 bg-destructive/5 p-4">
      <p className="text-sm font-medium">Cancel this match?</p>
      <p className="mt-0.5 text-xs text-muted-foreground">
        It'll be marked cancelled for everyone. You can't undo this here.
      </p>
      {cancel.isError && (
        <p className="mt-1 text-xs text-destructive">
          Something went wrong. Please try again.
        </p>
      )}
      <div className="mt-3 flex gap-2">
        <Button
          variant="destructive"
          size="sm"
          disabled={cancel.isPending}
          onClick={() => cancel.mutate()}
        >
          {cancel.isPending ? 'Cancelling…' : 'Yes, cancel it'}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={cancel.isPending}
          onClick={() => setConfirming(false)}
        >
          Keep match
        </Button>
      </div>
    </div>
  )
}

/**
 * "Leave match" action: same two-step confirm as `CancelMatch`, then `POST
 * /matches/:id/leave`. The owner can't leave this way — the server rejects
 * it — so rather than duplicate `LeaveTeamDialog`'s in-dialog "pick a new
 * owner" picker, this just points them at the roster's own "Make owner"
 * button (`SideRoster`'s `ShieldPlus` affordance, next to each other
 * accepted player) and asks them to come back once they've handed it off.
 */
function LeaveMatch({ match, isOwner }: { match: Match; isOwner: boolean }) {
  const queryClient = useQueryClient()
  const [confirming, setConfirming] = useState(false)

  const leave = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.POST('/matches/{match_id}/leave', {
        params: { path: { match_id: match.id } },
      })
      if (error) throw new Error('Failed to leave the match')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
    },
  })

  if (!confirming) {
    return (
      <Button
        variant="ghost"
        className="text-sm text-destructive hover:text-destructive"
        onClick={() => setConfirming(true)}
      >
        Leave match
      </Button>
    )
  }

  if (isOwner) {
    return (
      <div className="rounded-2xl border p-4">
        <p className="text-sm font-medium">Transfer ownership first</p>
        <p className="mt-0.5 text-xs text-muted-foreground">
          As owner, hand the role to someone else — tap "Make owner" next to
          their name in the roster above — before you can leave.
        </p>
        <div className="mt-3">
          <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
            Got it
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="rounded-2xl border border-destructive/30 bg-destructive/5 p-4">
      <p className="text-sm font-medium">Leave this match?</p>
      <p className="mt-0.5 text-xs text-muted-foreground">
        You'll need to be invited or added again to rejoin.
      </p>
      {leave.isError && (
        <p className="mt-1 text-xs text-destructive">
          Something went wrong. Please try again.
        </p>
      )}
      <div className="mt-3 flex gap-2">
        <Button
          variant="destructive"
          size="sm"
          disabled={leave.isPending}
          onClick={() => leave.mutate()}
        >
          {leave.isPending ? 'Leaving…' : 'Yes, leave'}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={leave.isPending}
          onClick={() => setConfirming(false)}
        >
          Stay in match
        </Button>
      </div>
    </div>
  )
}
