import { useMutation, useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog'
import { SportBadge } from '@/components/agon/SportBadge'
import { displayScore, headlineBySide, headlineLabel } from '@/lib/score'
import { cricketScoreFrom, formatOvers, sideNameFor } from '@/lib/cricketScore'
import { RosterConflictError, offersWaitlist } from '@/lib/waitlist'
import { sideDisplayName } from '@/lib/members'

type InvitationResponse = components['schemas']['InvitationResponse']
type Match = components['schemas']['Match']

export interface InvitationResponseDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Which flow to show. `null` while closed/between opens. */
  action: 'accept' | 'decline' | null
  /** Display name of what's being joined/declined (match or team name). */
  name: string
  /** Trailing qualifier appended after the name, e.g. ' as a member' for a team. */
  suffix?: string
  /** Present only for a match invite — fetched to preview the score. */
  matchId?: string
  /** Perform the invitation accept/decline call itself (by-id vs by-token
   *  differs per surface). Should throw on failure. */
  respond: (response: InvitationResponse) => Promise<void>
  /** Called once the response has succeeded. */
  onSuccess: (response: InvitationResponse) => void
}

/**
 * Shared accept/decline dialog for match and team invitations. For a match
 * invite it fetches the match (reusing the cache if it's already loaded
 * elsewhere, e.g. the match detail page) to preview the score.
 */
export function InvitationResponseDialog({
  open,
  onOpenChange,
  action,
  name,
  suffix = '',
  matchId,
  respond,
  onSuccess,
}: InvitationResponseDialogProps) {
  const matchQuery = useQuery({
    queryKey: ['match', matchId],
    enabled: open && !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  const match = matchQuery.data
  const preview = match ? displayScore(match) : null
  const cricketScore = preview ? cricketScoreFrom(preview.score) : null

  const mutation = useMutation({
    mutationFn: async (): Promise<InvitationResponse | undefined> => {
      if (!action) return undefined
      const response: InvitationResponse = action === 'accept' ? 'accepted' : 'declined'
      await respond(response)
      return response
    },
    onSuccess: (response) => {
      onOpenChange(false)
      if (response) onSuccess(response)
    },
  })

  // Offered only when accepting hit a full match/side (`mutation.error` is a
  // `RosterConflictError` — see below): accepts the still-pending invitation
  // and queues for the spot in one step, same endpoint the self-serve join
  // banners use for their own full-side case.
  const joinWaitlist = useMutation({
    mutationFn: async () => {
      if (!matchId) throw new Error('no match to queue for')
      const { error } = await fetchClient.POST('/matches/{match_id}/waitlist/join', {
        params: { path: { match_id: matchId } },
        body: {},
      })
      if (error) throw new Error('Failed to join the waitlist')
    },
    onSuccess: () => {
      onOpenChange(false)
      onSuccess('accepted')
    },
  })

  if (!action) return null

  const conflict = mutation.error instanceof RosterConflictError ? mutation.error.conflict : null

  const isAccept = action === 'accept'

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !mutation.isPending && onOpenChange(next)}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {isAccept ? `Join ${name}${suffix}?` : `Decline invite to ${name}?`}
          </DialogTitle>
          <DialogDescription>
            {isAccept
              ? "You'll be added to the roster."
              : "You are declining this invite — you won't be added."}
          </DialogDescription>
        </DialogHeader>

        {matchId && match && preview && cricketScore && (
          <div className="rounded-lg border bg-muted/40 px-3 py-2.5">
            <span className="text-sm font-medium">
              {sideDisplayName(match.sides[0]) ?? 'Side A'}
              <span className="text-muted-foreground"> vs </span>
              {sideDisplayName(match.sides[1]) ?? 'Side B'}
            </span>
            <div className="mt-1.5 space-y-0.5">
              {cricketScore.innings.map((inn, i) => (
                <p key={i} className="text-sm font-medium leading-tight tracking-tight">
                  {sideNameFor(match, inn.batting_side_id)} {inn.runs}/{inn.wickets}
                  <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                    ({formatOvers(inn.overs)} ov)
                  </span>
                </p>
              ))}
            </div>
            <div className="mt-1 text-[10px] uppercase tracking-widest text-muted-foreground">
              result
            </div>
          </div>
        )}
        {matchId && match && preview && !cricketScore && (
          <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/40 px-3 py-2.5">
            <div className="flex items-center gap-3 text-sm">
              <span className="font-medium">
                {sideDisplayName(match.sides[0]) ?? 'Side A'}
                <span className="text-muted-foreground"> vs </span>
                {sideDisplayName(match.sides[1]) ?? 'Side B'}
              </span>
            </div>
            <div className="text-right">
              <div className="text-lg font-medium tracking-tight">
                {headlineBySide(preview.score)[match.sides[0]?.id ?? ''] ?? 0}
                <span className="text-muted-foreground">–</span>
                {headlineBySide(preview.score)[match.sides[1]?.id ?? ''] ?? 0}
              </div>
              <div className="text-[10px] uppercase tracking-widest text-muted-foreground">
                {headlineLabel(preview.score)}
              </div>
            </div>
          </div>
        )}
        {matchId && match && !preview && (
          <div className="flex items-center gap-2 rounded-lg border bg-muted/40 px-3 py-2.5 text-sm text-muted-foreground">
            <SportBadge sport={match.match_type} />
            No score recorded yet.
          </div>
        )}

        {conflict && (
          <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3">
            <p className="text-sm text-destructive">{conflict.message}</p>
            {matchId && offersWaitlist(conflict) && (
              <>
                <Button
                  size="sm"
                  variant="outline"
                  className="mt-2"
                  disabled={joinWaitlist.isPending}
                  onClick={() => joinWaitlist.mutate()}
                >
                  {joinWaitlist.isPending
                    ? 'Joining waiting list…'
                    : 'Join the waiting list instead'}
                </Button>
                {joinWaitlist.isError && (
                  <p className="mt-2 text-xs text-destructive">
                    Something went wrong. Try again.
                  </p>
                )}
              </>
            )}
          </div>
        )}
        {mutation.isError && !conflict && (
          <p className="text-sm text-destructive">
            {isAccept ? 'Failed to join.' : 'Failed to decline.'} Please try
            again.
          </p>
        )}

        <DialogFooter>
          <Button
            variant="ghost"
            disabled={mutation.isPending}
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            variant={isAccept ? 'default' : 'destructive'}
            disabled={mutation.isPending}
            onClick={() => mutation.mutate()}
          >
            {mutation.isPending
              ? isAccept
                ? 'Joining…'
                : 'Declining…'
              : isAccept
                ? 'Join'
                : 'Decline'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
