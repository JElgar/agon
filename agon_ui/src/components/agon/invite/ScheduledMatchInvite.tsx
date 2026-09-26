import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Calendar, CalendarPlus, ChevronLeft, Pencil, MapPin, Share } from 'lucide-react'
import type { components } from '@/types/api'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { scheduledDateTime } from '@/lib/datetime'
import { directionsUrl } from '@/lib/location'
import { downloadMatchIcs } from '@/lib/calendar'
import { respondToInvitation } from '@/lib/invitations'
import { PersonAvatar, SideSwatch } from '@/components/agon/football/FootballMatchView'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import {
  memberAvatarUrl,
  memberName,
  myPendingInvitation,
  sideTeamHint,
  withInvitationStatus,
} from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Players who've actually said yes (or never needed to — added ad-hoc, no
 *  invitation at all) — a pending or declined invitee doesn't belong in the
 *  "going" grid. Mirrors `isParticipant`'s own per-player check. */
function goingPlayers(match: Match) {
  return match.players.filter((p) => {
    const invitation = p.member.invitation
    return !invitation || invitation.status === 'accepted'
  })
}

/** The match's overall player cap, only when every side has one set — same
 *  rule as `matchPlayerTotalLabel`, just returning the number instead of the
 *  formatted string (needed here for the progress bar's percentage). */
function overallCap(match: Match): number | undefined {
  const caps = match.sides.map((s) => s.max_players)
  return caps.every((c) => c != null) ? caps.reduce<number>((sum, c) => sum + (c ?? 0), 0) : undefined
}

function sideLabel(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/**
 * The redesigned "Invite" screen (`Invite.dc.html`) — a scheduled match's
 * pre-match view: title/when/where, a "going" attendee grid with a progress
 * bar toward the cap, a teams note, who organised it, and a sticky bottom
 * bar. Used for every non-football sport while `match.status === 'scheduled'`
 * — football has its own redesigned scheduled state (`FootballHeroCard`)
 * already, and this page's roster-editing/invite/waitlist/comments tools
 * (unchanged, below) still cover everything the single-viewport mock doesn't
 * need to show at all: admin roster edits, the waitlist, join links.
 */
export function ScheduledMatchInvite({
  match,
  currentUserId,
  canEdit,
  onBack,
  onShare,
  onEdit,
}: {
  match: Match
  currentUserId?: string
  canEdit: boolean
  onBack: () => void
  onShare: () => void
  onEdit: () => void
}) {
  const queryClient = useQueryClient()
  const [sideA, sideB] = match.sides
  const going = goingPlayers(match)
  const cap = overallCap(match)
  const spotsLeft = cap != null ? Math.max(cap - going.length, 0) : undefined
  const pct = cap ? Math.min(100, Math.round((going.length / cap) * 100)) : 0
  // A side counts as "picked on the night" (no fixed roster split yet) once
  // neither side is linked to a real team — the same signal `sideTeamHint`
  // uses to tell a persistent team from an ad-hoc one.
  const adHocTeams = match.sides.length === 2 && !sideTeamHint(sideA) && !sideTeamHint(sideB)
  const organiser = match.players.find((p) => p.role === 'owner')
  const metaTeamSide = match.sides.find((s) => s.team_name)

  const invitation = myPendingInvitation(match, currentUserId)
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)
  const matchKey = ['match', match.id]

  const respond = useMutation({
    mutationFn: async (response: components['schemas']['InvitationResponse']) => {
      if (!invitation) return
      await respondToInvitation(invitation.id, response)
    },
    onMutate: async (response) => {
      if (!currentUserId) return
      await queryClient.cancelQueries({ queryKey: matchKey })
      const previous = queryClient.getQueryData<Match>(matchKey)
      const status = response === 'accepted' ? 'accepted' : 'declined'
      if (previous) {
        queryClient.setQueryData<Match>(matchKey, withInvitationStatus(previous, currentUserId, status))
      }
      return { previous }
    },
    onError: (_err, _response, context) => {
      if (context?.previous) queryClient.setQueryData(matchKey, context.previous)
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: matchKey })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['notifications'] })
      queryClient.invalidateQueries({ queryKey: ['notifications-unread-count'] })
    },
  })

  return (
    <>
      <div className="-mx-2 -mt-5 flex items-center justify-between">
        <button
          type="button"
          onClick={onBack}
          aria-label="Back"
          className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted"
        >
          <ChevronLeft className="size-[22px]" strokeWidth={2.2} />
        </button>
        <div className="flex items-center">
          {canEdit && (
            <button
              type="button"
              onClick={onEdit}
              aria-label="Edit match details"
              className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted"
            >
              <Pencil className="size-[19px]" />
            </button>
          )}
          <button
            type="button"
            onClick={onShare}
            aria-label="Share invite"
            className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted"
          >
            <Share className="size-[22px]" />
          </button>
        </div>
      </div>

      <div className="flex flex-col gap-3.5 px-1">
        {metaTeamSide && (
          <div className="flex items-center gap-2.5">
            <SideSwatch index={match.sides.indexOf(metaTeamSide)} size={32} />
            <span className="text-sm font-semibold text-muted-foreground">{metaTeamSide.team_name}</span>
          </div>
        )}
        <h1 className="font-display text-[34px] leading-[1.05] font-extrabold tracking-[-0.5px]">{match.name}</h1>
        <div className="flex flex-col gap-2.5">
          <div className="flex items-center gap-3 text-base">
            <Calendar className="size-5 shrink-0 text-primary" />
            <span>
              <b className="font-semibold">{scheduledDateTime(match.starts_at)}</b>
            </span>
          </div>
          {match.location && (
            <div className="flex items-center gap-3 text-base">
              <MapPin className="size-5 shrink-0 text-primary" />
              <span className="truncate">{match.location.text}</span>
              {directionsUrl(match.location) && (
                <a
                  href={directionsUrl(match.location)}
                  target="_blank"
                  rel="noreferrer"
                  className="shrink-0 font-semibold text-primary hover:underline"
                >
                  Directions
                </a>
              )}
            </div>
          )}
        </div>
      </div>

      <Card className="flex flex-col gap-3.5 p-[18px]">
        <div className="flex items-baseline gap-2">
          <span className="font-display text-[30px] font-extrabold">{going.length} going</span>
          {cap != null && <span className="flex-grow text-[15px] text-muted-foreground">of {cap}</span>}
          {spotsLeft != null && (
            <span className="text-sm font-semibold text-primary">
              {spotsLeft} {spotsLeft === 1 ? 'spot' : 'spots'} left
            </span>
          )}
        </div>
        {cap != null && (
          <div className="h-2 overflow-hidden rounded-full bg-muted">
            <div className="h-2 rounded-full bg-primary" style={{ width: `${pct}%` }} />
          </div>
        )}
        {going.length > 0 && (
          <div className="grid grid-cols-2 gap-x-2.5 gap-y-3 pt-1">
            {going.map((p) => (
              <div key={p.member.id} className="flex items-center gap-2.5">
                <PersonAvatar name={memberName(p.member)} imageUrl={memberAvatarUrl(p.member)} size={32} />
                <span className="truncate text-sm font-medium">{memberName(p.member)}</span>
              </div>
            ))}
          </div>
        )}
      </Card>

      {adHocTeams && (
        <Card className="flex flex-col gap-3 p-[18px]">
          <span className="text-[13px] font-semibold text-muted-foreground">Teams</span>
          <div className="flex items-center gap-2.5">
            <SideSwatch index={0} size={14} />
            <span className="text-[17px] font-bold">{sideLabel(sideA, 'Side A')}</span>
            <span className="px-1 text-sm text-muted-foreground">vs</span>
            <SideSwatch index={1} size={14} />
            <span className="text-[17px] font-bold">{sideLabel(sideB, 'Side B')}</span>
          </div>
          <span className="text-sm text-muted-foreground">Teams get picked on the night.</span>
        </Card>
      )}

      {organiser && (
        <div className="flex items-center gap-3 px-1">
          <PersonAvatar name={memberName(organiser.member)} imageUrl={memberAvatarUrl(organiser.member)} size={36} />
          <span className="text-sm text-muted-foreground">
            Organised by <b className="font-semibold text-foreground">{memberName(organiser.member)}</b>
          </span>
        </div>
      )}

      <div className="sticky bottom-0 z-10 -mx-4 mt-2 flex flex-col gap-2 border-t bg-card px-4 pt-3 pb-[max(12px,env(safe-area-inset-bottom))] md:mx-0 md:rounded-2xl md:border">
        <div className="flex gap-2.5">
          {invitation ? (
            <Button
              size="lg"
              shape="pill"
              className="h-[54px] flex-1 gap-2 rounded-2xl text-base font-bold"
              onClick={() => setAction('accept')}
            >
              I'm in
            </Button>
          ) : (
            <button
              type="button"
              className="flex h-[54px] flex-1 items-center justify-center gap-2 rounded-2xl bg-primary text-base font-bold text-primary-foreground transition-opacity hover:opacity-90"
              onClick={() => downloadMatchIcs(match, { title: match.name, description: `${sideLabel(sideA, 'Side A')} vs ${sideLabel(sideB, 'Side B')}` })}
            >
              <CalendarPlus className="size-5" /> Add to calendar
            </button>
          )}
          {invitation && (
            <button
              type="button"
              aria-label="Add to calendar"
              onClick={() => downloadMatchIcs(match, { title: match.name, description: `${sideLabel(sideA, 'Side A')} vs ${sideLabel(sideB, 'Side B')}` })}
              className="box-border flex size-[54px] shrink-0 items-center justify-center rounded-2xl border bg-card"
            >
              <CalendarPlus className="size-[22px]" />
            </button>
          )}
        </div>
        {invitation && (
          <button
            type="button"
            className="h-11 bg-transparent text-[15px] font-semibold text-muted-foreground"
            onClick={() => setAction('decline')}
          >
            Can't make it
          </button>
        )}
      </div>

      <InvitationResponseDialog
        open={action !== null}
        onOpenChange={(open) => !open && setAction(null)}
        action={action}
        name={match.name}
        matchId={match.id}
        respond={(response) => respond.mutateAsync(response)}
        onSuccess={() => {
          setAction(null)
          queryClient.invalidateQueries({ queryKey: matchKey })
          queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
        }}
      />
    </>
  )
}
