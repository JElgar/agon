import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import {
  Calendar,
  CalendarPlus,
  ChevronLeft,
  Link2,
  LogOut,
  MoreVertical,
  Pencil,
  MapPin,
  Radio,
  Share,
  ShieldMinus,
  ShieldPlus,
  UserPlus,
} from 'lucide-react'
import type { components } from '@/types/api'
import { fetchClient } from '@/lib/api-client'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { scheduledDateTime } from '@/lib/datetime'
import { directionsUrl } from '@/lib/location'
import { addMatchToCalendar } from '@/lib/calendar'
import { respondToInvitation } from '@/lib/invitations'
import {
  MatchTabBar,
  PersonAvatar,
  SideSwatch,
  CommentsPreviewCard,
} from '@/components/agon/football/FootballMatchView'
import { InvitationResponseDialog } from '@/components/agon/InvitationResponseDialog'
import { LeaveMatchDialog } from '@/components/agon/invite/LeaveMatchDialog'
import { WaitlistSection } from '@/components/agon/WaitlistSection'
import { MatchRosterEditor } from '@/components/agon/MatchRosterEditor'
import { InvitePlayers } from '@/components/agon/InvitePlayers'
import { MatchJoinLinksDialog } from '@/components/agon/MatchJoinLinksDialog'
import { MatchComments } from '@/components/agon/MatchComments'
import { teamCrestColor } from '@/components/agon/RedesignedSportCard'
import {
  memberAvatarUrl,
  memberName,
  myPendingInvitation,
  playerId,
  sideDisplayName,
  withInvitationStatus,
} from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']

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
  return sideDisplayName(side) ?? fallback
}

type InviteTab = 'details' | 'teams'
const INVITE_TABS: { id: InviteTab; label: string }[] = [
  { id: 'details', label: 'Details' },
  { id: 'teams', label: 'Teams' },
]

/** Sport that records live events as it's played (the other sports are
 *  scored after the fact via `LogMatchPage`) — the only ones with
 *  somewhere for "Start scoring" to actually send an admin. */
function isLiveScoredSport(matchType: Match['match_type']): boolean {
  return matchType === 'football' || matchType === 'cricket' || matchType === 'netball'
}

/** "MON" / "29" — the desktop layout's mini date tile (`DesktopInvite.dc.html`). */
function dateTileParts(startsAt: string): { month: string; day: string } {
  const d = new Date(startsAt)
  return {
    month: d.toLocaleDateString(undefined, { month: 'short' }).toUpperCase(),
    day: String(d.getDate()),
  }
}

/** "Organised by ..." row — shared the same way the going list and teams
 *  tab are between the mobile flow and the desktop layout. */
function OrganizerRow({ organiser }: { organiser: MatchPlayer }) {
  return (
    <div className="flex items-center gap-3 px-1">
      <PersonAvatar name={memberName(organiser.member)} imageUrl={memberAvatarUrl(organiser.member)} size={36} />
      <span className="text-sm text-muted-foreground">
        Organised by <b className="font-semibold text-foreground">{memberName(organiser.member)}</b>
      </span>
    </div>
  )
}

/** The Teams tab's own side identity dot: an ad-hoc or derby side's own
 *  stored `colour` first (the server only ever sets it for those — see
 *  `MatchSide.colour`'s doc comment), else a team-linked side's
 *  `teamCrestColor` (the same deterministic colour its crest uses
 *  elsewhere — the schema has no real team-colour field), falling back to
 *  the plain index-based `SideSwatch` only when neither is set. Checking
 *  `colour` before `team_id` matters for a derby side, which has both. */
function TeamRosterSwatch({ side, index }: { side: MatchSide | undefined; index: number }) {
  const colour = side?.colour ?? (side?.team_id ? teamCrestColor(side.team_id) : undefined)
  if (colour) {
    return <span className="inline-block size-3.5 shrink-0 rounded-[5px]" style={{ backgroundColor: colour }} />
  }
  return <SideSwatch index={index} size={14} />
}

/** One side's roster on the Teams tab — read-only; editing who's on which
 *  side happens through `MatchRosterEditor`, surfaced alongside this via the
 *  "Edit roster" button rather than inline here. */
function TeamRosterCard({
  side,
  index,
  fallback,
  players,
}: {
  side: MatchSide | undefined
  index: number
  fallback: string
  players: MatchPlayer[]
}) {
  return (
    <Card className="flex flex-col gap-1 p-[18px]">
      <div className="flex items-center gap-2.5 pb-2">
        <TeamRosterSwatch side={side} index={index} />
        <span className="font-display flex-grow text-[18px] font-extrabold">{sideLabel(side, fallback)}</span>
        <span className="text-[13px] text-muted-foreground">{players.length} players</span>
      </div>
      {players.length === 0 ? (
        <p className="py-2 text-sm text-muted-foreground">
          No one assigned yet — teams get picked on the night.
        </p>
      ) : (
        players.map((p) => (
          <div key={p.member.id} className="flex items-center gap-2.5 border-t border-border/60 py-2 first:border-t-0">
            <PersonAvatar name={memberName(p.member)} imageUrl={memberAvatarUrl(p.member)} size={32} />
            <span className="truncate text-sm font-medium">{memberName(p.member)}</span>
          </div>
        ))
      )}
    </Card>
  )
}

/**
 * The redesigned "Invite" screen (`Invite.dc.html` / `DesktopInvite.dc.html`,
 * plus the Teams-tab state in `InviteTeams.dc.html`/`DesktopInviteTeams.dc.html`)
 * — a scheduled match's pre-match view, split into a "Details" tab
 * (title/when/where, a "going" list with a progress bar toward the cap and
 * admin role controls, the waitlist, who organised it) and a "Teams" tab
 * (each side's roster, plus the roster/invite/join-link admin tools), with
 * either a sticky mobile bottom bar or (at the `xl` desktop breakpoint,
 * alongside the app shell's sidebar) a right-hand action card replacing it.
 * Used for every sport while `match.status === 'scheduled'`.
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
  const organiser = match.players.find((p) => p.role === 'owner')
  const metaTeamSide = match.sides.find((s) => s.team_name)
  const viewerGoing = going.some((p) => p.member.type === 'User' && p.member.user_id === currentUserId)
  const { month, day } = dateTileParts(match.starts_at)
  const description = `${sideLabel(sideA, 'Side A')} vs ${sideLabel(sideB, 'Side B')}`
  // Football/netball gate the clock behind a short pre-match setup screen;
  // cricket has no equivalent step and goes straight into scoring — same
  // split `MatchDetailPage` uses for its own "Start scoring" entry.
  const liveEntryPath =
    match.match_type === 'cricket' || match.match_type === 'netball'
      ? `/matches/${match.id}/live`
      : `/matches/${match.id}/live/setup`

  const invitation = myPendingInvitation(match, currentUserId)
  const [tab, setTab] = useState<InviteTab>('details')
  const [action, setAction] = useState<'accept' | 'decline' | null>(null)
  const [editingRoster, setEditingRoster] = useState(false)
  const [inviting, setInviting] = useState(false)
  const [commentsOpen, setCommentsOpen] = useState(false)
  const myPlayer = match.players.find((p) => p.member.type === 'User' && p.member.user_id === currentUserId)
  const matchKey = ['match', match.id]

  const setPlayerRole = useMutation({
    mutationFn: async ({ playerId: pid, role }: { playerId: string; role: 'admin' | 'player' }) => {
      const { error } = await fetchClient.POST('/matches/{match_id}/players/{player_id}/role', {
        params: { path: { match_id: match.id, player_id: pid } },
        body: { role },
      })
      if (error) throw new Error('Failed to update role')
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: matchKey }),
  })

  const [leaveDialogOpen, setLeaveDialogOpen] = useState(false)

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

  const addToCalendar = () => addMatchToCalendar(match, { title: match.name, description })

  return (
    <>
      <div className="-mx-2 -mt-5 flex items-center justify-between">
        <button
          type="button"
          onClick={onBack}
          aria-label="Back"
          className="flex h-11 items-center gap-1 rounded-full px-2 text-foreground hover:bg-muted xl:px-3"
        >
          <ChevronLeft className="size-[22px]" strokeWidth={2.2} />
          <span className="hidden text-[15px] font-semibold xl:inline">Back</span>
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
          {/* Desktop's own action card has its own "Share invite" affordance
              (`DesktopInvite.dc.html`) — no need for a second one up here. */}
          <button
            type="button"
            onClick={onShare}
            aria-label="Share invite"
            className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted xl:hidden"
          >
            <Share className="size-[22px]" />
          </button>
          {myPlayer && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label="More options"
                  className="flex size-11 items-center justify-center rounded-full text-foreground hover:bg-muted"
                >
                  <MoreVertical className="size-[22px]" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem
                  className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                  onSelect={() => setLeaveDialogOpen(true)}
                >
                  <LogOut /> Leave match
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-4 xl:grid xl:grid-cols-[minmax(0,1fr)_380px] xl:items-start xl:gap-6">
        <div className="flex flex-col gap-4">
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

          <MatchTabBar tabs={INVITE_TABS} value={tab} onChange={setTab} />

          {tab === 'details' && (
            <>
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
              </Card>

              {canEdit && isLiveScoredSport(match.match_type) && (
                <Link
                  to={liveEntryPath}
                  className="flex h-12 items-center justify-center gap-2 rounded-2xl bg-foreground text-base font-bold text-background transition-opacity hover:opacity-90"
                >
                  <Radio className="size-5" /> Start scoring
                </Link>
              )}

              {going.length > 0 && (
                <Card className="flex flex-col p-0">
                  <span className="px-[18px] pt-3 pb-1 text-[13px] font-semibold text-muted-foreground">
                    Who's going
                  </span>
                  {going.map((p) => {
                    const id = playerId(p)
                    const isOwnerRow = p.role === 'owner'
                    const isYou = myPlayer && id === playerId(myPlayer)
                    return (
                      <div
                        key={id}
                        className="flex items-center gap-2.5 border-t border-border/60 px-[18px] py-2.5 first:border-t-0"
                      >
                        <PersonAvatar name={memberName(p.member)} imageUrl={memberAvatarUrl(p.member)} size={36} />
                        <div className="min-w-0 flex-1">
                          <span className="block truncate text-[15px] font-medium">
                            {memberName(p.member)}
                            {isYou && ' (you)'}
                          </span>
                          {p.role !== 'player' && (
                            <span className="block text-xs text-muted-foreground capitalize">{p.role}</span>
                          )}
                        </div>
                        {canEdit && !isOwnerRow && (
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="size-8 shrink-0"
                                disabled={setPlayerRole.isPending}
                                aria-label={`${memberName(p.member)} options`}
                              >
                                <MoreVertical className="size-4" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent>
                              {p.role === 'admin' ? (
                                <DropdownMenuItem
                                  disabled={setPlayerRole.isPending}
                                  onSelect={() => setPlayerRole.mutate({ playerId: id, role: 'player' })}
                                >
                                  <ShieldMinus /> Remove admin
                                </DropdownMenuItem>
                              ) : (
                                <DropdownMenuItem
                                  disabled={setPlayerRole.isPending}
                                  onSelect={() => setPlayerRole.mutate({ playerId: id, role: 'admin' })}
                                >
                                  <ShieldPlus /> Make admin
                                </DropdownMenuItem>
                              )}
                            </DropdownMenuContent>
                          </DropdownMenu>
                        )}
                      </div>
                    )
                  })}
                </Card>
              )}

              <WaitlistSection match={match} currentUserId={currentUserId} canManage={canEdit} />

              {/* Below the fold on mobile; on desktop this moves into the
                  action card's column instead (right-hand `xl:flex` below). */}
              {organiser && (
                <div className="xl:hidden">
                  <OrganizerRow organiser={organiser} />
                </div>
              )}
            </>
          )}

          {tab === 'teams' && (
            <>
              {match.sides.map((side, i) => (
                <TeamRosterCard
                  key={side.id}
                  side={side}
                  index={i}
                  fallback={i === 0 ? 'Side A' : 'Side B'}
                  players={going.filter((p) => p.side_id === side.id)}
                />
              ))}

              {canEdit && (
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
              {canEdit && editingRoster && (
                <MatchRosterEditor match={match} onDone={() => setEditingRoster(false)} />
              )}
              {canEdit && inviting && <InvitePlayers match={match} onDone={() => setInviting(false)} />}
            </>
          )}

          <CommentsPreviewCard
            match={match}
            viewerName={myPlayer ? memberName(myPlayer.member) : undefined}
            viewerAvatar={myPlayer ? memberAvatarUrl(myPlayer.member) : undefined}
            onOpen={() => setCommentsOpen(true)}
          />
        </div>

        {/* Desktop-only: replaces the mobile sticky bottom bar with a
            standing action card in the right column, alongside the app
            shell's sidebar (`DesktopInvite.dc.html`). */}
        <div className="hidden flex-col gap-4 xl:flex">
          <Card className="flex flex-col gap-3.5 p-[22px]">
            <div className="flex items-center gap-3.5">
              <div className="flex h-[70px] w-16 shrink-0 flex-col items-center justify-center rounded-2xl bg-primary/10">
                <span className="text-xs font-bold tracking-wider text-primary">{month}</span>
                <span className="font-display text-[30px] leading-none font-extrabold">{day}</span>
              </div>
              <div className="flex flex-col gap-0.5">
                <span className="text-[17px] font-bold">
                  {invitation ? 'Are you playing?' : viewerGoing ? "You're in" : "Don't miss it"}
                </span>
                <span className="text-sm text-muted-foreground">
                  {spotsLeft != null ? `${spotsLeft} ${spotsLeft === 1 ? 'spot' : 'spots'} left` : `${going.length} going`}
                </span>
              </div>
            </div>
            {invitation ? (
              <>
                <Button
                  size="lg"
                  className="h-[54px] rounded-2xl text-base font-bold"
                  onClick={() => setAction('accept')}
                >
                  I'm in
                </Button>
                <Button
                  variant="outline"
                  className="h-12 rounded-2xl text-[15px] font-bold"
                  onClick={() => setAction('decline')}
                >
                  Can't make it
                </Button>
              </>
            ) : (
              <button
                type="button"
                onClick={addToCalendar}
                className="flex h-[54px] items-center justify-center gap-2 rounded-2xl bg-primary text-base font-bold text-primary-foreground transition-opacity hover:opacity-90"
              >
                <CalendarPlus className="size-5" /> Add to calendar
              </button>
            )}
            <div className="flex gap-2.5 border-t pt-1.5">
              {invitation && (
                <button
                  type="button"
                  onClick={addToCalendar}
                  className="flex h-11 flex-1 items-center justify-center gap-2 text-sm font-bold text-foreground"
                >
                  <CalendarPlus className="size-[18px]" /> Add to calendar
                </button>
              )}
              <button
                type="button"
                onClick={onShare}
                className="flex h-11 flex-1 items-center justify-center gap-2 text-sm font-bold text-foreground"
              >
                <Share className="size-[18px]" /> Share invite
              </button>
            </div>
          </Card>
          {organiser && <OrganizerRow organiser={organiser} />}
        </div>
      </div>

      <div className="sticky bottom-0 z-10 -mx-4 mt-auto flex flex-col gap-2 bg-card/95 px-4 pt-3 pb-[max(12px,env(safe-area-inset-bottom))] backdrop-blur supports-[backdrop-filter]:bg-card/80 md:mt-2 xl:hidden">
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
              onClick={addToCalendar}
            >
              <CalendarPlus className="size-5" /> Add to calendar
            </button>
          )}
          {invitation && (
            <button
              type="button"
              aria-label="Add to calendar"
              onClick={addToCalendar}
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

      {myPlayer && (
        <LeaveMatchDialog
          open={leaveDialogOpen}
          onOpenChange={setLeaveDialogOpen}
          match={match}
          myPlayer={myPlayer}
          going={going}
          onLeft={() => setLeaveDialogOpen(false)}
        />
      )}

      <Sheet open={commentsOpen} onOpenChange={setCommentsOpen}>
        <SheetContent side="bottom" className="max-h-[85vh] overflow-y-auto rounded-t-[20px] bg-background p-4">
          <SheetHeader className="p-0">
            <SheetTitle className="font-display text-xl">Comments</SheetTitle>
          </SheetHeader>
          <MatchComments matchId={match.id} currentUserId={currentUserId} />
        </SheetContent>
      </Sheet>
    </>
  )
}
