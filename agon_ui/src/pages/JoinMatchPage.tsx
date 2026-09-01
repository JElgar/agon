import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Swords } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { clearPendingInvite } from '@/lib/pendingInvite'

type JoinLinkPreview = components['schemas']['JoinLinkPreview']
type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Which side(s) (if any) a join scope allows picking, and whether landing
 *  unassigned is offered — the client-side mirror of the server's
 *  `JoinScope`/`resolve_join_target` (see `agon_service/src/main.rs`). The
 *  server re-validates regardless; this only decides what the picker shows. */
interface JoinChoice {
  allowedSideIds: string[] | null
  allowUnassigned: boolean
}

function joinChoiceFor(preview: JoinLinkPreview, match: Match): JoinChoice {
  const scope = preview.scope
  return {
    allowedSideIds: scope.side_ids ?? null,
    // The link's own preference, capped by the match's own ceiling — see
    // `Match.allow_unassigned`'s doc comment. The server re-enforces this
    // regardless; this only decides what the picker offers.
    allowUnassigned: scope.allow_unassigned && match.allow_unassigned,
  }
}

function sidesFor(choice: JoinChoice, match: Match): MatchSide[] {
  if (choice.allowedSideIds === null) return match.sides
  const allowed = choice.allowedSideIds
  return match.sides.filter((s) => allowed.includes(s.id))
}

/**
 * The join-link landing screen. Reached once the visitor is signed in with an
 * Agon profile (the join landing routes them through login/signup first, same
 * as `AcceptInvitePage`). Previews the link via the public `by-token`
 * endpoint, loads the match itself for side names, offers a side picker when
 * the resolved scope names more than one option, then joins via
 * `POST /matches/:id/join`.
 */
export function JoinMatchPage() {
  const { token } = useParams<{ token: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [sideId, setSideId] = useState<string | undefined>(undefined)

  // We're now on the join URL itself; the stashed copy (used to survive
  // login) has done its job.
  useEffect(() => {
    clearPendingInvite()
  }, [])

  const preview = useQuery({
    queryKey: ['join-link-by-token', token],
    enabled: !!token,
    retry: false,
    queryFn: async (): Promise<JoinLinkPreview> => {
      const { data, error } = await fetchClient.GET('/join-links/by-token/{token}', {
        params: { path: { token: token! } },
      })
      if (error || !data) throw new Error('join-link-not-found')
      return data
    },
  })

  const matchId = preview.data?.match_id
  const matchQuery = useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  const match = matchQuery.data
  const choice = preview.data && match ? joinChoiceFor(preview.data, match) : undefined
  const pickableSides = choice && match ? sidesFor(choice, match) : []
  // A scope naming exactly one side auto-assigns it (mirroring the server's
  // own auto-assign for that case) rather than showing a one-option picker.
  const forcedSideId =
    choice?.allowedSideIds && choice.allowedSideIds.length === 1
      ? choice.allowedSideIds[0]
      : undefined
  const needsPick = choice
    ? choice.allowedSideIds === null || choice.allowedSideIds.length > 1
    : false
  const effectiveSideId = forcedSideId ?? sideId
  const canSubmit = !choice ? false : needsPick ? !!effectiveSideId : true

  const join = useMutation({
    mutationFn: async (): Promise<'joined' | 'conflict'> => {
      const { error, response } = await fetchClient.POST('/matches/{match_id}/join', {
        params: { path: { match_id: matchId! } },
        body: { token: token!, side_id: effectiveSideId },
      })
      if (response.status === 409) return 'conflict'
      if (error) throw new Error('Failed to join')
      return 'joined'
    },
    onSuccess: (result) => {
      if (result === 'conflict' || !matchId) return
      clearPendingInvite()
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      navigate(`/matches/${matchId}`, { replace: true })
    },
  })

  if (preview.isLoading || (matchId && matchQuery.isLoading)) {
    return <JoinCard>Loading this game…</JoinCard>
  }

  if (preview.isError || !preview.data || matchQuery.isError || !match || !choice) {
    return (
      <JoinCard>
        <h2 className="mb-2 text-xl font-semibold">Link not found</h2>
        <p className="mb-6 text-sm text-muted-foreground">
          This join link is invalid, has been revoked, or the game is gone.
        </p>
        <Button variant="outline" onClick={() => navigate('/feed', { replace: true })}>
          Go to your feed
        </Button>
      </JoinCard>
    )
  }

  return (
    <JoinCard>
      <div className="mb-4 flex size-14 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Swords className="size-7" />
      </div>
      <h2 className="mb-1 text-xl font-semibold">Join this game</h2>
      <p className="mb-6 text-sm text-muted-foreground">
        <strong className="font-medium text-foreground">{preview.data.match_name}</strong>
        {/* `!= null` (not `!== undefined`): the server serializes a Rust
            `Option::None` here as JSON `null`, not an absent key. */}
        {preview.data.max_players != null && (
          <>
            {' '}
            · {preview.data.total_player_count}/{preview.data.max_players} joined
          </>
        )}
      </p>

      {needsPick && (
        <div className="mb-4 w-full">
          <label htmlFor="join-side" className="mb-1 block text-xs text-muted-foreground">
            Which side?
          </label>
          <select
            id="join-side"
            value={sideId ?? ''}
            onChange={(e) => setSideId(e.target.value || undefined)}
            className="h-9 w-full rounded-md border bg-background px-3 text-sm"
          >
            {/* No side picked yet: an unassigned scope defaults to that
                explicitly; otherwise a disabled placeholder — never silently
                falling back to whichever side happens to render first. */}
            {choice.allowUnassigned ? (
              <option value="">Unassigned — pick a side later</option>
            ) : (
              <option value="" disabled>
                Choose a side…
              </option>
            )}
            {pickableSides.map((side, i) => (
              <option key={side.id} value={side.id}>
                {side.name?.trim() || `Side ${i + 1}`}
              </option>
            ))}
          </select>
        </div>
      )}

      {join.isError && (
        <p className="mb-3 text-sm text-destructive">Something went wrong. Try again.</p>
      )}
      {join.data === 'conflict' && (
        <p className="mb-3 text-sm text-destructive">
          This game (or side) is full, or you're already on the roster.
        </p>
      )}

      <Button
        className="w-full"
        disabled={join.isPending || !canSubmit}
        onClick={() => join.mutate()}
      >
        {join.isPending ? 'Joining…' : 'Join'}
      </Button>
    </JoinCard>
  )
}

/** Centered card chrome, mirroring `AcceptInvitePage`'s `InviteCard`. */
function JoinCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center rounded-2xl border bg-card p-8 text-center">
      {children}
    </div>
  )
}
