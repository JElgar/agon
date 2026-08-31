import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link2, Trash2 } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogTrigger,
} from '@/components/ui/dialog'
import { CopyInviteButton } from './CopyInviteButton'

type Match = components['schemas']['Match']
type JoinLink = components['schemas']['JoinLink']
type JoinLinkScope = components['schemas']['JoinLinkScope']

/** Which side(s)/unassigned a new link should be scoped to — the form's own
 *  shape, simpler than `JoinLinkScope` (no `Sides` case with zero ids). */
type ScopeChoice = { kind: 'inherit' } | { kind: 'unassigned' } | { kind: 'sides'; sideIds: string[] }

function scopeInputFor(choice: ScopeChoice): JoinLinkScope {
  // The generated `JoinLinkScope*Inherit`/`*Unassigned` branches are
  // `Record<string, never> & { type: '...' }` — an artifact of how
  // openapi-typescript encodes a discriminated union whose variant has no
  // fields of its own — which TS won't accept as an object literal without
  // a cast (same erased-discriminant issue as `InvitationKind` etc.).
  if (choice.kind === 'unassigned') return { type: 'Unassigned' } as JoinLinkScope
  if (choice.kind === 'sides') return { type: 'Sides', side_ids: choice.sideIds }
  return { type: 'Inherit' } as JoinLinkScope
}

/** Human summary of an existing link's scope, for the list. */
function scopeSummary(scope: JoinLinkScope, match: Match): string {
  if (scope.type === 'Unassigned') return 'Unassigned only'
  if (scope.type === 'Inherit') return "Anyone — follows this game's settings"
  const names = scope.side_ids.map((id, i) => {
    const side = match.sides.find((s) => s.id === id)
    return side?.name?.trim() || `Side ${i + 1}`
  })
  return names.length === 1 ? `Side: ${names[0]}` : `Either: ${names.join(', ')}`
}

/**
 * Manage a match's shareable join links: mint a many-use link scoped to
 * inherit the game's own `join_policy`, always join unassigned, or join one
 * (or, for an intra-squad match, either) specific side, list the ones already
 * made with a share button and a revoke action. Structure mirrors
 * `InviteToTeamDialog` — one dialog, a list, a "new" sub-form below it.
 * Admin-only; the match page only renders the trigger for an admin viewer
 * (see `canManageMatchJoinSettings`).
 */
export function MatchJoinLinksDialog({
  match,
  children,
}: {
  match: Match
  children: React.ReactNode
}) {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [scopeKind, setScopeKind] = useState<ScopeChoice['kind']>('inherit')
  const [sideIds, setSideIds] = useState<string[]>([])

  const linksKey = ['match-join-links', match.id]
  const linksQuery = useQuery({
    queryKey: linksKey,
    enabled: open,
    queryFn: async (): Promise<JoinLink[]> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}/join-links', {
        params: { path: { match_id: match.id } },
      })
      if (error || !data) throw new Error('Failed to load join links')
      return data
    },
  })

  const createMutation = useMutation({
    mutationFn: async () => {
      const scope = scopeInputFor(
        scopeKind === 'sides' ? { kind: 'sides', sideIds } : { kind: scopeKind },
      )
      const { error } = await fetchClient.POST('/matches/{match_id}/join-links', {
        params: { path: { match_id: match.id } },
        body: { scope },
      })
      if (error) throw new Error('Could not create join link')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: linksKey })
      setScopeKind('inherit')
      setSideIds([])
    },
  })

  const revokeMutation = useMutation({
    mutationFn: async (joinLinkId: string) => {
      const { error } = await fetchClient.DELETE(
        '/matches/{match_id}/join-links/{join_link_id}',
        { params: { path: { match_id: match.id, join_link_id: joinLinkId } } },
      )
      if (error) throw new Error('Could not revoke join link')
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: linksKey }),
  })

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      setScopeKind('inherit')
      setSideIds([])
      createMutation.reset()
    }
  }

  const toggleSide = (sideId: string) => {
    setSideIds((prev) =>
      prev.includes(sideId) ? prev.filter((id) => id !== sideId) : [...prev, sideId],
    )
  }

  const canCreate = scopeKind !== 'sides' || sideIds.length > 0
  const links = (linksQuery.data ?? []).filter((l) => !l.revoked_at)

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Join links</DialogTitle>
        </DialogHeader>

        {linksQuery.isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : links.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No active join links yet. Anyone with one can join this game.
          </p>
        ) : (
          <ul className="flex max-h-48 flex-col divide-y overflow-y-auto rounded-lg border">
            {links.map((link) => (
              <li key={link.id} className="flex items-center gap-2 px-3 py-2">
                <span className="min-w-0 flex-1 truncate text-sm">
                  {scopeSummary(link.scope, match)}
                </span>
                <CopyInviteButton token={link.token} kind="join" />
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7 shrink-0 text-destructive"
                  disabled={revokeMutation.isPending}
                  aria-label="Revoke link"
                  title="Revoke link"
                  onClick={() => revokeMutation.mutate(link.id)}
                >
                  <Trash2 className="size-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}

        <div className="flex flex-col gap-3 rounded-lg border p-3">
          <p className="text-sm font-medium">New link</p>

          <div className="flex flex-col gap-1.5">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name="join-link-scope"
                checked={scopeKind === 'inherit'}
                onChange={() => setScopeKind('inherit')}
              />
              Anyone — follows this game's settings
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name="join-link-scope"
                checked={scopeKind === 'unassigned'}
                onChange={() => setScopeKind('unassigned')}
              />
              Unassigned only
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name="join-link-scope"
                checked={scopeKind === 'sides'}
                onChange={() => setScopeKind('sides')}
              />
              A specific side
            </label>
          </div>

          {scopeKind === 'sides' && (
            <div className="ml-6 flex flex-col gap-1">
              {match.sides.map((side, i) => (
                <label key={side.id} className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={sideIds.includes(side.id)}
                    onChange={() => toggleSide(side.id)}
                  />
                  {side.name?.trim() || `Side ${i + 1}`}
                </label>
              ))}
              <p className="text-xs text-muted-foreground">
                Pick more than one to let the link's user choose between them.
              </p>
            </div>
          )}

          {createMutation.isError && (
            <p className="text-sm text-destructive">Could not create link. Try again.</p>
          )}

          <Button
            size="sm"
            className="gap-1.5 self-start"
            disabled={createMutation.isPending || !canCreate}
            onClick={() => createMutation.mutate()}
          >
            <Link2 className="size-3.5" />
            {createMutation.isPending ? 'Creating…' : 'Create link'}
          </Button>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => setOpen(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
