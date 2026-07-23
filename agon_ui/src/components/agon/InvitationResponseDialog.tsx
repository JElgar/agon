import { useEffect, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog'

type InvitationResponse = components['schemas']['InvitationResponse']

export interface PendingScoreRef {
  matchId: string
  submissionId: string
}

export interface InvitationResponseDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Which flow to show. `null` while closed/between opens. */
  action: 'accept' | 'decline' | null
  /** Display name of what's being joined/declined (match or team name). */
  name: string
  /** Trailing qualifier appended after the name, e.g. ' as a member' for a team. */
  suffix?: string
  /** Present only for a match invite whose score is already awaiting this
   *  invitee's side to confirm it — offers the "also confirm the score" toggle. */
  pendingScore?: PendingScoreRef | null
  /** Perform the invitation accept/decline call itself (by-id vs by-token
   *  differs per surface). Should throw on failure. */
  respond: (response: InvitationResponse) => Promise<void>
  /** Called once the response (and, if applicable, the score confirmation)
   *  has succeeded. */
  onSuccess: (response: InvitationResponse) => void
}

/**
 * Shared accept/decline dialog for match and team invitations. Accepting a
 * match invite whose score is already submitted and awaiting the invitee's
 * side offers a toggle (on by default) to confirm that score in the same
 * action, instead of a separate trip to the match page afterwards. A failed
 * score confirmation doesn't undo the (already-succeeded) invite response —
 * the score can still be confirmed separately from the match page.
 */
export function InvitationResponseDialog({
  open,
  onOpenChange,
  action,
  name,
  suffix = '',
  pendingScore,
  respond,
  onSuccess,
}: InvitationResponseDialogProps) {
  const [confirmScore, setConfirmScore] = useState(true)

  // Default the toggle back on each time the dialog opens.
  useEffect(() => {
    if (open) setConfirmScore(true)
  }, [open])

  const mutation = useMutation({
    mutationFn: async (): Promise<InvitationResponse | undefined> => {
      if (!action) return undefined
      const response: InvitationResponse = action === 'accept' ? 'accepted' : 'declined'
      await respond(response)
      if (action === 'accept' && pendingScore && confirmScore) {
        await fetchClient.POST(
          '/matches/{match_id}/score-submissions/{submission_id}/respond',
          {
            params: {
              path: {
                match_id: pendingScore.matchId,
                submission_id: pendingScore.submissionId,
              },
            },
            body: { response: 'confirm' },
          },
        )
      }
      return response
    },
    onSuccess: (response) => {
      onOpenChange(false)
      if (response) onSuccess(response)
    },
  })

  if (!action) return null

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

        {isAccept && pendingScore && (
          <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/40 px-3 py-2.5">
            <div>
              <p className="text-sm font-medium">Also confirm the score</p>
              <p className="text-xs text-muted-foreground">
                A result's already been submitted — confirm it now instead of
                separately.
              </p>
            </div>
            <Switch
              checked={confirmScore}
              onCheckedChange={setConfirmScore}
              aria-label="Also confirm the score"
            />
          </div>
        )}

        {mutation.isError && (
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
