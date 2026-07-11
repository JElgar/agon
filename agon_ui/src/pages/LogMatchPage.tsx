import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Lock } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import type { MatchType } from '@/lib/sports'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { SportPicker } from '@/components/agon/SportPicker'
import {
  PlayerSideEditor,
  type TaggedPlayer,
} from '@/components/agon/PlayerSideEditor'
import { cn } from '@/lib/utils'

type CreateMatchInput = components['schemas']['CreateMatchInput']
type UserProfile = components['schemas']['UserProfile']

/** Client ids used to wire invites/score to the created sides (see CreateMatchSideInput). */
const SIDE_A = 'side-a'
const SIDE_B = 'side-b'

/** Racket sports score by sets; everything else by a single points total. */
function isSetsSport(sport: MatchType): boolean {
  return (
    sport === 'tennis' ||
    sport === 'badminton' ||
    sport === 'squash' ||
    sport === 'table_tennis'
  )
}

/** One row of the sets editor: games won by each side in a single set. */
interface SetRow {
  a: string
  b: string
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
  const [name, setName] = useState('')
  const [sideA, setSideA] = useState<TaggedPlayer[]>([])
  const [sideB, setSideB] = useState<TaggedPlayer[]>([])

  // Result entry (optional). When off, an upcoming match is created (no score).
  const [recordResult, setRecordResult] = useState(false)
  const [sets, setSets] = useState<SetRow[]>([
    { a: '', b: '' },
    { a: '', b: '' },
  ])
  const [pointsA, setPointsA] = useState('')
  const [pointsB, setPointsB] = useState('')

  // The signed-in user's profile, so their own side shows a "you" chip. Best
  // effort — the form still works (with a generic label) if it can't load.
  const me = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<UserProfile | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile ?? null
    },
  })
  const youName = me.data?.name ?? 'You'

  // Ids already tagged, so a person can't be added to both sides.
  const sideAUserIds = sideA.flatMap((p) => (p.kind === 'user' ? [p.id] : []))
  const sideBUserIds = sideB.flatMap((p) => (p.kind === 'user' ? [p.id] : []))

  const setsPlayable = isSetsSport(sport ?? 'other')

  // Validation: a sport, a match name, and at least one opponent. The creator is
  // the implicit sole participant on their own side (a "you" chip), so their side
  // needs no explicit players; the opposition must have at least one person.
  const valid = useMemo(() => {
    if (!sport) return false
    if (name.trim().length === 0) return false
    if (sideB.length === 0) return false
    return true
  }, [sport, name, sideB.length])

  const mutation = useMutation({
    mutationFn: async (body: CreateMatchInput) => {
      const { data, error } = await fetchClient.POST('/matches', { body })
      if (error || !data)
        throw new Error(
          typeof error === 'string' ? error : 'Failed to post the match',
        )
      return data
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['feed'] })
      navigate('/feed')
    },
  })

  const buildInvites = (): CreateMatchInput['invites'] => {
    const invites: CreateMatchInput['invites'] = []
    for (const [clientId, players] of [
      [SIDE_A, sideA],
      [SIDE_B, sideB],
    ] as const) {
      const invited_user_ids = players
        .filter((p) => p.kind === 'user')
        .map((p) => (p as Extract<TaggedPlayer, { kind: 'user' }>).id)
      const invited_external_names = players
        .filter((p) => p.kind === 'external')
        .map((p) => p.name)
      if (invited_user_ids.length === 0 && invited_external_names.length === 0)
        continue
      invites.push({
        side_client_id: clientId,
        invited_user_ids,
        invited_external_names,
      })
    }
    return invites
  }

  /** Build the score payload (with the `type` discriminator the server requires,
   *  which the generated `Omit<Score,"type">` drops) plus the derived winner. */
  const buildScore = ():
    | { score: CreateMatchInput['score']; winner?: string }
    | null => {
    if (!recordResult || !sport) return null

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
        entries: [
          { side_id: SIDE_A, sets: rows.map((r) => r.a) },
          { side_id: SIDE_B, sets: rows.map((r) => r.b) },
        ],
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
      entries: [
        { side_id: SIDE_A, points: a },
        { side_id: SIDE_B, points: b },
      ],
    } as unknown as CreateMatchInput['score']
    const winner = a === b ? undefined : a > b ? SIDE_A : SIDE_B
    return { score, winner }
  }

  const handleSubmit = () => {
    if (!sport || !valid) return

    const oppositionName =
      sideB.length === 1 ? sideB[0].name : 'Opposition'

    const body: CreateMatchInput = {
      name: name.trim(),
      description: '',
      match_type: sport,
      starts_at: new Date().toISOString(),
      sides: [
        { client_id: SIDE_A, name: youName },
        { client_id: SIDE_B, name: oppositionName },
      ],
      invites: buildInvites(),
    }

    const scored = buildScore()
    if (scored) {
      body.score = scored.score
      if (scored.winner) body.winner_side_id = scored.winner
    }

    mutation.mutate(body)
  }

  const playersSet = sport !== null && sideB.length > 0

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
        <SportPicker value={sport} onChange={setSport} />
      </Section>

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

      {/* 3 · Players */}
      <Section num={3} title="Players" done={playersSet}>
        <div className="flex flex-col gap-2.5">
          <PlayerSideEditor
            title="Your side"
            searchPlaceholder="Add a teammate…"
            players={sideA}
            onChange={setSideA}
            youName={youName}
            excludeUserIds={sideBUserIds}
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
            excludeUserIds={sideAUserIds}
          />
        </div>
      </Section>

      {/* 4 · Score (locked until players are set) */}
      {playersSet ? (
        <Section num={4} title="Score">
          <div className="mb-3 flex items-center justify-between">
            <span className="text-sm text-muted-foreground">
              Record the result now
            </span>
            <Switch
              checked={recordResult}
              onCheckedChange={setRecordResult}
              aria-label="Record the result now"
            />
          </div>

          {recordResult && setsPlayable && (
            <div className="flex flex-col gap-2">
              <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2 text-center text-[11px] uppercase tracking-wider text-muted-foreground">
                <span className="truncate text-left">{youName}</span>
                <span>Set</span>
                <span className="truncate text-right">
                  {sideB.length === 1 ? sideB[0].name : 'Opposition'}
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

          {recordResult && !setsPlayable && (
            <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
              <div className="flex flex-col gap-1">
                <span className="truncate text-center text-xs text-muted-foreground">
                  {youName}
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
                  {sideB.length === 1 ? sideB[0].name : 'Opposition'}
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
        </Section>
      ) : (
        <LockedRow label="Score" />
      )}

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
  num: number
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
          {done ? <Check className="size-3" /> : num}
        </span>
        <h2 className="text-sm font-medium">{title}</h2>
      </div>
      {children}
    </section>
  )
}

/** A disabled placeholder row for a section that unlocks later (e.g. Score). */
function LockedRow({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-2 rounded-xl border bg-muted/40 px-4 py-3 text-muted-foreground">
      <span className="text-sm">{label}</span>
      <Lock className="ml-auto size-3.5" />
    </div>
  )
}
