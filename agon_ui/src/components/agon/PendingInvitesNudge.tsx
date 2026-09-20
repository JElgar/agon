import { useState } from 'react'
import { Info } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { pendingInvitees } from '@/lib/members'
import { PendingInvitesDialog } from '@/components/agon/PendingInvitesDialog'

type Match = components['schemas']['Match']

/**
 * Dismissible heads-up, shown before kickoff and before publishing a result,
 * that some invited players never responded — a nudge to review and revoke
 * stale ones rather than leave them looking like unresolved business.
 * Renders nothing once there's nobody pending. See `PendingInvitesDialog`
 * for the actual review/revoke flow this opens.
 */
export function PendingInvitesNudge({ match }: { match: Match }) {
  const [open, setOpen] = useState(false)
  const invitees = pendingInvitees(match)

  if (invitees.length === 0) return null

  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/40 px-3 py-2.5 text-sm">
      <div className="flex items-center gap-2 text-muted-foreground">
        <Info className="size-4 shrink-0" />
        <span>
          {invitees.length} invited {invitees.length === 1 ? 'player hasn’t' : 'players haven’t'}{' '}
          joined
        </span>
      </div>
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
        Review
      </Button>
      <PendingInvitesDialog open={open} onOpenChange={setOpen} match={match} />
    </div>
  )
}
