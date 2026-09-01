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

/** Which sides a new link should allow — the form's own shape.
 *  `'any'` -> `side_ids: undefined` (any side of the match is fine);
 *  `'specific'` -> `side_ids: [...]` (one or more, picked below);
 *  `'none'` -> `side_ids: []` (force-unassigned — see `JoinLinkScope`'s
 *  doc comment). */
type SidesChoice = 'any' | 'specific' | 'none'

interface ScopeForm {
  sidesChoice: SidesChoice
  sideIds: string[]
  allowUnassigned: boolean
}

function scopeInputFor(form: ScopeForm): JoinLinkScope {
  return {
    side_ids: form.sidesChoice === 'any' ? undefined : form.sideIds,
    // `'none'` requires unassigned to actually be reachable — force it,
    // regardless of the (hidden, in that mode) checkbox's last value.
    allow_unassigned: form.sidesChoice === 'none' ? true : form.allowUnassigned,
  }
}

/** Human summary of an existing link's scope, for the list. */
function scopeSummary(scope: JoinLinkScope, match: Match): string {
  const ids = scope.side_ids
  if (ids === undefined) {
    return scope.allow_unassigned ? 'Any side, or unassigned' : 'Any side — must pick one'
  }
  if (ids.length === 0) return 'Unassigned only'
  const names = ids.map((id, i) => {
    const side = match.sides.find((s) => s.id === id)
    return side?.name?.trim() || `Side ${i + 1}`
  })
  const sideLabel = names.length === 1 ? `Side: ${names[0]}` : `Either: ${names.join(', ')}`
  return scope.allow_unassigned ? `${sideLabel}, or unassigned` : sideLabel
}

const emptyForm: ScopeForm = { sidesChoice: 'any', sideIds: [], allowUnassigned: true }

/**
 * Manage a match's shareable join links: mint a many-use link allowing any
 * side (optionally also unassigned), one or more specific sides, or
 * unassigned only, list the ones already made with a share button and a
 * revoke action. Structure mirrors `InviteToTeamDialog` — one dialog, a
 * list, a "new" sub-form below it. Admin-only; the match page only renders
 * the trigger for an admin viewer (see `canManageMatchJoinSettings`).
 *
 * A link's own "allow unassigned" is always capped by the match's own
 * `allow_unassigned` (see `Match.allow_unassigned`'s doc comment) — when the
 * match doesn't allow it at all, that half of the form is hidden rather than
 * offering a setting that would silently do nothing.
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
  const [form, setForm] = useState<ScopeForm>(emptyForm)

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
      const { error } = await fetchClient.POST('/matches/{match_id}/join-links', {
        params: { path: { match_id: match.id } },
        body: { scope: scopeInputFor(form) },
      })
      if (error) throw new Error('Could not create join link')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: linksKey })
      setForm(emptyForm)
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
      setForm(emptyForm)
      createMutation.reset()
    }
  }

  const toggleSide = (sideId: string) => {
    setForm((prev) => ({
      ...prev,
      sideIds: prev.sideIds.includes(sideId)
        ? prev.sideIds.filter((id) => id !== sideId)
        : [...prev.sideIds, sideId],
    }))
  }

  const canCreate = form.sidesChoice !== 'specific' || form.sideIds.length > 0
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
                name="join-link-sides"
                checked={form.sidesChoice === 'any'}
                onChange={() => setForm((prev) => ({ ...prev, sidesChoice: 'any' }))}
              />
              Any side
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name="join-link-sides"
                checked={form.sidesChoice === 'specific'}
                onChange={() => setForm((prev) => ({ ...prev, sidesChoice: 'specific' }))}
              />
              Specific side(s)
            </label>
            {match.allow_unassigned && (
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name="join-link-sides"
                  checked={form.sidesChoice === 'none'}
                  onChange={() => setForm((prev) => ({ ...prev, sidesChoice: 'none' }))}
                />
                None — always unassigned
              </label>
            )}
          </div>

          {form.sidesChoice === 'specific' && (
            <div className="ml-6 flex flex-col gap-1">
              {match.sides.map((side, i) => (
                <label key={side.id} className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={form.sideIds.includes(side.id)}
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

          {/* Hidden entirely when the match itself disallows unassigned —
              offering this checkbox would just silently do nothing (see
              `Match.allow_unassigned`'s doc comment). Also hidden for
              `'none'`, where it's implied. */}
          {match.allow_unassigned && form.sidesChoice !== 'none' && (
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={form.allowUnassigned}
                onChange={(e) =>
                  setForm((prev) => ({ ...prev, allowUnassigned: e.target.checked }))
                }
              />
              Also allow joining unassigned
            </label>
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
