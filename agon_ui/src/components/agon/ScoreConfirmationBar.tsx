import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Check, X, Clock } from 'lucide-react'
import type { components } from '@/types/api'
import { fetchClient } from '@/lib/api-client'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { confirmationState } from '@/lib/confirmation'

type Match = components['schemas']['Match']

export interface ScoreConfirmationBarProps {
  match: Match
  currentUserId: string | undefined
  /** Compact layout for the feed card vs. a fuller block on match detail. */
  variant?: 'card' | 'detail'
  className?: string
}

/**
 * The confirm / dispute prompt for a match's pending (unconfirmed) score.
 *
 * Shown only when the match has a pending score AND the viewer is a participant.
 * If the viewer's side hasn't responded yet → Confirm / Dispute buttons; if it
 * already confirmed (or the viewer submitted it) → a passive "awaiting the other
 * side" note. Confirm/Dispute POST to the respond endpoint and, on success,
 * invalidate the feed + this match so the promoted/cleared score re-renders.
 * Renders nothing when there's nothing to act on.
 */
export function ScoreConfirmationBar({
  match,
  currentUserId,
  variant = 'card',
  className,
}: ScoreConfirmationBarProps) {
  const queryClient = useQueryClient()
  const state = confirmationState(match, currentUserId)

  const respond = useMutation({
    mutationFn: async (response: 'confirm' | 'dispute') => {
      if (!state.submissionId) throw new Error('no pending submission')
      const { data, error } = await fetchClient.POST(
        '/matches/{match_id}/score-submissions/{submission_id}/respond',
        {
          params: {
            path: { match_id: match.id, submission_id: state.submissionId },
          },
          body: { response },
        },
      )
      if (error || !data) throw new Error('Failed to respond to the score')
      return data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      queryClient.invalidateQueries({ queryKey: ['profile-activity'] })
    },
  })

  // Nothing pending, or the viewer isn't a participant → render nothing.
  if (!state.submissionId || !state.mySideId) return null

  if (state.awaitingOthers) {
    return (
      <div
        className={cn(
          'flex items-center gap-1.5 text-xs text-muted-foreground',
          className,
        )}
      >
        <Clock className="size-3.5" />
        Awaiting confirmation from the other side
      </div>
    )
  }

  if (!state.canRespond) return null

  return (
    <div
      className={cn(
        'flex flex-col gap-2 rounded-lg border border-warning/30 bg-warning/10 p-3',
        variant === 'card' && 'gap-1.5 p-2.5',
        className,
      )}
    >
      <p
        className={cn(
          'text-sm font-medium text-foreground',
          variant === 'card' && 'text-xs',
        )}
      >
        Confirm this result?
      </p>
      {respond.isError && (
        <p className="text-xs text-destructive">
          {(respond.error as Error).message}
        </p>
      )}
      <div className="flex gap-2">
        <Button
          size="sm"
          className="flex-1"
          disabled={respond.isPending}
          onClick={() => respond.mutate('confirm')}
        >
          <Check className="size-4" /> Confirm
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="flex-1"
          disabled={respond.isPending}
          onClick={() => respond.mutate('dispute')}
        >
          <X className="size-4" /> Dispute
        </Button>
      </div>
    </div>
  )
}
