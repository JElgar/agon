import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Lock } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { isSetsSport, type MatchType } from '@/lib/sports'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SportPicker } from '@/components/agon/SportPicker'
import { MatchFormatEditor } from '@/components/agon/MatchFormatEditor'
import type { MatchFormat } from '@/lib/matchFormat'
import { MultiImageUploadField } from '@/components/agon/MultiImageUploadField'
import {
  PlayerSideEditor,
  type TaggedPlayer,
} from '@/components/agon/PlayerSideEditor'
import { FootballScoreFields } from '@/components/agon/FootballScoreFields'
import { CricketScoreFields } from '@/components/agon/CricketScoreFields'
import { cn } from '@/lib/utils'
import { toDateTimeLocal } from '@/lib/datetime'
import { addPendingMatch } from '@/hooks/usePendingMatches'

type CreateMatchInput = components['schemas']['CreateMatchInput']
type UserProfile = components['schemas']['UserProfile']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']

/** A tagged side's players, reshaped into the API's `MatchPlayer` so the
 *  football/cricket detail editors (`FootballScoreFields`/`CricketScoreFields`,
 *  built for a real match's roster) can be reused here too — before the match
 *  exists, `member.id` holds the *reference* the server will resolve to a
 *  real player id: a tagged Agon user's own (already-known) id, or a guest's
 *  freshly generated `TaggedPlayer.id` (see `CreateMatchExternalInviteInput`). */
function toMatchPlayers(players: TaggedPlayer[], sideId: string): MatchPlayer[] {
  return players.map((p) =>
    p.kind === 'user'
      ? {
          member: {
            type: 'User',
            id: p.id,
            user_id: p.id,
            name: p.name,
            avatar_url: p.imageUrl,
          },
          side_id: sideId,
        }
      : {
          member: { type: 'External', id: p.id, display_name: p.name },
          side_id: sideId,
        },
  )
}

/** Client ids used to wire invites/score to the created sides (see CreateMatchSideInput). */
const SIDE_A = 'side-a'
const SIDE_B = 'side-b'

/** One row of the sets editor: games won by each side in a single set. */
interface SetRow {
  a: string
  b: string
}

/** Whether the match is upcoming (no score) or already played (with a score). */
type MatchMode = 'scheduled' | 'completed'

/** A display name for a side: the sole player's name, else the custom name
 *  typed for it (if any), else a generic fallback — mirrors the server's
 *  resolution order for this compose form's own preview text. */
function sideName(players: TaggedPlayer[], customName: string, fallback: string): string {
  if (players.length === 1) return players[0].name
  return customName.trim() || fallback
}

/** Default scheduled time: the next whole hour, at least an hour from now. */
function defaultScheduledAt(): string {
  const d = new Date()
  d.setHours(d.getHours() + 1, 0, 0, 0)
  return toDateTimeLocal(d)
}

/** Default completed time: now (rounded to the minute). */
function defaultCompletedAt(): string {
  return toDateTimeLocal(new Date())
}

/**
 * The "Log a match" flow: pick a sport, tag players onto your side and the
 * opposition (real Agon users via `/users/search`, or guests by name), then
 * optionally record the result and post. Posts `CreateMatchInput` to
 * `POST /matches`; on success invalidates the feed and navigates to it.
 */
export function LogMatchPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const [sport, setSport] = useState<MatchType | null>(null)
  const [format, setFormat] = useState<MatchFormat | null>(null)
  const [name, setName] = useState('')
  const [sideA, setSideA] = useState<TaggedPlayer[]>([])
  const [sideB, setSideB] = useState<TaggedPlayer[]>([])
  // Optional custom names for ad-hoc sides. No team-picker exists yet, so
  // these are always offered — once one does, hide the field for a side with
  // a team assigned (the server rejects a name alongside a team unless
  // another side shares it).
  const [sideAName, setSideAName] = useState('')
  const [sideBName, setSideBName] = useState('')

  // Scheduled (upcoming, no score) vs Completed (already played, with a score).
  // The mode drives both whether the score section shows and how `starts_at` is
  // validated (future for scheduled, past for completed) — mirroring the server.
  const [mode, setMode] = useState<MatchMode>('scheduled')
  // Local wall-clock "YYYY-MM-DDTHH:mm" for the datetime-local control. Seeded to
  // a sensible default per mode and re-seeded when the mode flips.
  const [startsAt, setStartsAt] = useState<string>(defaultScheduledAt)
  const recordResult = mode === 'completed'
  const [sets, setSets] = useState<SetRow[]>([
    { a: '', b: '' },
    { a: '', b: '' },
  ])
  const [pointsA, setPointsA] = useState('')
  const [pointsB, setPointsB] = useState('')
  // Football/cricket's own goal-detail / innings editors report their built
  // result here directly (see `FootballScoreFields`/`CricketScoreFields`),
  // rather than through the plain points/sets state above.
  const [detailBuilt, setDetailBuilt] = useState<{ score: Score; winnerSideId?: string } | null>(null)
  // Optional header images for the match, uploaded via the Asset API. Holds the
  // ordered uploaded asset ids (attached as `header_photo_asset_ids` on submit).
  const [headerAssetIds, setHeaderAssetIds] = useState<string[]>([])

  // The signed-in user's profile. Used to seed them onto their own side by
  // default (as a real, removable player) and to badge/exclude them in search.
  const me = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<UserProfile | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile ?? null
    },
  })
  const currentUserId = me.data?.id

  // Seed the current user onto side A once, when their profile first loads.
  // Tracked so removing yourself sticks (we don't re-add on later renders).
  const [seededSelf, setSeededSelf] = useState(false)
  useEffect(() => {
    if (seededSelf || !me.data) return
    const self = me.data
    setSideA((prev) =>
      prev.some((p) => p.kind === 'user' && p.id === self.id)
        ? prev
        : [
            {
              kind: 'user',
              id: self.id,
              name: self.name,
              imageUrl: self.profile_image?.image_url,
            },
            ...prev,
          ],
    )
    setSeededSelf(true)
  }, [me.data, seededSelf])

  // Switch mode and re-seed the time to a default appropriate for it (future for
  // scheduled, now for completed) so the picker never starts on an invalid time.
  const changeMode = (next: MatchMode) => {
    setMode(next)
    setStartsAt(next === 'scheduled' ? defaultScheduledAt() : defaultCompletedAt())
  }

  // Ids already tagged, so a person can't be added to both sides.
  const sideAUserIds = sideA.flatMap((p) => (p.kind === 'user' ? [p.id] : []))
  const sideBUserIds = sideB.flatMap((p) => (p.kind === 'user' ? [p.id] : []))

  const setsPlayable = isSetsSport(sport ?? 'other')
  const isFootball = sport === 'football'
  const isCricket = sport === 'cricket'

  // Is the picked time valid for the chosen mode? Completed matches must be in
  // the past, scheduled ones in the future (matches the server's rule). Empty or
  // unparseable input is invalid. `Date.now()` is read at render, which is fine —
  // the server re-validates on submit, so a moment's clock drift can't slip through.
  const timeError = useMemo((): string | null => {
    if (!startsAt) return 'Pick a date and time'
    const ts = new Date(startsAt).getTime()
    if (Number.isNaN(ts)) return 'Pick a valid date and time'
    if (mode === 'completed' && ts > Date.now())
      return 'A completed match must be in the past'
    if (mode === 'scheduled' && ts <= Date.now())
      return 'A scheduled match must be in the future'
    return null
  }, [startsAt, mode])

  // A completed match must carry a result; a scheduled one must not. Returns a
  // message when the score is required-but-missing (drives the submit gate and an
  // inline hint), or null when the score state is acceptable for the mode.
  const scoreError = useMemo((): string | null => {
    if (mode !== 'completed') return null
    if (isFootball || isCricket) {
      return detailBuilt ? null : 'Enter the score'
    }
    if (setsPlayable) {
      const anySet = sets.some((r) => {
        const a = Number(r.a)
        const b = Number(r.b)
        return (
          (r.a !== '' || r.b !== '') &&
          Number.isFinite(a) &&
          Number.isFinite(b) &&
          a >= 0 &&
          b >= 0 &&
          (a > 0 || b > 0)
        )
      })
      return anySet ? null : 'Enter the score for at least one set'
    }
    const a = Number(pointsA)
    const b = Number(pointsB)
    if (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))
      return 'Enter the score for both sides'
    return null
  }, [mode, isFootball, isCricket, detailBuilt, setsPlayable, sets, pointsA, pointsB])

  // Validation: a sport, a match name, at least one player on each side (so the
  // match is meaningful), at least one opponent on side B, a time valid for the
  // mode, and — for a completed match — a result.
  const valid = useMemo(() => {
    if (!sport) return false
    if (name.trim().length === 0) return false
    if (sideA.length === 0) return false
    if (sideB.length === 0) return false
    if (timeError) return false
    if (scoreError) return false
    return true
  }, [sport, name, sideA.length, sideB.length, timeError, scoreError])

  const mutation = useMutation({
    mutationFn: async (body: CreateMatchInput) => {
      const { data, error } = await fetchClient.POST('/matches', { body })
      if (error || !data)
        throw new Error(
          typeof error === 'string' ? error : 'Failed to post the match',
        )
      return data
    },
    onSuccess: async (created) => {
      // The feed is eventually consistent — the worker fans this match out to
      // `GET /feed` a beat later. Stash it in the pending-match overlay so it
      // shows in the feed instantly, and kick off a refetch that will reconcile
      // (and prune the overlay) once the server catches up.
      addPendingMatch(queryClient, created)
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      navigate('/feed')
    },
  })

  const buildInvites = (): CreateMatchInput['invites'] => {
    const invites: CreateMatchInput['invites'] = []
    for (const [clientId, players] of [
      [SIDE_A, sideA],
      [SIDE_B, sideB],
    ] as const) {
      // The current user is sent via `creator_side_client_id` (an accepted
      // player), NOT as an invite — filter them out of the invite list here.
      const invited_user_ids = players
        .filter((p) => p.kind === 'user' && p.id !== currentUserId)
        .map((p) => (p as Extract<TaggedPlayer, { kind: 'user' }>).id)
      const invited_externals = players
        .filter((p): p is Extract<TaggedPlayer, { kind: 'external' }> => p.kind === 'external')
        .map((p) => ({ client_id: p.id, name: p.name }))
      if (invited_user_ids.length === 0 && invited_externals.length === 0) continue
      invites.push({
        side_client_id: clientId,
        invited_user_ids,
        invited_externals,
      })
    }
    return invites
  }

  /** Which side (if any) the current user is on → the creator_side_client_id. */
  const creatorSideClientId = (): string | undefined => {
    if (!currentUserId) return undefined
    if (sideA.some((p) => p.kind === 'user' && p.id === currentUserId))
      return SIDE_A
    if (sideB.some((p) => p.kind === 'user' && p.id === currentUserId))
      return SIDE_B
    return undefined
  }

  /** Build the score payload (with the `type` discriminator the server requires,
   *  which the generated `Omit<Score,"type">` drops) plus the derived winner. */
  const buildScore = ():
    | { score: CreateMatchInput['score']; winner?: string }
    | null => {
    if (!recordResult || !sport) return null

    if (isFootball || isCricket) {
      if (!detailBuilt) return null
      return {
        score: detailBuilt.score as unknown as CreateMatchInput['score'],
        winner: detailBuilt.winnerSideId,
      }
    }

    if (setsPlayable) {
      const rows = sets
        .map((r) => ({ a: Number(r.a), b: Number(r.b) }))
        .filter(
          (r) =>
            r.a >= 0 &&
            r.b >= 0 &&
            (r.a > 0 || r.b > 0) &&
            Number.isFinite(r.a) &&
            Number.isFinite(r.b),
        )
      if (rows.length === 0) return null
      let aSets = 0
      let bSets = 0
      for (const r of rows) {
        if (r.a > r.b) aSets += 1
        else if (r.b > r.a) bSets += 1
      }
      const score = {
        type: 'Sets',
        entries: {
          [SIDE_A]: rows.map((r) => r.a),
          [SIDE_B]: rows.map((r) => r.b),
        },
      } as unknown as CreateMatchInput['score']
      const winner = aSets === bSets ? undefined : aSets > bSets ? SIDE_A : SIDE_B
      return { score, winner }
    }

    const a = Number(pointsA)
    const b = Number(pointsB)
    if (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))
      return null
    const score = {
      type: 'Simple',
      entries: { [SIDE_A]: a, [SIDE_B]: b },
    } as unknown as CreateMatchInput['score']
    const winner = a === b ? undefined : a > b ? SIDE_A : SIDE_B
    return { score, winner }
  }

  const handleSubmit = () => {
    if (!sport || !valid) return

    const body: CreateMatchInput = {
      name: name.trim(),
      description: '',
      match_type: sport,
      // datetime-local is local wall-clock; convert to a UTC ISO instant.
      starts_at: new Date(startsAt).toISOString(),
      // A custom name is only sent if the user actually typed one — the
      // server otherwise resolves a display name per request (sole player's
      // name, "Your side"/"Opposition", ...), since a name fixed at creation
      // time would be wrong once the roster changes or anyone but the
      // creator views the match.
      sides: [
        { client_id: SIDE_A, name: sideAName.trim() || undefined },
        { client_id: SIDE_B, name: sideBName.trim() || undefined },
      ],
      invites: buildInvites(),
    }

    const creatorSide = creatorSideClientId()
    if (creatorSide) body.creator_side_client_id = creatorSide

    if (headerAssetIds.length > 0) body.header_photo_asset_ids = headerAssetIds
    if (format) body.format = format

    const scored = buildScore()
    if (scored) {
      body.score = scored.score
      if (scored.winner) body.winner_side_id = scored.winner
    }

    mutation.mutate(body)
  }

  const playersSet = sport !== null && sideA.length > 0 && sideB.length > 0

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-3">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Log a match</h1>
        <Button variant="ghost" size="sm" onClick={() => navigate('/feed')}>
          Cancel
        </Button>
      </div>

      {/* 1 · Sport */}
      <Section num={1} title="Sport" done={sport !== null}>
        <SportPicker
          value={sport}
          onChange={(s) => {
            setSport(s)
            setFormat(null)
          }}
        />
      </Section>

      {/* Format (optional, football/cricket only) */}
      {sport !== null && (sport === 'football' || sport === 'cricket') && (
        <Section title="Match format">
          <MatchFormatEditor sport={sport} value={format} onChange={setFormat} />
        </Section>
      )}

      {/* Match name */}
      <Section num={2} title="Match name" done={name.trim().length > 0}>
        <Label htmlFor="match-name" className="sr-only">
          Match name
        </Label>
        <Input
          id="match-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. Tuesday night singles"
        />
      </Section>

      {/* Header images (optional) */}
      <Section title="Header images" done={headerAssetIds.length > 0}>
        <MultiImageUploadField
          purpose="match_header"
          label="Add header images"
          onChange={setHeaderAssetIds}
        />
      </Section>

      {/* 3 · Players */}
      <Section num={3} title="Players" done={playersSet}>
        <div className="flex flex-col gap-2.5">
          <PlayerSideEditor
            title="Your side"
            searchPlaceholder="Add a teammate…"
            players={sideA}
            onChange={setSideA}
            currentUserId={currentUserId}
            excludeUserIds={sideBUserIds}
            name={sideAName}
            onNameChange={setSideAName}
          />
          <div className="flex items-center justify-center">
            <span className="rounded-full border border-primary/30 bg-accent px-3 py-0.5 text-[11px] font-medium text-primary">
              vs
            </span>
          </div>
          <PlayerSideEditor
            title="Opposition"
            searchPlaceholder="Add an opponent…"
            players={sideB}
            onChange={setSideB}
            currentUserId={currentUserId}
            excludeUserIds={sideAUserIds}
            name={sideBName}
            onNameChange={setSideBName}
          />
        </div>
      </Section>

      {/* 4 · When — scheduled vs completed + the match time */}
      <Section num={4} title="When" done={!timeError}>
        <div
          role="tablist"
          aria-label="Match status"
          className="mb-3 grid grid-cols-2 gap-1 rounded-lg bg-muted p-1"
        >
          {(['scheduled', 'completed'] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="tab"
              aria-selected={mode === m}
              onClick={() => changeMode(m)}
              className={cn(
                'rounded-md px-3 py-1.5 text-sm font-medium capitalize transition-colors',
                mode === m
                  ? 'bg-card text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground',
              )}
            >
              {m}
            </button>
          ))}
        </div>
        <Label htmlFor="starts-at" className="text-xs text-muted-foreground">
          {mode === 'scheduled' ? 'Kick-off time' : 'When it was played'}
        </Label>
        <Input
          id="starts-at"
          type="datetime-local"
          value={startsAt}
          onChange={(e) => setStartsAt(e.target.value)}
          className="mt-1"
        />
        {timeError && (
          <p className="mt-1.5 text-xs text-destructive">{timeError}</p>
        )}
      </Section>

      {/* 5 · Score — only for a completed match, and only once players are set */}
      {mode === 'completed' &&
        (playersSet ? (
          <Section num={5} title="Score">
          {(isFootball || isCricket) && (() => {
            const sideAObj: MatchSide = { id: SIDE_A, name: sideAName.trim() || undefined }
            const sideBObj: MatchSide = { id: SIDE_B, name: sideBName.trim() || undefined }
            const players = [...toMatchPlayers(sideA, SIDE_A), ...toMatchPlayers(sideB, SIDE_B)]
            return isFootball ? (
              <FootballScoreFields
                sideA={sideAObj}
                sideB={sideBObj}
                players={players}
                onChange={setDetailBuilt}
              />
            ) : (
              <CricketScoreFields
                sideA={sideAObj}
                sideB={sideBObj}
                players={players}
                onChange={setDetailBuilt}
              />
            )
          })()}

          {!isFootball && !isCricket && setsPlayable && (
            <div className="flex flex-col gap-2">
              <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2 text-center text-[11px] uppercase tracking-wider text-muted-foreground">
                <span className="truncate text-left">{sideName(sideA, sideAName, 'Your side')}</span>
                <span>Set</span>
                <span className="truncate text-right">
                  {sideName(sideB, sideBName, 'Opposition')}
                </span>
              </div>
              {sets.map((row, i) => (
                <div
                  key={i}
                  className="grid grid-cols-[1fr_auto_1fr] items-center gap-2"
                >
                  <Input
                    type="number"
                    min={0}
                    inputMode="numeric"
                    value={row.a}
                    onChange={(e) =>
                      setSets((s) =>
                        s.map((r, j) => (j === i ? { ...r, a: e.target.value } : r)),
                      )
                    }
                    placeholder="0"
                  />
                  <span className="text-xs text-muted-foreground">Set {i + 1}</span>
                  <Input
                    type="number"
                    min={0}
                    inputMode="numeric"
                    value={row.b}
                    onChange={(e) =>
                      setSets((s) =>
                        s.map((r, j) => (j === i ? { ...r, b: e.target.value } : r)),
                      )
                    }
                    placeholder="0"
                  />
                </div>
              ))}
              <div className="flex justify-between">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setSets((s) => [...s, { a: '', b: '' }])}
                >
                  Add set
                </Button>
                {sets.length > 1 && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => setSets((s) => s.slice(0, -1))}
                  >
                    Remove set
                  </Button>
                )}
              </div>
            </div>
          )}

          {!isFootball && !isCricket && !setsPlayable && (
            <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
              <div className="flex flex-col gap-1">
                <span className="truncate text-center text-xs text-muted-foreground">
                  {sideName(sideA, sideAName, 'Your side')}
                </span>
                <Input
                  type="number"
                  min={0}
                  inputMode="numeric"
                  value={pointsA}
                  onChange={(e) => setPointsA(e.target.value)}
                  placeholder="0"
                />
              </div>
              <span className="pt-5 text-muted-foreground">–</span>
              <div className="flex flex-col gap-1">
                <span className="truncate text-center text-xs text-muted-foreground">
                  {sideName(sideB, sideBName, 'Opposition')}
                </span>
                <Input
                  type="number"
                  min={0}
                  inputMode="numeric"
                  value={pointsB}
                  onChange={(e) => setPointsB(e.target.value)}
                  placeholder="0"
                />
              </div>
            </div>
          )}

          {scoreError && (
            <p className="mt-2 text-xs text-destructive">{scoreError}</p>
          )}
          </Section>
        ) : (
          <LockedRow
            label="Score"
            hint={
              sport === null
                ? 'Pick a sport to enter the score'
                : 'Add players to both sides to enter the score'
            }
          />
        ))}

      {mutation.isError && (
        <p className="text-sm text-destructive">
          {(mutation.error as Error).message}
        </p>
      )}

      <Button
        className="mt-1"
        size="lg"
        disabled={!valid || mutation.isPending}
        onClick={handleSubmit}
      >
        {mutation.isPending ? 'Posting…' : 'Post match'}
      </Button>
    </div>
  )
}

/** A numbered form section card, matching the mock's "1 · Sport" layout. */
function Section({
  num,
  title,
  done,
  children,
}: {
  /** The step number badge. Omit for optional/unnumbered sections (e.g. the
   *  header image), which then show only a done tick or a neutral dot. */
  num?: number
  title: string
  done?: boolean
  children: React.ReactNode
}) {
  return (
    <section className="rounded-xl border bg-card p-4">
      <div className="mb-3 flex items-center gap-2">
        <span
          className={cn(
            'inline-flex size-5 items-center justify-center rounded-full text-[11px] font-medium',
            done
              ? 'bg-success text-success-foreground'
              : 'bg-muted text-muted-foreground',
          )}
        >
          {done ? <Check className="size-3" /> : (num ?? '·')}
        </span>
        <h2 className="text-sm font-medium">{title}</h2>
      </div>
      {children}
    </section>
  )
}

/**
 * A disabled placeholder row for a section that unlocks later (e.g. Score). The
 * optional `hint` explains what the user must do first.
 */
function LockedRow({ label, hint }: { label: string; hint?: string }) {
  return (
    <div className="flex items-center gap-2 rounded-xl border bg-muted/40 px-4 py-3 text-muted-foreground">
      <span className="text-sm">{label}</span>
      {hint && <span className="text-xs">{hint}</span>}
      <Lock className="ml-auto size-3.5" />
    </div>
  )
}
