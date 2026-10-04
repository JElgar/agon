import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { format as formatDate } from 'date-fns'
import { ChevronLeft, Clock } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { isSetsSport, sportIcon, type MatchType } from '@/lib/sports'
import { colourFromName, defaultSideColour, nameFromColour } from '@/lib/sideColours'
import { suggestMatchName } from '@/lib/matchName'
import { cricketFormatLabel, type MatchFormat } from '@/lib/matchFormat'
import { Button } from '@/components/ui/button'
import { DateTimePicker } from '@/components/ui/date-time-picker'
import { SportPicker } from '@/components/agon/SportPicker'
import { MultiImageUploadField } from '@/components/agon/MultiImageUploadField'
import { Avatar } from '@/components/agon/Avatar'
import type { TaggedPlayer } from '@/components/agon/PlayerSideEditor'
import { FootballScoreFields } from '@/components/agon/FootballScoreFields'
import { CricketScoreFields } from '@/components/agon/CricketScoreFields'
import { NetballScoreFields } from '@/components/agon/NetballScoreFields'
import { CARD, FormSection, Segmented, Stepper, ToggleRow } from '@/components/agon/logmatch/parts'
import { FormatPresets } from '@/components/agon/logmatch/FormatPresets'
import { RacketPlayers, type RacketFormat } from '@/components/agon/logmatch/RacketPlayers'
import { TeamSideCard, type SideKind } from '@/components/agon/logmatch/TeamSideCard'
import { SetsScoreEditor } from '@/components/agon/logmatch/SetsScoreEditor'
import { formatDefaultHint, playedSets, type SetRow } from '@/lib/logMatch'
import { cn } from '@/lib/utils'
import { toDateTimeLocal } from '@/lib/datetime'
import { addPendingMatch } from '@/hooks/usePendingMatches'

type CreateMatchInput = components['schemas']['CreateMatchInput']
type UserProfile = components['schemas']['UserProfile']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']
type Score = components['schemas']['Score']
type TeamListItem = components['schemas']['TeamListItem']

/** A tagged side's players, reshaped into the API's `MatchPlayer` so the
 *  football/cricket/netball score editors (built for a real match's roster)
 *  can be reused before the match exists. `member.id` holds the reference
 *  the server resolves to a real player id: a tagged user's own id, or a
 *  guest's generated `TaggedPlayer.id`. */
function toMatchPlayers(players: TaggedPlayer[], sideId: string): MatchPlayer[] {
  return players.map((p) =>
    p.kind === 'user'
      ? {
          member: { type: 'User', id: p.id, user_id: p.id, name: p.name, avatar_url: p.imageUrl },
          side_id: sideId,
          role: 'player',
        }
      : {
          member: { type: 'External', id: p.id, display_name: p.name },
          side_id: sideId,
          role: 'player',
        },
  )
}

/** Client ids used to wire invites/score to the created sides. */
const SIDE_A = 'side-a'
const SIDE_B = 'side-b'

type MatchMode = 'scheduled' | 'completed'

const MODE_OPTIONS: { value: MatchMode; label: string }[] = [
  { value: 'scheduled', label: 'Upcoming' },
  { value: 'completed', label: 'Already played' },
]

/** First name, for compact pair labels like "James & Robbie". */
function firstName(name: string): string {
  return name.trim().split(/\s+/)[0] ?? name
}

/** A side's display name in this form, in the server's order (see
 *  `Api::resolve_side_names`): its own name, else its team's, else, for a
 *  racket sport only, its one or two players' names; `undefined` when none
 *  apply so callers can use their own fallback. */
function resolvedSideName(
  sport: MatchType | null,
  players: TaggedPlayer[],
  customName: string,
  team: TeamListItem | null,
  short = false,
): string | undefined {
  const trimmed = customName.trim()
  if (trimmed) return trimmed
  if (team) return team.name
  if (!sport || !isSetsSport(sport)) return undefined
  const names = players.map((p) => (short ? firstName(p.name) : p.name))
  if (names.length === 1) return names[0]
  if (names.length === 2) return `${names[0]} & ${names[1]}`
  return undefined
}

function defaultScheduledAt(): string {
  const d = new Date()
  d.setHours(d.getHours() + 1, 0, 0, 0)
  return toDateTimeLocal(d)
}

function defaultCompletedAt(): string {
  return toDateTimeLocal(new Date())
}

/** A short label for a picked format, e.g. "2 × 30 min" or "T20". */
function formatSummary(format: MatchFormat | null): string | undefined {
  if (!format) return undefined
  if (format.sport === 'Football') return `${format.num_halves} × ${format.half_length_minutes} min`
  if (format.sport === 'Cricket') return cricketFormatLabel(format)
  return `${format.num_quarters} × ${format.quarter_length_minutes} min`
}

/**
 * The "Log a match" flow, in three steps:
 *   1. Sport & time: sport, optional format, upcoming vs already played,
 *      when, and a match name suggested from the sport and time.
 *   2. Players: Singles/Doubles for racket sports; for everything else each
 *      side is an existing team or a one-off side with its own name/colour.
 *   3. Review (upcoming, no score) or Score (already played).
 * Posts `CreateMatchInput` to `POST /matches`, then returns to the feed.
 */
export function LogMatchPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const [step, setStep] = useState(0)

  const [sport, setSport] = useState<MatchType | null>(null)
  const [format, setFormat] = useState<MatchFormat | null>(null)
  const [mode, setMode] = useState<MatchMode>('scheduled')
  const [startsAt, setStartsAt] = useState<string>(defaultScheduledAt)
  // The name follows the sport and time until the user types their own;
  // clearing the field hands it back to the suggestion.
  const [typedName, setTypedName] = useState('')
  const suggestedName = suggestMatchName(sport, startsAt) ?? ''
  const name = typedName.trim() ? typedName : suggestedName

  const [racketFormat, setRacketFormat] = useState<RacketFormat>('singles')
  const [sideA, setSideA] = useState<TaggedPlayer[]>([])
  const [sideB, setSideB] = useState<TaggedPlayer[]>([])
  const [sideAKind, setSideAKind] = useState<SideKind>('team')
  const [sideBKind, setSideBKind] = useState<SideKind>('oneoff')
  const [sideATeam, setSideATeam] = useState<TeamListItem | null>(null)
  const [sideBTeam, setSideBTeam] = useState<TeamListItem | null>(null)
  const [sideAName, setSideANameRaw] = useState('')
  const [sideBName, setSideBNameRaw] = useState('')
  const [sideAColour, setSideAColourRaw] = useState(() => defaultSideColour())
  const [sideBColour, setSideBColourRaw] = useState(() => defaultSideColour(defaultSideColour()))
  const [sideANameTouched, setSideANameTouched] = useState(false)
  const [sideBNameTouched, setSideBNameTouched] = useState(false)
  const [sideAColourTouched, setSideAColourTouched] = useState(false)
  const [sideBColourTouched, setSideBColourTouched] = useState(false)
  const [sideAMaxPlayers, setSideAMaxPlayers] = useState('')
  const [sideBMaxPlayers, setSideBMaxPlayers] = useState('')
  const [sideATeamJoinEnabled, setSideATeamJoinEnabled] = useState(true)
  const [sideBTeamJoinEnabled, setSideBTeamJoinEnabled] = useState(true)
  const [allowUnassigned, setAllowUnassigned] = useState(true)
  const [headerAssetIds, setHeaderAssetIds] = useState<string[]>([])

  const [sets, setSets] = useState<SetRow[]>([
    { a: '', b: '' },
    { a: '', b: '' },
    { a: '', b: '' },
  ])
  const [pointsA, setPointsA] = useState('')
  const [pointsB, setPointsB] = useState('')
  const [detailBuilt, setDetailBuilt] = useState<{ score: Score; winnerSideId?: string } | null>(null)

  const racket = sport !== null && isSetsSport(sport)
  const isFootball = sport === 'football'
  const isCricket = sport === 'cricket'
  const isNetball = sport === 'netball'
  const racketCap = racketFormat === 'singles' ? 1 : 2

  const linkedA = !racket && sideAKind === 'team' ? sideATeam : null
  const linkedB = !racket && sideBKind === 'team' ? sideBTeam : null
  const sharedTeam = linkedA !== null && linkedA.id === linkedB?.id
  // The server only takes a name/colour on a side with no team, or on a
  // side whose team is also on the other side (a team playing itself).
  const sideAIdentity = !racket && (sideAKind === 'oneoff' || sharedTeam)
  const sideBIdentity = !racket && (sideBKind === 'oneoff' || sharedTeam)

  const setSideAName = (v: string) => {
    setSideANameRaw(v)
    setSideANameTouched(true)
    if (!sideAColourTouched) {
      const inferred = colourFromName(v)
      if (inferred) setSideAColourRaw(inferred)
    }
  }
  const setSideBName = (v: string) => {
    setSideBNameRaw(v)
    setSideBNameTouched(true)
    if (!sideBColourTouched) {
      const inferred = colourFromName(v)
      if (inferred) setSideBColourRaw(inferred)
    }
  }
  const setSideAColour = (hex: string) => {
    setSideAColourRaw(hex)
    setSideAColourTouched(true)
    if (!sideANameTouched) setSideANameRaw(nameFromColour(hex) ?? '')
  }
  const setSideBColour = (hex: string) => {
    setSideBColourRaw(hex)
    setSideBColourTouched(true)
    if (!sideBNameTouched) setSideBNameRaw(nameFromColour(hex) ?? '')
  }

  // A team playing itself needs a way to tell the sides apart: name each
  // untouched side after its kit colour as soon as that happens, so the
  // user never has to go back and fill one in.
  useEffect(() => {
    if (!sharedTeam) return
    if (!sideANameTouched) setSideANameRaw((n) => n || (nameFromColour(sideAColour) ?? ''))
    if (!sideBNameTouched) setSideBNameRaw((n) => n || (nameFromColour(sideBColour) ?? ''))
  }, [sharedTeam, sideANameTouched, sideBNameTouched, sideAColour, sideBColour])

  const changeSport = (next: MatchType) => {
    setSport(next)
    setFormat(null)
    if (isSetsSport(next)) {
      setSideA((p) => p.slice(0, racketCap))
      setSideB((p) => p.slice(0, racketCap))
    }
  }

  const changeRacketFormat = (next: RacketFormat) => {
    setRacketFormat(next)
    const cap = next === 'singles' ? 1 : 2
    setSideA((p) => p.slice(0, cap))
    setSideB((p) => p.slice(0, cap))
  }

  const changeMode = (next: MatchMode) => {
    setMode(next)
    setStartsAt(next === 'scheduled' ? defaultScheduledAt() : defaultCompletedAt())
  }

  const me = useQuery({
    queryKey: ['users-me'],
    queryFn: async (): Promise<UserProfile | null> => {
      const { data } = await fetchClient.GET('/users/me')
      return data?.profile ?? null
    },
  })
  const currentUserId = me.data?.id

  // Seed the current user onto side A once; removing yourself sticks.
  const [seededSelf, setSeededSelf] = useState(false)
  useEffect(() => {
    if (seededSelf || !me.data) return
    const self = me.data
    setSideA((prev) =>
      prev.some((p) => p.kind === 'user' && p.id === self.id)
        ? prev
        : [{ kind: 'user', id: self.id, name: self.name, imageUrl: self.profile_image?.image_url }, ...prev],
    )
    setSeededSelf(true)
  }, [me.data, seededSelf])

  const timeError = useMemo((): string | null => {
    if (!startsAt) return 'Pick a date and time'
    const ts = new Date(startsAt).getTime()
    if (Number.isNaN(ts)) return 'Pick a valid date and time'
    if (mode === 'completed' && ts > Date.now()) return 'A match that has been played must be in the past'
    if (mode === 'scheduled' && ts <= Date.now()) return 'An upcoming match must be in the future'
    return null
  }, [startsAt, mode])

  const step1Error = !sport ? 'Pick a sport' : !name.trim() ? 'Give the match a name' : timeError

  const maxError = (label: string, max: string, count: number): string | null => {
    if (!max.trim()) return null
    const n = Number(max)
    if (!Number.isInteger(n) || n < 1) return `${label}: max players must be a whole number`
    if (n < count) return `${label} has ${count} players, over its max of ${n}`
    return null
  }

  const sideALabel = resolvedSideName(sport, sideA, sideAIdentity ? sideAName : '', linkedA) ?? 'Your side'
  const sideBLabel = resolvedSideName(sport, sideB, sideBIdentity ? sideBName : '', linkedB) ?? 'Opposition'

  const step2Error = useMemo((): string | null => {
    if (sideA.length === 0) return 'Add at least one player to your side'
    if (racket) {
      if (sideB.length === 0) return racketFormat === 'singles' ? 'Invite an opponent' : 'Add at least one opponent'
      return null
    }
    if (sideAKind === 'team' && !sideATeam) return 'Pick a team for your side, or make it a one-off side'
    if (sideBKind === 'team' && !sideBTeam) return 'Pick a team for the opposition, or make it a one-off side'
    if (sideB.length === 0 && !(sideBIdentity && sideBName.trim()) && !linkedB)
      return 'Name the opposition or invite a player'
    return maxError('Your side', sideAMaxPlayers, sideA.length) ?? maxError('Opposition', sideBMaxPlayers, sideB.length)
  }, [
    sideA.length, sideB.length, racket, racketFormat, sideAKind, sideBKind, sideATeam, sideBTeam,
    sideBIdentity, sideBName, linkedB, sideAMaxPlayers, sideBMaxPlayers,
  ])

  const scoreError = useMemo((): string | null => {
    if (mode !== 'completed') return null
    if (isFootball || isCricket || isNetball) return detailBuilt ? null : 'Enter the score'
    if (racket) return playedSets(sets).length > 0 ? null : 'Enter the score for at least one set'
    const a = Number(pointsA)
    const b = Number(pointsB)
    if (pointsA === '' || pointsB === '' || !Number.isFinite(a) || !Number.isFinite(b))
      return 'Enter the score for both sides'
    return null
  }, [mode, isFootball, isCricket, isNetball, detailBuilt, racket, sets, pointsA, pointsB])

  const stepError = step === 0 ? step1Error : step === 1 ? step2Error : (step1Error ?? step2Error ?? scoreError)

  const mutation = useMutation({
    mutationFn: async (body: CreateMatchInput) => {
      const { data, error } = await fetchClient.POST('/matches', { body })
      if (error || !data) throw new Error(typeof error === 'string' ? error : 'Failed to post the match')
      return data
    },
    onSuccess: async (created) => {
      addPendingMatch(queryClient, created)
      queryClient.invalidateQueries({ queryKey: ['feed'] })
      navigate('/feed')
    },
  })

  const invitedUserCount = [...sideA, ...sideB].filter(
    (p) => p.kind === 'user' && p.id !== currentUserId,
  ).length

  const buildInvites = (): CreateMatchInput['invites'] => {
    const invites: CreateMatchInput['invites'] = []
    for (const [clientId, players] of [
      [SIDE_A, sideA],
      [SIDE_B, sideB],
    ] as const) {
      // The creator joins via `creator_side_client_id`, not as an invite.
      const invited_user_ids = players
        .filter((p) => p.kind === 'user' && p.id !== currentUserId)
        .map((p) => p.id)
      const invited_externals = players
        .filter((p): p is Extract<TaggedPlayer, { kind: 'external' }> => p.kind === 'external')
        .map((p) => ({ client_id: p.id, name: p.name }))
      if (invited_user_ids.length === 0 && invited_externals.length === 0) continue
      invites.push({ side_client_id: clientId, invited_user_ids, invited_externals })
    }
    return invites
  }

  const creatorSideClientId = (): string | undefined => {
    if (!currentUserId) return undefined
    if (sideA.some((p) => p.kind === 'user' && p.id === currentUserId)) return SIDE_A
    if (sideB.some((p) => p.kind === 'user' && p.id === currentUserId)) return SIDE_B
    return undefined
  }

  /** The score payload (with the `type` discriminator the generated
   *  `Omit<Score,"type">` drops) plus the derived winner. */
  const buildScore = (): { score: CreateMatchInput['score']; winner?: string } | null => {
    if (mode !== 'completed' || !sport) return null
    if (isFootball || isCricket || isNetball) {
      if (!detailBuilt) return null
      return {
        score: detailBuilt.score as unknown as CreateMatchInput['score'],
        winner: detailBuilt.winnerSideId,
      }
    }
    if (racket) {
      const rows = playedSets(sets)
      if (rows.length === 0) return null
      const aSets = rows.filter((r) => r.a > r.b).length
      const bSets = rows.filter((r) => r.b > r.a).length
      const score = {
        type: 'Sets',
        entries: { [SIDE_A]: rows.map((r) => r.a), [SIDE_B]: rows.map((r) => r.b) },
      } as unknown as CreateMatchInput['score']
      return { score, winner: aSets === bSets ? undefined : aSets > bSets ? SIDE_A : SIDE_B }
    }
    const a = Number(pointsA)
    const b = Number(pointsB)
    const score = { type: 'Simple', entries: { [SIDE_A]: a, [SIDE_B]: b } } as unknown as CreateMatchInput['score']
    return { score, winner: a === b ? undefined : a > b ? SIDE_A : SIDE_B }
  }

  const sideInput = (
    clientId: string,
    identity: boolean,
    customName: string,
    colour: string,
    team: TeamListItem | null,
    maxPlayers: string,
    teamJoinEnabled: boolean,
  ): CreateMatchInput['sides'][number] => {
    if (racket) return { client_id: clientId, colour, max_players: racketCap }
    return {
      client_id: clientId,
      name: identity ? customName.trim() || undefined : undefined,
      team_id: team?.id,
      colour: identity ? colour : undefined,
      max_players: maxPlayers.trim() ? Number(maxPlayers) : undefined,
      team_join_enabled: team ? teamJoinEnabled : undefined,
    }
  }

  const handleSubmit = () => {
    if (!sport || stepError) return
    const body: CreateMatchInput = {
      name: name.trim(),
      description: '',
      match_type: sport,
      starts_at: new Date(startsAt).toISOString(),
      sides: [
        sideInput(SIDE_A, sideAIdentity, sideAName, sideAColour, linkedA, sideAMaxPlayers, sideATeamJoinEnabled),
        sideInput(SIDE_B, sideBIdentity, sideBName, sideBColour, linkedB, sideBMaxPlayers, sideBTeamJoinEnabled),
      ],
      invites: buildInvites(),
    }
    const creatorSide = creatorSideClientId()
    if (creatorSide) body.creator_side_client_id = creatorSide
    if (headerAssetIds.length > 0) body.header_photo_asset_ids = headerAssetIds
    if (format) body.format = format
    body.allow_unassigned = allowUnassigned
    const scored = buildScore()
    if (scored) {
      body.score = scored.score
      if (scored.winner) body.winner_side_id = scored.winner
    }
    mutation.mutate(body)
  }

  const lastStepLabel = mode === 'completed' ? 'Score' : 'Review'
  const SportIcon = sport ? sportIcon(sport) : null
  const startsDate = new Date(startsAt)
  const whenLabel = Number.isNaN(startsDate.getTime()) ? '' : formatDate(startsDate, 'EEE d MMM · HH:mm')

  const goBack = () => (step === 0 ? navigate('/feed') : setStep(step - 1))
  const submitLabel =
    mode === 'completed'
      ? 'Post match'
      : invitedUserCount > 0
        ? `Create and send ${invitedUserCount} ${invitedUserCount === 1 ? 'invite' : 'invites'}`
        : 'Create match'

  const summaryChip = (
    <div className="flex items-center justify-between gap-3">
      <span className="inline-flex h-9 min-w-0 items-center gap-2 rounded-full bg-accent px-3.5 text-sm font-bold text-accent-foreground">
        {SportIcon && <SportIcon className="size-4 shrink-0" />}
        <span className="truncate">
          {name}
          {mode === 'completed' && ' · played'}
        </span>
      </span>
      <button type="button" onClick={() => setStep(0)} className="shrink-0 text-[13px] font-bold text-link">
        Change
      </button>
    </div>
  )

  return (
    <div className="mx-auto flex min-h-[calc(100vh-10rem)] max-w-xl flex-col gap-[18px]">
      <header className="-ml-2 flex items-center gap-1">
        <button
          type="button"
          onClick={goBack}
          aria-label={step === 0 ? 'Cancel' : 'Back'}
          className="flex size-11 items-center justify-center rounded-full hover:bg-accent"
        >
          <ChevronLeft className="size-[22px]" />
        </button>
        <h1 className="font-display text-[22px] font-extrabold">Log a match</h1>
      </header>

      <Stepper labels={['Sport & time', 'Players', lastStepLabel]} current={step} />

      {step === 0 && (
        <>
          <FormSection title="Sport">
            <SportPicker value={sport} onChange={changeSport} />
          </FormSection>

          {sport && formatDefaultHint(sport) && (
            <FormSection title="Match format" aside="Optional">
              <div className={CARD}>
                <FormatPresets sport={sport} value={format} onChange={setFormat} />
              </div>
              {format === null && <p className="px-1 text-xs text-muted-foreground">{formatDefaultHint(sport)}</p>}
            </FormSection>
          )}

          <FormSection title="When">
            <div className={CARD}>
              <Segmented label="Match status" options={MODE_OPTIONS} value={mode} onChange={changeMode} />
              <DateTimePicker id="starts-at" value={startsAt} onChange={setStartsAt} />
              {timeError && <p className="text-xs text-destructive">{timeError}</p>}
            </div>
          </FormSection>

          <FormSection title="Match name" htmlFor="match-name">
            <input
              id="match-name"
              value={typedName.trim() ? typedName : suggestedName}
              onChange={(e) => setTypedName(e.target.value)}
              placeholder="e.g. Tuesday night singles"
              maxLength={80}
              className="h-12 rounded-[14px] border bg-card px-3.5 text-[15px] font-semibold outline-none placeholder:font-normal placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
            />
          </FormSection>
        </>
      )}

      {step === 1 && (
        <>
          {summaryChip}
          {racket ? (
            <RacketPlayers
              format={racketFormat}
              onFormatChange={changeRacketFormat}
              sideA={sideA}
              sideB={sideB}
              onSideAChange={setSideA}
              onSideBChange={setSideB}
              currentUserId={currentUserId}
            />
          ) : (
            <>
              <TeamSideCard
                title="Your side"
                idPrefix="side-a"
                kind={sideAKind}
                onKindChange={(k) => {
                  setSideAKind(k)
                  if (k === 'oneoff') setSideATeam(null)
                }}
                team={sideATeam}
                onTeamChange={(t) => {
                  setSideATeam(t)
                  if (!t) setSideATeamJoinEnabled(true)
                }}
                sharedTeam={sharedTeam}
                name={sideAName}
                onNameChange={setSideAName}
                colour={sideAColour}
                onColourChange={setSideAColour}
                teamJoinEnabled={sideATeamJoinEnabled}
                onTeamJoinEnabledChange={setSideATeamJoinEnabled}
                players={sideA}
                onPlayersChange={setSideA}
                taken={[...sideA, ...sideB]}
                currentUserId={currentUserId}
                maxPlayers={sideAMaxPlayers}
                onMaxPlayersChange={setSideAMaxPlayers}
              />
              <TeamSideCard
                title="Opposition"
                idPrefix="side-b"
                kind={sideBKind}
                onKindChange={(k) => {
                  setSideBKind(k)
                  if (k === 'oneoff') setSideBTeam(null)
                }}
                team={sideBTeam}
                onTeamChange={(t) => {
                  setSideBTeam(t)
                  if (!t) setSideBTeamJoinEnabled(true)
                }}
                sharedTeam={sharedTeam}
                name={sideBName}
                onNameChange={setSideBName}
                colour={sideBColour}
                onColourChange={setSideBColour}
                teamJoinEnabled={sideBTeamJoinEnabled}
                onTeamJoinEnabledChange={setSideBTeamJoinEnabled}
                players={sideB}
                onPlayersChange={setSideB}
                taken={[...sideA, ...sideB]}
                currentUserId={currentUserId}
                maxPlayers={sideBMaxPlayers}
                onMaxPlayersChange={setSideBMaxPlayers}
              />
            </>
          )}
        </>
      )}

      {step === 2 && mode === 'scheduled' && (
        <>
          <section aria-label="Match summary" className={cn(CARD, 'gap-3.5 p-4')}>
            <div className="flex items-center gap-2.5">
              <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-accent text-accent-foreground">
                {SportIcon && <SportIcon className="size-5" />}
              </span>
              <div className="flex min-w-0 flex-col">
                <span className="truncate font-display text-lg font-extrabold">{name}</span>
                <span className="text-[13px] text-muted-foreground">
                  {[whenLabel, formatSummary(format)].filter(Boolean).join(' · ')}
                </span>
              </div>
            </div>
            <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
              <SideSummary
                label={sideALabel}
                team={linkedA}
                colour={racket ? undefined : sideAIdentity ? sideAColour : undefined}
                players={sideA}
                currentUserId={currentUserId}
              />
              <span className="text-xs font-bold text-muted-foreground">v</span>
              <SideSummary
                label={sideBLabel}
                team={linkedB}
                colour={racket ? undefined : sideBIdentity ? sideBColour : undefined}
                players={sideB}
                currentUserId={currentUserId}
              />
            </div>
          </section>

          <div className="flex items-start gap-2.5 rounded-[14px] bg-accent px-3.5 py-3 text-[13px] leading-snug text-accent-foreground">
            <Clock className="mt-px size-[18px] shrink-0" />
            <span>No score yet. You can score it live from the match page when it starts, or add the result afterwards.</span>
          </div>

          <FormSection title="Who can join">
            <div className={cn(CARD, 'py-1.5')}>
              <ToggleRow
                label="Join without picking a side"
                hint="For people who use a join link"
                checked={allowUnassigned}
                onChange={setAllowUnassigned}
              />
            </div>
          </FormSection>

          <FormSection title="Header images" aside="Optional">
            <MultiImageUploadField purpose="match_header" label="Add header images" onChange={setHeaderAssetIds} />
          </FormSection>
        </>
      )}

      {step === 2 && mode === 'completed' && (
        <>
          {summaryChip}
          <FormSection title={racket ? 'Sets' : 'Score'}>
            {(isFootball || isCricket || isNetball) &&
              (() => {
                const sideAObj: MatchSide = {
                  id: SIDE_A,
                  name: resolvedSideName(sport, sideA, sideAIdentity ? sideAName : '', linkedA),
                  team_id: linkedA?.id,
                  team_join_enabled: false,
                  player_count: 0,
                }
                const sideBObj: MatchSide = {
                  id: SIDE_B,
                  name: resolvedSideName(sport, sideB, sideBIdentity ? sideBName : '', linkedB),
                  team_id: linkedB?.id,
                  team_join_enabled: false,
                  player_count: 0,
                }
                const players = [...toMatchPlayers(sideA, SIDE_A), ...toMatchPlayers(sideB, SIDE_B)]
                const Editor = isFootball ? FootballScoreFields : isCricket ? CricketScoreFields : NetballScoreFields
                return (
                  <div className={CARD}>
                    <Editor sideA={sideAObj} sideB={sideBObj} players={players} onChange={setDetailBuilt} />
                  </div>
                )
              })()}

            {racket && (
              <SetsScoreEditor
                sideAName={resolvedSideName(sport, sideA, '', null, true) ?? 'Your side'}
                sideBName={resolvedSideName(sport, sideB, '', null, true) ?? 'Opposition'}
                rows={sets}
                onChange={setSets}
              />
            )}

            {!racket && !isFootball && !isCricket && !isNetball && (
              <div className={cn(CARD, 'grid grid-cols-[1fr_auto_1fr] items-end gap-2')}>
                <label className="flex flex-col gap-1 text-center text-xs text-muted-foreground">
                  <span className="truncate">{sideALabel}</span>
                  <input
                    type="number"
                    min={0}
                    inputMode="numeric"
                    value={pointsA}
                    onChange={(e) => setPointsA(e.target.value)}
                    placeholder="0"
                    className="h-12 rounded-xl border bg-card text-center font-display text-lg font-extrabold text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  />
                </label>
                <span className="pb-3 text-muted-foreground">–</span>
                <label className="flex flex-col gap-1 text-center text-xs text-muted-foreground">
                  <span className="truncate">{sideBLabel}</span>
                  <input
                    type="number"
                    min={0}
                    inputMode="numeric"
                    value={pointsB}
                    onChange={(e) => setPointsB(e.target.value)}
                    placeholder="0"
                    className="h-12 rounded-xl border bg-card text-center font-display text-lg font-extrabold text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  />
                </label>
              </div>
            )}
          </FormSection>

          <FormSection title="Header images" aside="Optional">
            <MultiImageUploadField purpose="match_header" label="Add header images" onChange={setHeaderAssetIds} />
          </FormSection>
        </>
      )}

      <div className="sticky bottom-0 z-10 -mx-4 mt-auto flex flex-col gap-2 border-t bg-card/95 px-4 pt-3 pb-[max(16px,env(safe-area-inset-bottom))] backdrop-blur supports-[backdrop-filter]:bg-card/80 md:mx-0 md:rounded-2xl md:border">
        {stepError && step > 0 && <p className="text-xs text-muted-foreground">{stepError}</p>}
        {mutation.isError && <p className="text-sm text-destructive">{(mutation.error as Error).message}</p>}
        {step < 2 ? (
          <Button
            className="h-13 rounded-2xl text-base"
            size="lg"
            disabled={!!stepError}
            onClick={() => setStep(step + 1)}
          >
            Continue
          </Button>
        ) : (
          <Button
            className="h-13 rounded-2xl text-base"
            size="lg"
            disabled={!!stepError || mutation.isPending}
            onClick={handleSubmit}
          >
            {mutation.isPending ? 'Posting…' : submitLabel}
          </Button>
        )}
      </div>
    </div>
  )
}

/** One side in the review summary: crest or colour dot, name, and who's on it. */
function SideSummary({
  label,
  team,
  colour,
  players,
  currentUserId,
}: {
  label: string
  team: TeamListItem | null
  colour?: string
  players: TaggedPlayer[]
  currentUserId?: string
}) {
  const inCount = players.filter((p) => p.kind === 'user' && p.id === currentUserId).length
  const invited = players.filter((p) => p.kind === 'user' && p.id !== currentUserId).length
  const guests = players.filter((p) => p.kind === 'external').length
  const counts = [
    inCount > 0 && `${inCount} in`,
    invited > 0 && `${invited} invited`,
    guests > 0 && `${guests} ${guests === 1 ? 'guest' : 'guests'}`,
  ].filter(Boolean)
  return (
    <div className="flex min-w-0 flex-col items-center gap-1.5 text-center">
      {team ? (
        <Avatar name={team.name} imageUrl={team.logo?.image_url} size="lg" className="size-10 rounded-xl" />
      ) : colour ? (
        <span className="size-10 rounded-full border border-black/10" style={{ backgroundColor: colour }} />
      ) : (
        <span className="flex -space-x-2">
          {players.slice(0, 2).map((p) => (
            <Avatar key={p.id} name={p.name} imageUrl={p.kind === 'user' ? p.imageUrl : undefined} size="lg" className="ring-2 ring-card" />
          ))}
        </span>
      )}
      <span className="max-w-full truncate text-sm font-bold">{label}</span>
      <span className="text-xs text-muted-foreground">{counts.join(' · ') || 'No players yet'}</span>
    </div>
  )
}
