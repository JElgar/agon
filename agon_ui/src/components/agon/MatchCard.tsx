import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Check, Flame, MailOpen, MessageCircle, Share2 } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { useToggleLike } from '@/hooks/useToggleLike'
import { relativeTime } from '@/lib/datetime'
import { Avatar } from './Avatar'
import { Button } from '@/components/ui/button'
import { SportBadge } from './SportBadge'
import { StatusBadge, matchBadgeStatus } from './StatusBadge'
import { ScoreConfirmationBar } from './ScoreConfirmationBar'
import { MatchHeaderCarousel } from './MatchHeaderCarousel'
import { InvitationResponseDialog } from './InvitationResponseDialog'
import { LiveMatchBlock } from './live/LiveMatchBlock'
import { CricketMatchBlock } from './live/CricketMatchBlock'
import { NetballMatchBlock } from './live/NetballMatchBlock'
import { CricketScoreBlock } from './CricketScoreBlock'
import { KnownPlayersRow } from './KnownPlayersRow'
import { FootballScorersBySide } from './FootballScorersBySide'
import { NetballScorersBySide } from './NetballScorersBySide'
import { useMatchScore } from '@/hooks/useMatchScore'
import { footballScoreFrom } from '@/lib/liveScore'
import { netballGoalsFromScore } from '@/lib/score'
import { netballScoreFrom } from '@/lib/netballScore'
import {
  cricketProgressFromScore,
  cricketScoreFrom,
  cricketStateDescription,
} from '@/lib/cricketScore'
import { cricketFormat } from '@/lib/matchFormat'
import {
  displayScore,
  footballGoalsFromScore,
  headlineBySide,
  headlineLabel,
  setLine,
} from '@/lib/score'
import { myPendingInvitation, mySideId, orderSidesForViewer } from '@/lib/members'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
type MatchSide = components['schemas']['MatchSide']

export interface MatchCardProps extends React.HTMLAttributes<HTMLDivElement> {
  /** A detail-view `Match` (full roster), a feed's lighter `FeedMatch`, or a
   *  search/profile-activity `SearchMatch` — none of the trimmed shapes
   *  carry the full roster. Every field this card actually reads is present
   *  on all three. */
  match: Match | FeedMatch | SearchMatch
  /** Called when the card body is activated (navigate to match detail). */
  onOpen?: () => void
  /** The signed-in user's id. When they're a participant with a pending score to
   *  respond to, an inline confirm/dispute prompt is shown. */
  currentUserId?: string
}

/** Display label for a side: the server-resolved name (always present), or a
 *  neutral fallback for the unlikely case it's missing. */
function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/**
 * Accept/decline prompt for the feed card, shown when the viewer has a
 * pending invitation to this match — takes the same slot as the score
 * confirm bar below, since only one applies at a time: respond to the invite
 * first, and only once joined does confirming the score apply.
 */
function InviteResponseBar({
  match,
  currentUserId,
}: {
  match: Match | FeedMatch | SearchMatch
  currentUserId?: string
}) {
  const queryClient = useQueryClient()
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)
  const invitation = myPendingInvitation(match, currentUserId)

  const respond = useMutation({
    mutationFn: async (response: components['schemas']['InvitationResponse']) => {
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({
        queryKey: ['notifications-unread-count'],
      })
    },
  })

  if (!invitation) return null

  return (
    <>
      <div className="flex items-center justify-between gap-2 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2">
        <p className="flex items-center gap-1.5 text-xs font-medium text-primary">
          <MailOpen className="size-3.5" /> You're invited
        </p>
        <div className="flex gap-1.5">
          <Button
            size="sm"
            className="h-7 px-2.5 text-xs"
            onClick={() => setAction('accept')}
          >
            Accept
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-7 px-2.5 text-xs"
            onClick={() => setAction('decline')}
          >
            Decline
          </Button>
        </div>
      </div>
      <InvitationResponseDialog
        open={action !== null}
        onOpenChange={(open) => !open && setAction(null)}
        action={action}
        name={match.name}
        matchId={match.id}
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={() => setAction(null)}
      />
    </>
  )
}

/**
 * Shares a match's detail link via the native share sheet, falling back to a
 * clipboard copy (then a manual prompt) where that's unavailable — mirrors
 * `CopyInviteButton`'s fallback chain, with a transient checkmark standing in
 * for its "Copied!" label since this is an icon-only button.
 */
function ShareMatchButton({ match }: { match: Match | FeedMatch | SearchMatch }) {
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const id = setTimeout(() => setCopied(false), 2000)
    return () => clearTimeout(id)
  }, [copied])

  const share = async () => {
    const url = `${window.location.origin}/matches/${match.id}`
    if (navigator.share) {
      try {
        await navigator.share({ title: match.name, url })
        return
      } catch {
        // User dismissed the sheet, or share failed — fall through to copy.
      }
    }
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
    } catch {
      // Clipboard blocked (e.g. insecure context) — surface the link so the
      // user can copy it manually rather than failing silently.
      window.prompt('Copy this match link:', url)
    }
  }

  return (
    <button
      type="button"
      onClick={share}
      className="flex items-center transition-colors hover:text-primary"
      aria-label="Share match"
    >
      {copied ? (
        <Check className="size-3.5 text-primary" />
      ) : (
        <Share2 className="size-3.5" />
      )}
    </button>
  )
}

/**
 * A match card for the feed: the two sides, the score, sport, confirmation
 * state, and social actions. Presentational — data comes from a `Match`
 * (which the feed's `FeedItem_Match` extends); callers wire the action handlers.
 */
export function MatchCard({
  match,
  onOpen,
  currentUserId,
  className,
  ...props
}: MatchCardProps) {
  // The viewer's own side, when resolvable, always renders first (left) —
  // both here and on the match detail page. `orderedMatch` carries the same
  // reordering into the live-score/scorer sub-components below, which each
  // derive their own sideA/sideB straight off `match.sides` — passing this
  // instead of `match` keeps them in sync with the score box without
  // touching their internals (everything they key off is side id, not
  // array position).
  const orderedSides = orderSidesForViewer(match.sides, mySideId(match, currentUserId))
  const orderedMatch = { ...match, sides: orderedSides }
  const [sideA, sideB] = orderedSides
  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}
  const sets = scoreInfo ? setLine(scoreInfo.score, orderedSides) : []

  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')
  const aWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideA?.id
  const bWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideB?.id
  // The "X beat Y" headline needs the winner named first, or it reads
  // backwards whenever B is the one who actually won (the score box below
  // stays in match.sides order regardless — only this headline reorders).
  // No winner (tie/vs case) just falls back to A/B order, same as before.
  const winningTeamName = bWon ? nameB : nameA
  const losingTeamName = bWon ? nameA : nameB

  const isLiveSport =
    match.match_type === 'football' || match.match_type === 'cricket' || match.match_type === 'netball'
  const isCurrentlyLive = isLiveSport && match.status === 'in_progress'
  // Only fetched while live — a finished match doesn't need it. Football's
  // confirmed/pending `Score.Football` already embeds its goals (see
  // `footballGoalsFromScore`/`FootballScorersBySide` below), and cricket's
  // confirmed `Score.Cricket` already embeds its per-innings detail
  // (`cricketScore` below) — both produced by finishing a live-scored match
  // (see `finishMatch` in `LiveScoringPage`/`CricketLiveScoringPage`), same
  // technique as `MatchDetailPage`'s completed scorecards but without that
  // page's extra `/score` poll, which this compact card doesn't need.
  const scoreQuery = useMatchScore(match.id, {
    enabled: isCurrentlyLive,
    refetchInterval: 20000,
  })
  // Gate on `isCurrentlyLive`, not just "did the fetch return something" —
  // `scoreQuery` is a react-query cache keyed only by match id, shared with
  // `MatchDetailPage`, which fetches it for a *completed* match too (to
  // show the finished scorecard). `enabled: false` only stops a new fetch
  // here; it doesn't hide data another component already populated that
  // cache entry with, so without this guard a card could keep rendering the
  // live block/badge for a match that finished after the card last mounted.
  const footballState = isCurrentlyLive ? footballScoreFrom(scoreQuery.data) : null
  const cricketState = isCurrentlyLive ? cricketScoreFrom(scoreQuery.data) : null
  const netballState = isCurrentlyLive ? netballScoreFrom(scoreQuery.data) : null
  // Goals for a finished football/netball match, read straight off the
  // confirmed/pending score (no fetch) — shown under the plain score box
  // below (the live ticker in `LiveMatchBlock`/`NetballMatchBlock` only
  // renders while still in progress).
  const finishedFootballGoals = scoreInfo && !isCurrentlyLive ? footballGoalsFromScore(scoreInfo.score) : null
  const finishedNetballGoals = scoreInfo && !isCurrentlyLive ? netballGoalsFromScore(scoreInfo.score) : null
  // Resolved server-side on `confirmed_score`/`pending_score` itself (see
  // `Api::hydrate_confirmed_pending_score_players`) — this card's `match` is
  // a `FeedMatch`/`SearchMatch`/`Match`, none of which reliably carry a full
  // player roster to resolve scorer names from locally.
  const finishedFootballScorePlayers =
    scoreInfo && !isCurrentlyLive ? footballScoreFrom(scoreInfo.score)?.players : undefined
  const finishedFootballPeriodTimes =
    scoreInfo && !isCurrentlyLive ? footballScoreFrom(scoreInfo.score)?.period_times : undefined
  const finishedNetballScorePlayers =
    scoreInfo && !isCurrentlyLive ? netballScoreFrom(scoreInfo.score)?.players : undefined
  const finishedNetballPeriodTimes =
    scoreInfo && !isCurrentlyLive ? netballScoreFrom(scoreInfo.score)?.period_times : undefined
  // A cricket match's confirmed score carries its own per-innings detail once
  // it's been live-scored (`Score::Cricket`; see `finishMatch` in
  // `CricketLiveScoringPage`) — a manually-logged result still degrades to
  // the generic totals-only `Score::Simple`.
  const cricketScore = scoreInfo ? cricketScoreFrom(scoreInfo.score) : null
  // Cricket's own state-of-game line ("England won by 4 wickets" / "...need
  // 200 to win" / "...lead by 30 runs") is a strictly better headline than
  // the generic "beat"/"vs" — it carries the margin, not just the winner —
  // so it takes over the header whenever there's cricket detail (live or
  // confirmed) to derive it from, and the score block below skips repeating
  // it (`showDescription`).
  const cricketFmt = cricketFormat(match.format)
  const cricketDescription = cricketState
    ? cricketStateDescription(
        match,
        { innings: cricketState.innings, awaiting_next_innings: cricketState.awaiting_next_innings ?? true },
        cricketFmt,
      )
    : cricketScore
      ? cricketStateDescription(match, cricketProgressFromScore(cricketScore), cricketFmt)
      : null

  const { like_count, comment_count, i_liked } = match.social
  const toggleLike = useToggleLike(match)

  return (
    <div
      className={cn(
        'overflow-hidden rounded-xl border bg-card text-card-foreground',
        className,
      )}
      {...props}
    >
      {/* Header: the cricket state-of-game line when there is one, else the
          usual "who beat who" + when + sport. */}
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-start justify-between gap-3 p-3.5 text-left"
      >
        <p className="text-sm leading-snug">
          {cricketDescription ? (
            <span>{cricketDescription}</span>
          ) : (
            <>
              <span className={cn(!!scoreInfo?.winnerSideId && 'font-medium')}>{winningTeamName}</span>
              <span className="text-primary">
                {' '}
                {match.match_type !== 'cricket' && scoreInfo?.winnerSideId ? 'beat' : 'vs'}{' '}
              </span>
              <span>{losingTeamName}</span>
            </>
          )}
          <span className="text-muted-foreground"> · {relativeTime(match.starts_at)}</span>
        </p>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          <SportBadge sport={match.match_type} />
        </div>
      </button>

      {/* Title + description — clickable, same as the header/score blocks above. */}
      {(match.name || match.description) && (
        <button type="button" onClick={onOpen} className="block w-full px-3.5 pb-3 text-left">
          {match.name && <p className="font-medium leading-snug">{match.name}</p>}
          {match.description && (
            <p className="mt-0.5 text-sm text-muted-foreground">
              {match.description}
            </p>
          )}
        </button>
      )}

      {/* Score block — a live-scored match shows the mini-ticker instead of
          the usual confirmed/pending result; a finished cricket match with
          per-innings detail gets its own tile too. */}
      {footballState ? (
        <button type="button" onClick={onOpen} className="mx-3.5 mb-3 block text-left">
          <LiveMatchBlock match={orderedMatch} state={footballState} />
        </button>
      ) : netballState ? (
        <button type="button" onClick={onOpen} className="mx-3.5 mb-3 block text-left">
          <NetballMatchBlock match={orderedMatch} state={netballState} />
        </button>
      ) : cricketState ? (
        <button type="button" onClick={onOpen} className="mx-3.5 mb-3 block text-left">
          <CricketMatchBlock match={orderedMatch} state={cricketState} showDescription={false} />
        </button>
      ) : cricketScore ? (
        <button type="button" onClick={onOpen} className="mx-3.5 mb-3 block text-left">
          <CricketScoreBlock match={orderedMatch} score={cricketScore} showDescription={false} />
        </button>
      ) : (
        scoreInfo && (
          <button
            type="button"
            onClick={onOpen}
            className="mx-3.5 mb-3 block rounded-lg bg-muted/50 px-3.5 py-3 text-left"
          >
            <div className="flex items-center justify-between">
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <Avatar name={nameA} imageUrl={sideA?.team_logo?.image_url} size="md" ring={aWon ? 'winner' : 'none'} />
                <span className="truncate text-xs font-medium">{nameA}</span>
              </div>
              <div className="px-3 text-center">
                <div className="text-2xl font-medium leading-none tracking-tight">
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
                <span className="truncate text-xs font-medium">{nameB}</span>
              </div>
            </div>
            {sets.length > 0 && (
              <div className="mt-2 border-t pt-2 text-center text-[11px] text-muted-foreground">
                {sets.map((s, i) => (
                  <span key={i}>
                    {i > 0 && <span className="mx-1.5 text-border">·</span>}
                    Set {i + 1} <span className="font-medium text-foreground">{s}</span>
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
                className="mt-2.5 text-[11px]"
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
                className="mt-2.5 text-[11px]"
              />
            )}
          </button>
        )
      )}

      {/* "You follow Sofia, Raj +1" — who among the match's participants the
          viewer follows. Only a feed card's `FeedMatch` carries this (a
          per-viewer fan-out concept); `Match`/`SearchMatch` don't. */}
      {'known_participants' in match && (
        <KnownPlayersRow
          participants={match.known_participants}
          count={match.known_participants_count}
          className="mx-3.5 mb-3 rounded-lg bg-muted/50 px-3.5 py-2.5"
        />
      )}

      {/* Header photo, when the match has one. The gap above comes from the
          preceding block's own bottom margin (score box / title), so this
          only needs to provide the gap below itself. */}
      {match.header_photos.length > 0 && (
        <div className="px-3.5 pb-3">
          <MatchHeaderCarousel photos={match.header_photos} />
        </div>
      )}

      {/* Respond to a pending invite first; only once joined does the score
          confirm/dispute prompt apply — the two are mutually exclusive. */}
      {myPendingInvitation(match, currentUserId) ? (
        <div className="px-3.5 pb-2.5">
          <InviteResponseBar match={match} currentUserId={currentUserId} />
        </div>
      ) : (
        match.pending_score && (
          <div className="px-3.5 pb-2.5">
            <ScoreConfirmationBar
              match={match}
              currentUserId={currentUserId}
              variant="card"
            />
          </div>
        )
      )}

      {/* Footer: kudos + comments on the left, lifecycle/confirmation state on the right. */}
      <div className="flex items-center gap-4 border-t px-3.5 py-2.5 text-muted-foreground">
        <button
          type="button"
          onClick={() => toggleLike.mutate(!i_liked)}
          aria-pressed={i_liked}
          aria-label={i_liked ? 'Remove kudos' : 'Give kudos'}
          className={cn(
            'flex items-center gap-1.5 text-xs transition-colors hover:text-primary',
            i_liked && 'text-primary',
          )}
        >
          <Flame className={cn('size-3.5', i_liked && 'fill-current')} />{' '}
          {like_count}
        </button>
        <button
          type="button"
          onClick={onOpen}
          className="flex items-center gap-1.5 text-xs transition-colors hover:text-primary"
        >
          <MessageCircle className="size-3.5" /> {comment_count}
        </button>
        <ShareMatchButton match={match} />
        <StatusBadge status={matchBadgeStatus(match)} className="ml-auto" />
      </div>
    </div>
  )
}
