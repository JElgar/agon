import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Pencil } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type OpenJoinScopeKind = components['schemas']['OpenJoinScopeKind']

/** A side's draft open-join settings, meaning depends on `mode`:
 * under `match_wide`, `open` is `SideOpenJoinInput.enabled`; under
 * `per_side`, `open` gates whether this side has its own policy at all. */
interface SideDraft {
  open: boolean
  minMatchesPlayed: string // '' = no minimum
  requiresApproval: boolean
  maxOpenSpots: string // '' = uncapped
}
type SideDrafts = Record<string, SideDraft>

function sideDraftsFromMatch(match: Match, mode: OpenJoinScopeKind): SideDrafts {
  return Object.fromEntries(
    match.sides.map((side) => [
      side.id,
      mode === 'match_wide'
        ? {
            open: side.open_join_enabled,
            minMatchesPlayed: '',
            requiresApproval: false,
            maxOpenSpots: side.max_open_spots?.toString() ?? '',
          }
        : {
            open: side.open_join != null,
            minMatchesPlayed: side.open_join?.min_matches_played?.toString() ?? '',
            requiresApproval: side.open_join?.requires_approval ?? false,
            maxOpenSpots: side.max_open_spots?.toString() ?? '',
          },
    ]),
  )
}

function sideSummary(side: MatchSide, i: number, mode: OpenJoinScopeKind): string | null {
  const open = mode === 'match_wide' ? side.open_join_enabled : side.open_join != null
  if (!open) return null
  const name = sideDisplayName(side) ?? `Side ${i + 1}`
  const cap = side.max_open_spots != null ? ` (max ${side.max_open_spots})` : ''
  return `${name}${cap}`
}

/**
 * Editable "ringers" card — whether a match is open to individuals who
 * aren't invited, a link, or a team member (`Match.open_join`), either as
 * one match-wide policy or a policy per side. Team-audience open-join
 * (`OpenJoinAudience.teams`) isn't wired to any join path yet, so this
 * editor only ever writes `audience: individuals`. Read-only for everyone;
 * an admin (`canManage`) gets an "Edit" affordance, same posture as
 * `MatchJoinSettingsEditor`: whole-value replace on save, not a diff.
 */
export function MatchOpenJoinEditor({ match, canManage }: { match: Match; canManage: boolean }) {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [openEnabled, setOpenEnabled] = useState(match.open_join != null)
  const [mode, setMode] = useState<OpenJoinScopeKind>(match.open_join?.mode ?? 'match_wide')
  const [minMatchesPlayed, setMinMatchesPlayed] = useState(
    match.open_join?.policy?.min_matches_played?.toString() ?? '',
  )
  const [requiresApproval, setRequiresApproval] = useState(
    match.open_join?.policy?.requires_approval ?? false,
  )
  const [maxOpenSpots, setMaxOpenSpots] = useState(
    match.open_join?.policy?.max_open_spots?.toString() ?? '',
  )
  const [sides, setSides] = useState<SideDrafts>(() =>
    sideDraftsFromMatch(match, match.open_join?.mode ?? 'match_wide'),
  )

  const resetDrafts = () => {
    const m = match.open_join?.mode ?? 'match_wide'
    setOpenEnabled(match.open_join != null)
    setMode(m)
    setMinMatchesPlayed(match.open_join?.policy?.min_matches_played?.toString() ?? '')
    setRequiresApproval(match.open_join?.policy?.requires_approval ?? false)
    setMaxOpenSpots(match.open_join?.policy?.max_open_spots?.toString() ?? '')
    setSides(sideDraftsFromMatch(match, m))
  }

  const save = useMutation({
    mutationFn: async () => {
      if (!openEnabled) {
        const { error } = await fetchClient.DELETE('/matches/{match_id}/open-join', {
          params: { path: { match_id: match.id } },
        })
        if (error) throw new Error('Failed to close open joining')
        return
      }

      const { error: scopeError } = await fetchClient.PUT('/matches/{match_id}/open-join', {
        params: { path: { match_id: match.id } },
        body: {
          mode,
          policy:
            mode === 'match_wide'
              ? {
                  audience: 'individuals',
                  min_matches_played: minMatchesPlayed.trim()
                    ? Number(minMatchesPlayed)
                    : undefined,
                  requires_approval: requiresApproval,
                  max_open_spots: maxOpenSpots.trim() ? Number(maxOpenSpots) : undefined,
                }
              : undefined,
        },
      })
      if (scopeError) throw new Error('Failed to save open-join policy')

      for (const side of match.sides) {
        const draft = sides[side.id]
        const sideMaxOpenSpots = draft?.maxOpenSpots.trim() ? Number(draft.maxOpenSpots) : undefined
        const { error: sideError } = await fetchClient.PUT(
          '/matches/{match_id}/sides/{side_id}/open-join',
          {
            params: { path: { match_id: match.id, side_id: side.id } },
            body:
              mode === 'match_wide'
                ? {
                    enabled: draft?.open ?? false,
                    side_policy: undefined,
                    max_open_spots: sideMaxOpenSpots,
                  }
                : {
                    enabled: false,
                    side_policy: draft?.open
                      ? {
                          audience: 'individuals',
                          min_matches_played: draft.minMatchesPlayed.trim()
                            ? Number(draft.minMatchesPlayed)
                            : undefined,
                          requires_approval: draft.requiresApproval,
                        }
                      : undefined,
                    max_open_spots: sideMaxOpenSpots,
                  },
          },
        )
        if (sideError)
          throw new Error(`Failed to save ${sideDisplayName(side) ?? 'a side'}'s ringer settings`)
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match', match.id] })
      setEditing(false)
    },
  })

  if (!editing) {
    const openSideSummaries = match.sides
      .map((s, i) => sideSummary(s, i, match.open_join?.mode ?? 'match_wide'))
      .filter((s): s is string => s != null)
    return (
      <div className="flex items-center justify-between rounded-xl border bg-card p-4">
        <div>
          <p className="text-sm font-medium">Ringers</p>
          {match.open_join == null ? (
            <p className="text-xs text-muted-foreground">
              Closed — people join by invite, link, or team
            </p>
          ) : (
            <>
              <p className="text-xs text-muted-foreground">
                {match.open_join.mode === 'match_wide'
                  ? 'Open — one policy for the whole match'
                  : 'Open — each side sets its own policy'}
                {match.open_join.policy?.requires_approval ? ' · requests need approval' : ''}
                {match.open_join.policy?.min_matches_played != null
                  ? ` · min ${match.open_join.policy.min_matches_played} matches played`
                  : ''}
              </p>
              <p className="text-xs text-muted-foreground">
                {openSideSummaries.length > 0
                  ? `Accepting ringers: ${openSideSummaries.join(' · ')}`
                  : 'No side accepts ringers yet'}
              </p>
            </>
          )}
        </div>
        {canManage && (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 gap-1 px-2 text-xs text-muted-foreground"
            onClick={() => {
              resetDrafts()
              setEditing(true)
            }}
          >
            <Pencil className="size-3" /> Edit
          </Button>
        )}
      </div>
    )
  }

  return (
    <div className="rounded-xl border bg-card p-4">
      <p className="mb-3 text-sm font-medium">Ringers</p>

      <label className="mb-3 flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          className="mt-0.5"
          checked={openEnabled}
          onChange={(e) => setOpenEnabled(e.target.checked)}
        />
        <span>
          Open this match to ringers
          <span className="block text-xs text-muted-foreground">
            Lets people who aren't invited, on a join link, or on a linked team join directly.
          </span>
        </span>
      </label>

      {openEnabled && (
        <>
          <div className="mb-3 flex gap-3 text-sm">
            <label className="flex items-center gap-1.5">
              <input
                type="radio"
                checked={mode === 'match_wide'}
                onChange={() => setMode('match_wide')}
              />
              One policy for the match
            </label>
            <label className="flex items-center gap-1.5">
              <input
                type="radio"
                checked={mode === 'per_side'}
                onChange={() => setMode('per_side')}
              />
              Each side sets its own
            </label>
          </div>

          {mode === 'match_wide' && (
            <div className="mb-3 flex flex-col gap-2 rounded-lg border p-2">
              <div className="flex items-center gap-2">
                <Label htmlFor="open-join-min-matches" className="flex-1 text-xs">
                  Minimum matches played
                </Label>
                <input
                  id="open-join-min-matches"
                  type="number"
                  min={0}
                  placeholder="No minimum"
                  value={minMatchesPlayed}
                  onChange={(e) => setMinMatchesPlayed(e.target.value)}
                  className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
                />
              </div>
              <div className="flex items-center gap-2">
                <Label htmlFor="open-join-max-spots" className="flex-1 text-xs">
                  Max ringers for the whole match
                </Label>
                <input
                  id="open-join-max-spots"
                  type="number"
                  min={1}
                  placeholder="No cap"
                  value={maxOpenSpots}
                  onChange={(e) => setMaxOpenSpots(e.target.value)}
                  className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
                />
              </div>
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                <input
                  type="checkbox"
                  checked={requiresApproval}
                  onChange={(e) => setRequiresApproval(e.target.checked)}
                />
                Ask-to-join: an admin approves each request
              </label>
            </div>
          )}

          <div className="flex flex-col gap-3">
            {match.sides.map((side, i) => {
              const draft = sides[side.id]
              return (
                <div key={side.id} className="flex flex-col gap-1.5 rounded-lg border p-2">
                  <label className="flex items-center gap-2 text-xs">
                    <input
                      type="checkbox"
                      checked={draft?.open ?? false}
                      onChange={(e) =>
                        setSides((prev) => ({
                          ...prev,
                          [side.id]: { ...prev[side.id], open: e.target.checked },
                        }))
                      }
                    />
                    {sideDisplayName(side) ?? `Side ${i + 1}`} accepts ringers
                  </label>
                  {draft?.open && (
                    <div className="flex flex-col gap-2 pl-5">
                      {mode === 'per_side' && (
                        <>
                          <div className="flex items-center gap-2">
                            <Label htmlFor={`side-min-matches-${side.id}`} className="flex-1 text-xs">
                              Minimum matches played
                            </Label>
                            <input
                              id={`side-min-matches-${side.id}`}
                              type="number"
                              min={0}
                              placeholder="No minimum"
                              value={draft.minMatchesPlayed}
                              onChange={(e) =>
                                setSides((prev) => ({
                                  ...prev,
                                  [side.id]: { ...prev[side.id], minMatchesPlayed: e.target.value },
                                }))
                              }
                              className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
                            />
                          </div>
                          <label className="flex items-center gap-2 text-xs text-muted-foreground">
                            <input
                              type="checkbox"
                              checked={draft.requiresApproval}
                              onChange={(e) =>
                                setSides((prev) => ({
                                  ...prev,
                                  [side.id]: { ...prev[side.id], requiresApproval: e.target.checked },
                                }))
                              }
                            />
                            Ask-to-join: an admin approves each request
                          </label>
                        </>
                      )}
                      <div className="flex items-center gap-2">
                        <Label htmlFor={`side-max-spots-${side.id}`} className="flex-1 text-xs">
                          Max ringers on this side
                        </Label>
                        <input
                          id={`side-max-spots-${side.id}`}
                          type="number"
                          min={1}
                          placeholder="No cap"
                          value={draft.maxOpenSpots}
                          onChange={(e) =>
                            setSides((prev) => ({
                              ...prev,
                              [side.id]: { ...prev[side.id], maxOpenSpots: e.target.value },
                            }))
                          }
                          className="h-8 w-24 rounded-md border bg-background px-2 text-sm"
                        />
                      </div>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        </>
      )}

      {save.isError && (
        <p className="mt-2 text-xs text-destructive">{(save.error as Error).message}</p>
      )}
      <div className="mt-3 flex gap-2">
        <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? 'Saving…' : 'Save'}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={save.isPending}
          onClick={() => setEditing(false)}
        >
          Cancel
        </Button>
      </div>
    </div>
  )
}
