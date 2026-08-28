import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Flame, MailOpen, Pencil, Radio, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Avatar } from '@/components/agon/Avatar'
import { MatchHeaderCarousel } from '@/components/agon/MatchHeaderCarousel'
import { SportBadge } from '@/components/agon/SportBadge'
import { StatusBadge, matchBadgeStatus } from '@/components/agon/StatusBadge'
import { ScoreConfirmationBar } from '@/components/agon/ScoreConfirmationBar'
import { LiveMatchBlock } from '@/components/agon/live/LiveMatchBlock'
import { CricketMatchBlock } from '@/components/agon/live/CricketMatchBlock'
import { NetballMatchBlock } from '@/components/agon/live/NetballMatchBlock'
import { CricketScoreBlock } from '@/components/agon/CricketScoreBlock'
import { CricketScorecard } from '@/components/agon/CricketScorecard'
import { FootballScorecard } from '@/components/agon/FootballScorecard'
import { FootballGoalContributions } from '@/components/agon/FootballGoalContributions'
import { FootballScorersBySide } from '@/components/agon/FootballScorersBySide'
import { NetballScorecard } from '@/components/agon/NetballScorecard'
import { NetballScorersBySide } from '@/components/agon/NetballScorersBySide'
import { NetballQuarterBreakdown } from '@/components/agon/NetballQuarterBreakdown'
import { useLiveEvents } from '@/hooks/useLiveScore'
import { useMatchScore } from '@/hooks/useMatchScore'
import { footballScoreFrom, footballEventSourceFromScore } from '@/lib/liveScore'
import { netballScoreFrom, netballEventSourceFromScore } from '@/lib/netballScore'
import { cricketInningsFor, cricketScoreFrom, inningsDeliveriesFromEvents } from '@/lib/cricketScore'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import {
  displayScore,
  footballGoalsFromScore,
  netballGoalsFromScore,
  headlineBySide,
  headlineLabel,
  setLine,
} from '@/lib/score'
import {
  isParticipant,
  memberAvatarUrl,
  memberInviteToken,
  memberName,
  myPendingInvitation,
  mySideId,
  orderSidesForViewer,
  withInvitationStatus,
} from '@/lib/members'
import { CopyInviteButton } from '@/components/agon/CopyInviteButton'
import { MatchDetailsEditor } from '@/components/agon/MatchDetailsEditor'
import { MatchFormatCard } from '@/components/agon/MatchFormatCard'
import { MatchResultEditor } from '@/components/agon/MatchResultEditor'
import { MatchRosterEditor } from '@/components/agon/MatchRosterEditor'
import { InvitePlayers } from '@/components/agon/InvitePlayers'
import { MatchComments } from '@/components/agon/MatchComments'
import { LikedByLine } from '@/components/agon/MatchLikes'
import { useToggleLike } from '@/hooks/useToggleLike'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { InvitePromptDialog } from '@/components/agon/InvitePromptDialog'
import { useInvitePrompt } from '@/hooks/useInvitePrompt'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']

/** Display label for a side: the server-resolved name (always present), or a
 *  neutral fallback for the unlikely case it's missing. */
function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

// Roster tab ids for the mobile tablist — side A/B use these fixed ids
// rather than the sides' own (possibly absent) `id`s so the tablist always
// has something stable to key off; unassigned players get a third tab.
const ROSTER_TAB_A = '__side_a__'
const ROSTER_TAB_B = '__side_b__'
const ROSTER_TAB_UNASSIGNED = '__unassigned__'

/** Full match view: score (with confirm/dispute when pending), sides + rosters.
 *  Participants get inline editing of details/result, plus invite and cancel. */
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
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
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
  // Which roster tab is open on mobile — side-by-side columns (`sm:` and up)
  // ignore this entirely. Defaults to the first side; reset below if it ever
  // points at a tab that's no longer showing (e.g. the last unassigned
  // player was placed on a side).
  const [rosterTab, setRosterTab] = useState<string>(ROSTER_TAB_A)

  const canEdit = isParticipant(match, currentUserId)
  const cancelled = match.status === 'cancelled'
  const isLiveSport =
    match.match_type === 'football' || match.match_type === 'cricket' || match.match_type === 'netball'

  // The viewer's own side, when they're playing, always renders first (left)
  // — mirrors the feed card (`MatchCard`). `orderedMatch` carries the same
  // reordering into the live-score/scorecard sub-components below, which
  // each derive their own sideA/sideB straight off `match.sides` — passing
  // this instead of `match` keeps them in sync with the score box without
  // touching their internals (everything they key off is side id, not
  // array position).
  const orderedSides = orderSidesForViewer(match.sides, mySideId(match, currentUserId))
  const orderedMatch = { ...match, sides: orderedSides }
  const [sideA, sideB] = orderedSides
  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')

  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}
  const sets = scoreInfo ? setLine(scoreInfo.score, orderedSides) : []
  const aWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideA?.id
  const bWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideB?.id

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
  // Goals for a finished football/netball match, read straight off the
  // confirmed/pending score — shown as a scorer breakdown under the plain
  // score box below (same as the feed/profile card's `MatchCard`); the full
  // event timeline (goals *and* cards/subs, or goals *and* fouls) stays in
  // `FootballScorecard`/`NetballScorecard` further down the page regardless.
  const finishedFootballGoals = scoreInfo && !isCurrentlyLive ? footballGoalsFromScore(scoreInfo.score) : null
  const finishedFootballScorePlayers =
    scoreInfo && !isCurrentlyLive ? footballScoreFrom(scoreInfo.score)?.players : undefined
  const finishedFootballPeriodTimes =
    scoreInfo && !isCurrentlyLive ? footballScoreFrom(scoreInfo.score)?.period_times : undefined
  const finishedNetballGoals = scoreInfo && !isCurrentlyLive ? netballGoalsFromScore(scoreInfo.score) : null
  const finishedNetballScorePlayers =
    scoreInfo && !isCurrentlyLive ? netballScoreFrom(scoreInfo.score)?.players : undefined
  const finishedNetballPeriodTimes =
    scoreInfo && !isCurrentlyLive ? netballScoreFrom(scoreInfo.score)?.period_times : undefined
  // Same "live while in progress, else confirmed/pending" source as
  // `cricketScoreInnings` below — needed here just for `.players` (the
  // score's resolved-name map), passed to `CricketScorecard`.
  const cricketScore = cricketScoreFrom(isCurrentlyLive ? scoreQuery.data : scoreInfo?.score)
  // `FootballScorecard`'s event timeline: the live running score while the
  // match is in progress, else straight off the confirmed/pending score —
  // stays visible once the match is completed, unlike `footballState` above.
  const footballEventSource = footballEventSourceFromScore(
    isCurrentlyLive ? scoreQuery.data : scoreInfo?.score,
  )
  const netballEventSource = netballEventSourceFromScore(
    isCurrentlyLive ? scoreQuery.data : scoreInfo?.score,
  )
  // Quarter-by-quarter breakdown reads the same way regardless of which of
  // netball's two live-scoring methods produced the score (see
  // `NetballQuarterBreakdown`'s doc comment) — shown even for a
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

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ChevronLeft className="size-4" /> Back
        </Button>
        <SportBadge sport={match.match_type} />
      </div>

      {/* Header image(s): a banner for one, a swipeable carousel for several. */}
      <MatchHeaderCarousel photos={match.header_photos} />

      {/* Details card — name + when + where, inline-editable by participants. */}
      {editingDetails ? (
        <MatchDetailsEditor
          match={match}
          onDone={() => setEditingDetails(false)}
        />
      ) : (
        <div className="rounded-xl border bg-card p-4">
          <div className="flex items-start justify-between gap-2">
            <p className="text-sm text-muted-foreground">{match.name}</p>
            {canEdit && !cancelled && (
              <Button
                variant="ghost"
                size="sm"
                className="-mt-1 -mr-1 h-7 gap-1 px-2 text-xs text-muted-foreground"
                onClick={() => setEditingDetails(true)}
              >
                <Pencil className="size-3" /> Edit
              </Button>
            )}
          </div>

          {/* Score header — the live block (score + mini-ticker) takes over
              while a football/cricket match is being scored live; otherwise
              the usual confirmed/pending result. */}
          {footballState ? (
            <div className="mt-3">
              <LiveMatchBlock match={orderedMatch} state={footballState} tickerLimit={3} />
            </div>
          ) : netballState ? (
            <div className="mt-3">
              <NetballMatchBlock match={orderedMatch} state={netballState} tickerLimit={3} />
            </div>
          ) : cricketState ? (
            <div className="mt-3">
              <CricketMatchBlock match={orderedMatch} state={cricketState} />
            </div>
          ) : cricketScore ? (
            <div className="mt-3">
              <CricketScoreBlock match={orderedMatch} score={cricketScore} />
            </div>
          ) : scoreInfo ? (
            <div className="mt-3 flex items-center justify-between">
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <Avatar name={nameA} imageUrl={sideA?.team_logo?.image_url} size="md" ring={aWon ? 'winner' : 'none'} />
                <p className={cn('truncate text-sm', aWon && 'font-medium')}>{nameA}</p>
              </div>
              <div className="px-3 text-center">
                <div className="text-3xl font-medium tracking-tight">
                  {headline[sideA?.id ?? ''] ?? 0}
                  <span className="text-muted-foreground">–</span>
                  {headline[sideB?.id ?? ''] ?? 0}
                </div>
                <div className="mt-0.5 text-[9px] uppercase tracking-widest text-muted-foreground">
                  {headlineLabel(scoreInfo.score)}
                </div>
              </div>
              <div className="flex min-w-0 flex-1 flex-row-reverse items-center gap-2 text-right">
                <Avatar name={nameB} imageUrl={sideB?.team_logo?.image_url} size="md" ring={bWon ? 'winner' : 'none'} />
                <p className={cn('truncate text-sm', bWon && 'font-medium')}>{nameB}</p>
              </div>
            </div>
          ) : (
            <p className="mt-3 text-sm text-muted-foreground">
              No score recorded yet.
            </p>
          )}

          {sets.length > 0 && (
            <div className="mt-2 border-t pt-2 text-center text-xs text-muted-foreground">
              {sets.map((s, i) => (
                <span key={i}>
                  {i > 0 && <span className="mx-1.5 text-border">·</span>}
                  Set {i + 1}{' '}
                  <span className="font-medium text-foreground">{s}</span>
                </span>
              ))}
            </div>
          )}

          {finishedFootballGoals && (
            <FootballScorersBySide
              goals={finishedFootballGoals}
              match={orderedMatch}
              players={finishedFootballScorePlayers}
              periodTimes={finishedFootballPeriodTimes}
              sideA={sideA}
              sideB={sideB}
              className="mt-2 text-xs"
            />
          )}

          {finishedNetballGoals && (
            <NetballScorersBySide
              goals={finishedNetballGoals}
              match={orderedMatch}
              players={finishedNetballScorePlayers}
              periodTimes={finishedNetballPeriodTimes}
              sideA={sideA}
              sideB={sideB}
              className="mt-2 text-xs"
            />
          )}

          <div className="mt-3 flex items-center justify-between">
            <StatusBadge status={matchBadgeStatus(match)} />
            <div className="flex items-center gap-1">
              {canEdit && isLiveSport && !cancelled && match.status !== 'completed' && (
                <Button asChild variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs text-primary">
                  <Link to={liveEntryPath}>
                    <Radio className="size-3" /> {hasLiveState ? 'Continue scoring' : 'Score live'}
                  </Link>
                </Button>
              )}
              {/* Hidden while live scoring is actually under way (`hasLiveState`):
                  that data belongs on the live scoring screen above, and
                  finishing there is what completes the match with it. Opening
                  this generic editor instead used to let a blank/separate
                  score quietly displace the live-scored one — see
                  `MatchResultEditor`'s doc comment and the matching
                  server-side guard in `update_match`. Once the match is
                  completed (or was never live-scored) this is the normal
                  add/edit-result affordance again. */}
              {canEdit && !cancelled && !hasLiveState && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs text-muted-foreground"
                  onClick={() => setEditingResult(true)}
                >
                  {scoreInfo ? 'Edit result' : 'Add result'}
                </Button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Result editor — opens below the card when editing the score. */}
      {editingResult && (
        <MatchResultEditor
          match={match}
          onDone={() => setEditingResult(false)}
        />
      )}

      {/* Goal contributions table — every scorer/assister, sortable by most
          goals (default) or most assists, each breaking ties on the other.
          Same goal log as the event timeline further down, so it tracks a
          live match too. */}
      {footballEventSource && (
        <FootballGoalContributions
          goals={footballEventSource.goals}
          match={orderedMatch}
          players={footballEventSource.players}
        />
      )}

      {/* Match format — half length/overs limit/penalty runs, football and
          cricket only. Renders nothing for other sports. */}
      <MatchFormatCard match={match} canEdit={canEdit && !cancelled} />

      {/* Respond to a pending invite first; only once joined does the score
          confirm/dispute prompt apply — the two are mutually exclusive (same
          logic as the feed/profile match card). */}
      {myPendingInvitation(match, currentUserId) ? (
        <InviteBanner match={match} currentUserId={currentUserId} />
      ) : (
        match.pending_score && (
          <ScoreConfirmationBar
            match={match}
            currentUserId={currentUserId}
            variant="detail"
          />
        )
      )}

      {/* Rosters, one column per side — or the drag-to-reassign/remove editor
          in place of it, for a participant reconciling the line-up. */}
      {editingRoster ? (
        <MatchRosterEditor match={match} onDone={() => setEditingRoster(false)} />
      ) : (
        <div className="flex flex-col gap-2">
          {canEdit && !cancelled && (
            <div className="flex justify-end">
              <Button
                variant="ghost"
                size="sm"
                className="h-7 gap-1 px-2 text-xs text-muted-foreground"
                onClick={() => setEditingRoster(true)}
              >
                <Pencil className="size-3" /> Edit roster
              </Button>
            </div>
          )}
          <RosterTabs
            nameA={nameA}
            nameB={nameB}
            playersA={match.players.filter((p) => p.side_id === sideA?.id)}
            playersB={match.players.filter((p) => p.side_id === sideB?.id)}
            unassigned={match.players.filter(
              (p) => p.side_id !== sideA?.id && p.side_id !== sideB?.id,
            )}
            activeTab={rosterTab}
            onTabChange={setRosterTab}
          />
        </div>
      )}

      {/* Cricket scorecard: run progression + per-player batting/bowling,
          once there's per-innings detail recorded (live-scored or entered
          directly). */}
      {cricketInnings && cricketInnings.length > 0 && (
        <CricketScorecard match={orderedMatch} innings={cricketInnings} players={cricketScore?.players} />
      )}

      {/* Football event timeline: goals/cards/subs, once there's detail
          recorded (live-scored or entered directly) — stays visible after
          the match finishes, unlike the live score header above. */}
      {footballEventSource && <FootballScorecard match={orderedMatch} detail={footballEventSource} />}

      {/* Netball quarter breakdown — reads the same regardless of which
          live-scoring method produced the score (see
          `NetballQuarterBreakdown`'s doc comment), so it shows even for a
          quarter-only-scored match. */}
      {netballQuarterScore && (
        <NetballQuarterBreakdown score={netballQuarterScore} sideA={sideA} sideB={sideB} />
      )}

      {/* Netball event timeline: goals/fouls, only present for an
          event-by-event-scored match — stays visible after the match
          finishes, unlike the live score header above. */}
      {netballEventSource && <NetballScorecard match={orderedMatch} detail={netballEventSource} />}

      {/* Invite more people (participants only). */}
      {canEdit && !cancelled && (
        inviting ? (
          <InvitePlayers match={match} onDone={() => setInviting(false)} />
        ) : (
          <Button
            variant="outline"
            className="gap-1.5"
            onClick={() => setInviting(true)}
          >
            <UserPlus className="size-4" /> Invite players
          </Button>
        )
      )}

      {/* Social: like the match, then the comment thread. */}
      <LikeBar match={match} />
      <MatchComments matchId={match.id} currentUserId={currentUserId} />

      {/* Cancel the match (participants only; not already cancelled). */}
      {canEdit && !cancelled && <CancelMatch match={match} />}
    </div>
  )
}

/**
 * The match's like control + count, plus who liked it. Anyone signed in can
 * like a match (not just participants). Optimistic via `useToggleLike`, so the
 * flame fills and the count moves the instant it's pressed. `LikedByLine`
 * renders "Liked by A, B +N" alongside it and opens the full list on click.
 */
function LikeBar({ match }: { match: Match }) {
  const { like_count, i_liked } = match.social
  const toggleLike = useToggleLike(match)

  return (
    <div className="flex items-center gap-4 rounded-xl border bg-card px-4 py-2.5 text-sm text-muted-foreground">
      <button
        type="button"
        onClick={() => toggleLike.mutate(!i_liked)}
        aria-pressed={i_liked}
        aria-label={i_liked ? 'Unlike match' : 'Like match'}
        className={cn(
          'flex items-center gap-1.5 transition-colors hover:text-primary',
          i_liked && 'text-primary',
        )}
      >
        <Flame className={cn('size-4', i_liked && 'fill-current')} /> {like_count}{' '}
        {like_count === 1 ? 'like' : 'likes'}
      </button>

      <LikedByLine matchId={match.id} likeCount={like_count} />
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
      const { error } = await fetchClient.POST(
        '/invitations/{invitation_id}/respond',
        {
          params: { path: { invitation_id: invitation.id } },
          body: { response },
        },
      )
      if (error) throw new Error('Failed to respond to invitation')
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
      <div className="rounded-xl border border-primary/30 bg-primary/5 p-4">
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
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={handleResponded}
      />
    </>
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
    <div className="rounded-xl border border-destructive/30 bg-destructive/5 p-4">
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
 * Rosters, side by side on a screen wide enough for it (`sm:` and up) — a
 * phone-width column pair squeezes both names down too far to read, so
 * below that breakpoint this switches to one roster at a time behind a
 * tablist: a tab per side, plus "Unassigned" whenever anyone hasn't been
 * placed on a side yet (the side-by-side layout has no room to show them
 * at all, so the tab is the only place they're visible).
 */
function RosterTabs({
  nameA,
  nameB,
  playersA,
  playersB,
  unassigned,
  activeTab,
  onTabChange,
}: {
  nameA: string
  nameB: string
  playersA: MatchPlayer[]
  playersB: MatchPlayer[]
  unassigned: MatchPlayer[]
  activeTab: string
  onTabChange: (tab: string) => void
}) {
  const tabs = [
    { id: ROSTER_TAB_A, title: nameA, players: playersA },
    { id: ROSTER_TAB_B, title: nameB, players: playersB },
    ...(unassigned.length > 0
      ? [{ id: ROSTER_TAB_UNASSIGNED, title: 'Unassigned', players: unassigned }]
      : []),
  ]
  // The unassigned tab can disappear out from under an active selection
  // (last unassigned player got placed on a side) — fall back to side A
  // rather than rendering nothing.
  const active = tabs.find((t) => t.id === activeTab) ?? tabs[0]

  return (
    <>
      <div
        role="tablist"
        aria-label="Roster"
        className={cn(
          'mb-2 grid gap-1 rounded-lg bg-muted p-1 sm:hidden',
          tabs.length === 3 ? 'grid-cols-3' : 'grid-cols-2',
        )}
      >
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={active.id === t.id}
            onClick={() => onTabChange(t.id)}
            className={cn(
              'truncate rounded-md px-2 py-1.5 text-sm font-medium transition-colors',
              active.id === t.id
                ? 'bg-card text-foreground shadow-sm'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {t.title}
          </button>
        ))}
      </div>
      <div className="sm:hidden">
        <SideRoster title={active.title} players={active.players} />
      </div>

      <div className="hidden gap-3 sm:grid sm:grid-cols-2">
        <SideRoster title={nameA} players={playersA} />
        <SideRoster title={nameB} players={playersB} />
      </div>
    </>
  )
}

function SideRoster({ title, players }: { title: string; players: MatchPlayer[] }) {
  return (
    <div className="rounded-xl border bg-card p-3">
      <p className="mb-2 truncate text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </p>
      <div className="flex flex-col gap-1.5">
        {players.length === 0 && (
          <p className="text-xs text-muted-foreground">No players.</p>
        )}
        {players.map((p, i) => {
          const name = memberName(p.member)
          const avatarUrl = memberAvatarUrl(p.member)
          const pending =
            p.member.invitation && p.member.invitation.status === 'pending'
          // Token-invited (external) players have a shareable link; offer to
          // copy it instead of the bare "invited" label.
          const inviteToken = memberInviteToken(p.member)
          // Only linked Agon users have a profile to open — external players
          // (invited by name only) have no `user_id` and stay plain text.
          const userId = p.member.type === 'User' ? p.member.user_id : undefined
          return (
            <div key={i} className="flex items-center gap-2">
              {userId ? (
                <Link
                  to={`/users/${userId}`}
                  className="flex min-w-0 flex-1 items-center gap-2"
                >
                  <Avatar name={name} imageUrl={avatarUrl} size="md" />
                  <span className="flex-1 truncate text-sm">{name}</span>
                </Link>
              ) : (
                <>
                  <Avatar name={name} imageUrl={avatarUrl} size="md" />
                  <span className="flex-1 truncate text-sm">{name}</span>
                </>
              )}
              {inviteToken ? (
                <CopyInviteButton token={inviteToken} />
              ) : (
                pending && (
                  <span className="text-[10px] text-muted-foreground">invited</span>
                )
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
