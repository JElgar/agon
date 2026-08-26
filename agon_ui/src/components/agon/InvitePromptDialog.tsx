import { useMutation } from '@tanstack/react-query'
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

type InvitationResponse = components['schemas']['InvitationResponse']

export interface InvitePromptDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Display name of what's being joined (match or team name). */
  name: string
  /** Trailing qualifier appended after the name, e.g. ' as a member' for a team. */
  suffix?: string
  /** Perform the invitation accept/decline call itself. Should throw on failure. */
  respond: (response: InvitationResponse) => Promise<void>
  /** Called once the response has succeeded. */
  onSuccess: (response: InvitationResponse) => void
}

/**
 * The "you're invited" popup that greets the viewer the first time they open
 * a match/team page they've been invited to (driven by `useInvitePrompt` —
 * see its doc comment for the "first time" bookkeeping). Puts the decision
 * right in front of them, without an extra confirm step: Accept (primary)
 * and Decline (secondary) both respond immediately, and the corner close
 * button (from `DialogContent`) just dismisses — for someone who'd rather
 * look around before deciding — without responding either way. They can
 * still accept/decline afterwards from the page's own banner.
 */
export function InvitePromptDialog({
  open,
  onOpenChange,
  name,
  suffix = '',
  respond,
  onSuccess,
}: InvitePromptDialogProps) {
  const mutation = useMutation({
    mutationFn: (response: InvitationResponse) => respond(response),
    onSuccess: (_data, response) => {
      onOpenChange(false)
      onSuccess(response)
    },
  })

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !mutation.isPending && onOpenChange(next)}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            You're invited to {name}
            {suffix}
          </DialogTitle>
          <DialogDescription>
            Accept to join, or decline if you can't make it — or close this and
            decide later.
          </DialogDescription>
        </DialogHeader>

        {mutation.isError && (
          <p className="text-sm text-destructive">
            Something went wrong. Please try again.
          </p>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            disabled={mutation.isPending}
            onClick={() => mutation.mutate('declined')}
          >
            {mutation.isPending && mutation.variables === 'declined'
              ? 'Declining…'
              : 'Decline'}
          </Button>
          <Button
            disabled={mutation.isPending}
            onClick={() => mutation.mutate('accepted')}
          >
            {mutation.isPending && mutation.variables === 'accepted'
              ? 'Accepting…'
              : 'Accept'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
