import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Plus, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { DateTimePicker } from '@/components/ui/date-time-picker'
import { CARD, FormSection } from '@/components/agon/logmatch/parts'
import { MAX_START_HOURS_IN_PAST, isTooFarInPast } from '@/lib/datetime'
import { sideDisplayName } from '@/lib/members'
import { sportLabel } from '@/lib/sports'
import {
  groupMatchesByRound,
  listAllTournamentEntries,
  listAllTournamentMatches,
  tournamentEntryAudienceLabel,
  tournamentFormatLabel,
  tournamentStatusLabel,
  type TournamentEntry,
} from '@/lib/tournaments'
import { useEntrantDisplays } from '@/pages/TournamentEntriesPage'

type Tournament = components['schemas']['Tournament']
type TournamentMatchSummary = components['schemas']['TournamentMatchSummary']
type TournamentStandingsRow = components['schemas']['TournamentStandingsRow']
type CreateMatchInput = components['schemas']['CreateMatchInput']
// openapi-typescript emits `Tournament['generation']` as
// `Omit<TournamentGenerationInput, "type"> & unknown` (a codegen quirk on
// this optional discriminated-union field — see `CreateMatchSeriesInput['ends']`
// for the same shape), which drops `type` from the visible fields. Read
// through the actual union type instead of that mangled one.
type TournamentGenerationInput = components['schemas']['TournamentGenerationInput']

/**
 * Tournament hub: a scheduled tournament shows entries progress and a Start
 * button; a started one shows either a knockout bracket (grouped by round,
 * via `GET /tournaments/{id}/matches`) or a league's standings table (`GET
 * /tournaments/{id}/standings`) plus fixtures by round. Each match row links
 * to the existing `/matches/:id` page for full detail/live scoring.
 */
export function TournamentPage() {
  const { tournamentId } = useParams<{ tournamentId: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const tournamentQuery = useQuery({
    queryKey: ['tournament', tournamentId],
    enabled: !!tournamentId,
    queryFn: async (): Promise<Tournament> => {
      const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data) throw new Error('Failed to load this tournament')
      return data
    },
  })

  const tournament = tournamentQuery.data
  const started = tournament?.status === 'in_progress' || tournament?.status === 'completed'

  // Always fetched, not just pre-start: a manual-generation league's "Add
  // match" picker (below) needs the accepted entrants/squads to choose
  // sides from even once the tournament is under way.
  const entriesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'entries'],
    enabled: !!tournamentId,
    queryFn: () => listAllTournamentEntries(tournamentId!),
  })

  const matchesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'matches'],
    enabled: !!tournamentId && started,
    queryFn: () => listAllTournamentMatches(tournamentId!),
  })

  const standingsQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'standings'],
    enabled: !!tournamentId && started && tournament?.format === 'league',
    queryFn: async (): Promise<TournamentStandingsRow[]> => {
      const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}/standings', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data) throw new Error('Failed to load standings')
      return data.rows
    },
  })

  const startMutation = useMutation({
    mutationFn: async () => {
      const { data, error } = await fetchClient.POST('/tournaments/{tournament_id}/start', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data)
        throw new Error(typeof error === 'string' ? error : 'Could not start this tournament')
      return data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tournament', tournamentId] })
    },
    onError: (err) => {
      // A teamless tournament's own start failures are always about its
      // entrants (squad assignment, or too few squads) — rather than just
      // blocking on the error, send the organiser straight to the entries
      // page (now a drag-and-drop squad board) with the reason, so they can
      // fix it on the spot instead of bouncing back and forth.
      if (tournament?.entry_audience === 'players') {
        navigate(`/tournaments/${tournamentId}/entries`, {
          state: { startBlockedReason: err.message },
        })
      }
    },
  })

  if (!tournament) {
    return (
      <div className="mx-auto w-full max-w-xl md:max-w-2xl">
        <div className="h-28 animate-pulse rounded-2xl border bg-card" aria-hidden />
      </div>
    )
  }

  const entries = entriesQuery.data ?? []
  const matches = matchesQuery.data ?? []
  const entryCount = started ? tournament.entry_count : entries.length
  const canStart = !started && entryCount >= tournament.min_teams

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-5 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="icon" className="-ml-2" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <div className="flex-1">
          <h1 className="font-display text-xl font-extrabold md:text-2xl">{tournament.name}</h1>
          <p className="text-sm text-muted-foreground">
            {sportLabel(tournament.match_type)} · {tournamentFormatLabel(tournament.format)} ·{' '}
            {tournamentStatusLabel(tournament.status)}
          </p>
        </div>
      </div>

      {!started && (
        <FormSection
          title="Entries"
          aside={
            <Link to={`/tournaments/${tournament.id}/entries`} className="underline">
              Manage
            </Link>
          }
        >
          <div className={CARD}>
            <div className="flex items-center gap-2">
              <Users className="size-4 text-muted-foreground" />
              <span className="text-sm font-semibold">
                {entryCount} {tournamentEntryAudienceLabel(tournament.entry_audience).toLowerCase()}
                {tournament.max_teams ? ` / ${tournament.max_teams}` : ''}
              </span>
            </div>
            <p className="text-xs text-muted-foreground">
              Needs at least {tournament.min_teams} to start.
            </p>
          </div>
          {startMutation.isError && (
            <p className="text-sm text-destructive">{startMutation.error.message}</p>
          )}
          <Button
            size="lg"
            disabled={!canStart || startMutation.isPending}
            onClick={() => startMutation.mutate()}
          >
            {startMutation.isPending ? 'Starting…' : 'Start tournament'}
          </Button>
        </FormSection>
      )}

      {started && tournament.format === 'league' && (
        <FormSection title="Standings">
          <StandingsTable rows={standingsQuery.data ?? []} names={entrantNames(matches)} />
        </FormSection>
      )}

      {started && (
        <FormSection title={tournament.format === 'knockout' ? 'Bracket' : 'Fixtures'}>
          <div className="flex flex-col gap-4">
            {groupMatchesByRound(matches).map(([round, roundMatches]) => (
              <div key={round} className="flex flex-col gap-2">
                <h3 className="px-1 text-xs font-bold uppercase text-muted-foreground">
                  {tournament.format === 'knockout' ? roundLabel(round, matches.length) : `Matchday ${round + 1}`}
                </h3>
                <div className={CARD}>
                  {roundMatches.map((match) => (
                    <TournamentMatchRow key={match.id} match={match} />
                  ))}
                </div>
              </div>
            ))}
            {matches.length === 0 && (
              <p className="py-2 text-center text-sm text-muted-foreground">No matches yet.</p>
            )}
          </div>
        </FormSection>
      )}

      {started &&
        tournament.format === 'league' &&
        (tournament.generation as TournamentGenerationInput | undefined)?.type === 'Manual' && (
        <AddTournamentMatchSection tournament={tournament} entries={entriesQuery.data ?? []} />
      )}
    </div>
  )
}

/**
 * A manual-generation league tournament creates no fixtures on its own (see
 * `TournamentGenerationInput`'s doc comment) — this is how the organiser
 * adds them, one at a time, via `POST /matches` with `tournament_id` set
 * (`CreateMatchInput.tournament_id`). Picks two accepted entrants (teams) or
 * two squads (a teamless tournament), same shape `start_tournament`'s own
 * generation uses.
 */
function AddTournamentMatchSection({
  tournament,
  entries,
}: {
  tournament: Tournament
  entries: TournamentEntry[]
}) {
  const queryClient = useQueryClient()
  const accepted = entries.filter((e) => e.status === 'accepted')
  const isTeamless = tournament.entry_audience === 'players'
  const squads = [...new Set(accepted.map((e) => e.squad).filter((s): s is number => s != null))].sort(
    (a, b) => a - b,
  )
  const entrantDisplays = useEntrantDisplays(
    accepted.map((e) => e.entrant_id),
    tournament.entry_audience,
  )

  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [startsAt, setStartsAt] = useState('')
  const [sideA, setSideA] = useState('')
  const [sideB, setSideB] = useState('')

  const options = isTeamless
    ? squads.map((s) => ({ value: String(s), label: tournament.squad_labels[String(s)]?.name ?? `Squad ${s + 1}` }))
    : accepted.map((e) => ({ value: e.entrant_id, label: entrantDisplays.get(e.entrant_id)?.name ?? e.entrant_id }))

  const timeError = !startsAt
    ? null
    : isTooFarInPast(startsAt)
      ? `A match can't start more than ${MAX_START_HOURS_IN_PAST} hours in the past`
      : null

  const canSave = name.trim() !== '' && !!startsAt && !timeError && !!sideA && !!sideB && sideA !== sideB

  const addMatchMutation = useMutation({
    mutationFn: async () => {
      const body: CreateMatchInput = {
        tournament_id: tournament.id,
        match_type: tournament.match_type,
        name: name.trim(),
        description: '',
        starts_at: new Date(startsAt).toISOString(),
        format: tournament.match_format,
        sides: isTeamless
          ? [
              { client_id: 'a', tournament_squad: Number(sideA) },
              { client_id: 'b', tournament_squad: Number(sideB) },
            ]
          : [
              { client_id: 'a', team_id: sideA },
              { client_id: 'b', team_id: sideB },
            ],
        invites: [],
      }
      const { data, error } = await fetchClient.POST('/matches', { body })
      if (error || !data)
        throw new Error(typeof error === 'string' ? error : 'Could not add this match')
      return data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tournament', tournament.id, 'matches'] })
      queryClient.invalidateQueries({ queryKey: ['tournament', tournament.id, 'standings'] })
      setOpen(false)
      setName('')
      setStartsAt('')
      setSideA('')
      setSideB('')
    },
  })

  return (
    <FormSection
      title="Add match"
      aside={
        !open ? (
          <button className="flex items-center gap-1 underline" onClick={() => setOpen(true)}>
            <Plus className="size-3.5" /> Add
          </button>
        ) : undefined
      }
    >
      {open && (
        <div className={CARD}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="add-match-name">Name</Label>
            <Input id="add-match-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-match-side-a">{isTeamless ? 'Squad A' : 'Team A'}</Label>
              <select
                id="add-match-side-a"
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={sideA}
                onChange={(e) => setSideA(e.target.value)}
              >
                <option value="" disabled>
                  Select
                </option>
                {options.map((o) => (
                  <option key={o.value} value={o.value} disabled={o.value === sideB}>
                    {o.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-match-side-b">{isTeamless ? 'Squad B' : 'Team B'}</Label>
              <select
                id="add-match-side-b"
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={sideB}
                onChange={(e) => setSideB(e.target.value)}
              >
                <option value="" disabled>
                  Select
                </option>
                {options.map((o) => (
                  <option key={o.value} value={o.value} disabled={o.value === sideA}>
                    {o.label}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="add-match-starts-at">Starts</Label>
            <DateTimePicker id="add-match-starts-at" value={startsAt} onChange={setStartsAt} />
            {timeError && <p className="text-xs text-destructive">{timeError}</p>}
          </div>
          {addMatchMutation.isError && (
            <p className="text-sm text-destructive">{addMatchMutation.error.message}</p>
          )}
          <div className="flex gap-2">
            <Button
              disabled={!canSave || addMatchMutation.isPending}
              onClick={() => addMatchMutation.mutate()}
            >
              {addMatchMutation.isPending ? 'Adding…' : 'Add match'}
            </Button>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </FormSection>
  )
}

function roundLabel(round: number, totalRounds: number): string {
  const roundsFromEnd = totalRounds - round
  if (roundsFromEnd === 1) return 'Final'
  if (roundsFromEnd === 2) return 'Semi-final'
  if (roundsFromEnd === 3) return 'Quarter-final'
  return `Round ${round + 1}`
}

function TournamentMatchRow({ match }: { match: TournamentMatchSummary }) {
  const [sideA, sideB] = match.sides
  const nameA = sideDisplayName(sideA) ?? 'Side A'
  const nameB = sideDisplayName(sideB) ?? 'Side B'
  const winnerId = match.confirmed_score?.winner_side_id

  return (
    <Link
      to={`/matches/${match.id}`}
      className="flex items-center justify-between gap-2 border-b py-2.5 text-sm last:border-b-0"
    >
      <span className={winnerId === sideA?.id ? 'font-bold' : undefined}>{nameA}</span>
      <span className="text-xs text-muted-foreground">
        {match.status === 'completed' ? 'FT' : match.status === 'in_progress' ? 'Live' : 'vs'}
      </span>
      <span className={winnerId === sideB?.id ? 'font-bold' : undefined}>{nameB}</span>
    </Link>
  )
}

/** A standings row's `entrant_id` is the `team_id` its match sides carry —
 *  a real team's id, or a teamless tournament's synthetic `"squad-<n>"` —
 *  so its display name comes off those sides (a squad's organiser-set name
 *  is the side's own `name`), the same way `TournamentMatchRow` names them. */
function entrantNames(matches: TournamentMatchSummary[]): Map<string, string> {
  const names = new Map<string, string>()
  for (const match of matches) {
    for (const side of match.sides) {
      const name = sideDisplayName(side)
      if (side.team_id && name && !names.has(side.team_id)) names.set(side.team_id, name)
    }
  }
  return names
}

function StandingsTable({ rows, names }: { rows: TournamentStandingsRow[]; names: Map<string, string> }) {
  if (rows.length === 0) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">No standings yet.</p>
      </div>
    )
  }
  return (
    <div className={CARD}>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-xs text-muted-foreground">
            <th className="pb-1.5">Entrant</th>
            <th className="pb-1.5 text-center">P</th>
            <th className="pb-1.5 text-center">W</th>
            <th className="pb-1.5 text-center">D</th>
            <th className="pb-1.5 text-center">L</th>
            <th className="pb-1.5 text-right">Pts</th>
          </tr>
        </thead>
        <tbody>
          {rows
            .slice()
            .sort((a, b) => b.points - a.points)
            .map((row) => (
              <tr key={row.entrant_id} className="border-t">
                <td className="truncate py-1.5 font-semibold">{names.get(row.entrant_id) ?? row.entrant_id}</td>
                <td className="text-center">{row.played}</td>
                <td className="text-center">{row.won}</td>
                <td className="text-center">{row.drawn}</td>
                <td className="text-center">{row.lost}</td>
                <td className="text-right font-bold">{row.points}</td>
              </tr>
            ))}
        </tbody>
      </table>
    </div>
  )
}
