import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Flame, MailOpen, Pencil, UserPlus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Avatar } from '@/components/agon/Avatar'
import { SportBadge } from '@/components/agon/SportBadge'
import { StatusBadge, matchBadgeStatus } from '@/components/agon/StatusBadge'
import { ScoreConfirmationBar } from '@/components/agon/ScoreConfirmationBar'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { displayScore, headlineBySide, headlineLabel, setLine } from '@/lib/score'
import {
  isParticipant,
  memberInviteToken,
  memberName,
  myPendingInvitation,
  withInvitationStatus,
} from '@/lib/members'
import { CopyInviteButton } from '@/components/agon/CopyInviteButton'
import { MatchDetailsEditor } from '@/components/agon/MatchDetailsEditor'
import { MatchResultEditor } from '@/components/agon/MatchResultEditor'
import { InvitePlayers } from '@/components/agon/InvitePlayers'
import { MatchComments } from '@/components/agon/MatchComments'
import { useToggleLike } from '@/hooks/useToggleLike'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']

function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

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
  const [inviting, setInviting] = useState(false)

  const canEdit = isParticipant(match, currentUserId)
  const cancelled = match.status === 'cancelled'

  const [sideA, sideB] = match.sides
  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')

  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}
  const sets = scoreInfo ? setLine(scoreInfo.score, match.sides) : []
  const aWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideA?.id
  const bWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideB?.id

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ChevronLeft className="size-4" /> Back
        </Button>
        <SportBadge sport={match.match_type} />
      </div>

      {/* Details card — name + when + where, inline-editable by participants. */}
      {editingDetails ? (
        <MatchDetailsEditor
          match={match}
          onDone={() => setEditingDetails(false)}
        />
      ) : (
        <div className="rounded-xl border bg-card p-4">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <p className="text-sm text-muted-foreground">{match.name}</p>
              {match.description.trim() && (
                <p className="mt-1 whitespace-pre-line text-sm">
                  {match.description}
                </p>
              )}
            </div>
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

          {/* Score header */}
          {scoreInfo ? (
            <div className="mt-3 flex items-center justify-between">
              <div className="flex-1">
                <p className={cn('text-sm', aWon && 'font-medium')}>{nameA}</p>
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
              <div className="flex-1 text-right">
                <p className={cn('text-sm', bWon && 'font-medium')}>{nameB}</p>
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

          <div className="mt-3 flex items-center justify-between">
            <StatusBadge status={matchBadgeStatus(match)} />
            {canEdit && !cancelled && (
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
      )}

      {/* Result editor — opens below the card when editing the score. */}
      {editingResult && (
        <MatchResultEditor
          match={match}
          onDone={() => setEditingResult(false)}
        />
      )}

      {/* Invitation banner: the viewer has a pending invite to this match. */}
      <InviteBanner match={match} currentUserId={currentUserId} />

      {/* Confirm / dispute (only when the viewer's side owes a response) */}
      {match.pending_score && (
        <ScoreConfirmationBar
          match={match}
          currentUserId={currentUserId}
          variant="detail"
        />
      )}

      {/* Rosters, one column per side */}
      <div className="grid grid-cols-2 gap-3">
        <SideRoster
          title={nameA}
          players={match.players.filter((p) => p.side_id === sideA?.id)}
        />
        <SideRoster
          title={nameB}
          players={match.players.filter((p) => p.side_id === sideB?.id)}
        />
      </div>

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
 * The match's like control + count. Anyone signed in can like a match (not just
 * participants). Optimistic via `useToggleLike`, so the flame fills and the
 * count moves the instant it's pressed.
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
    </div>
  )
}

/**
 * Shown when the signed-in viewer has a pending invitation to this match: a
 * prominent Accept/Decline banner wired to `POST /invitations/:id/respond`.
 * On success it refreshes the match (so the roster/badge update) and the
 * notification badge (the matching invite notification is now handled).
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

  return (
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
          {respond.isError && (
            <p className="mt-1 text-xs text-red-600">
              Something went wrong. Please try again.
            </p>
          )}
          <div className="mt-3 flex gap-2">
            <Button
              size="sm"
              disabled={respond.isPending}
              onClick={() => respond.mutate('accepted')}
            >
              {respond.isPending ? 'Saving…' : 'Accept'}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={respond.isPending}
              onClick={() => respond.mutate('declined')}
            >
              Decline
            </Button>
          </div>
        </div>
      </div>
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
          const pending =
            p.member.invitation && p.member.invitation.status === 'pending'
          // Token-invited (external) players have a shareable link; offer to
          // copy it instead of the bare "invited" label.
          const inviteToken = memberInviteToken(p.member)
          return (
            <div key={i} className="flex items-center gap-2">
              <Avatar name={name} size="md" />
              <span className="flex-1 truncate text-sm">{name}</span>
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
