import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { CARD, FormSection } from '@/components/agon/logmatch/parts'
import { sideDisplayName } from '@/lib/members'
import { sportLabel } from '@/lib/sports'
import {
  groupMatchesByRound,
  listAllTournamentEntries,
  listAllTournamentMatches,
  tournamentEntryAudienceLabel,
  tournamentFormatLabel,
  tournamentStatusLabel,
} from '@/lib/tournaments'

type Tournament = components['schemas']['Tournament']
type TournamentMatchSummary = components['schemas']['TournamentMatchSummary']
type TournamentStandingsRow = components['schemas']['TournamentStandingsRow']

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

  const entriesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'entries'],
    enabled: !!tournamentId && !started,
    queryFn: () => listAllTournamentEntries(tournamentId!),
  })

  const matchesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'matches'],
    enabled: !!tournamentId && started,
    queryFn: () => listAllTournamentMatches(tournamentId!),
  })

  const standingsQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'standings'],
    enabled:
      !!tournamentId &&
      started &&
      (tournament?.format === 'league' || tournament?.format === 'rotating_players'),
    queryFn: async (): Promise<TournamentStandingsRow[]> => {
      const { data, error } = await fetchClient.GET('/tournaments/{tournament_id}/standings', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data) throw new Error('Failed to load standings')
      return data.rows
    },
  })

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['tournament', tournamentId] })

  const startMutation = useMutation({
    mutationFn: async () => {
      const { data, error } = await fetchClient.POST('/tournaments/{tournament_id}/start', {
        params: { path: { tournament_id: tournamentId! } },
      })
      if (error || !data) throw new Error('Could not start this tournament')
      return data
    },
    onSuccess: invalidate,
  })

  // Auto-forms the next round (ranked by current standings) — manual
  // per-round side assignment isn't built into this page yet; an organiser
  // who wants to hand-pick a round can still do it via the API directly.
  const nextRoundMutation = useMutation({
    mutationFn: async () => {
      const { data, error } = await fetchClient.POST('/tournaments/{tournament_id}/rounds/next', {
        params: { path: { tournament_id: tournamentId! } },
        body: {},
      })
      if (error || !data) throw new Error("Couldn't generate the next round — make sure every match in the current round has a confirmed score.")
      return data
    },
    onSuccess: invalidate,
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

      {started && (tournament.format === 'league' || tournament.format === 'rotating_players') && (
        <FormSection title="Standings">
          <StandingsTable rows={standingsQuery.data ?? []} />
        </FormSection>
      )}

      {started && tournament.format === 'rotating_players' && tournament.status === 'in_progress' && (
        <FormSection title="Rounds">
          {tournament.current_round != null && tournament.rounds != null && (
            <p className="text-sm text-muted-foreground">
              Round {tournament.current_round + 1} of {tournament.rounds}.
            </p>
          )}
          {nextRoundMutation.isError && (
            <p className="text-sm text-destructive">{nextRoundMutation.error.message}</p>
          )}
          {tournament.current_round != null &&
            tournament.rounds != null &&
            tournament.current_round + 1 < tournament.rounds && (
              <Button
                disabled={nextRoundMutation.isPending}
                onClick={() => nextRoundMutation.mutate()}
              >
                {nextRoundMutation.isPending ? 'Generating…' : 'Generate next round'}
              </Button>
            )}
        </FormSection>
      )}

      {started && (
        <FormSection title={tournament.format === 'knockout' ? 'Bracket' : 'Fixtures'}>
          <div className="flex flex-col gap-4">
            {groupMatchesByRound(matches).map(([round, roundMatches]) => (
              <div key={round} className="flex flex-col gap-2">
                <h3 className="px-1 text-xs font-bold uppercase text-muted-foreground">
                  {tournament.format === 'knockout'
                    ? roundLabel(round, matches.length)
                    : tournament.format === 'rotating_players'
                      ? `Round ${round + 1}`
                      : `Matchday ${round + 1}`}
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
    </div>
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

function StandingsTable({ rows }: { rows: TournamentStandingsRow[] }) {
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
                <td className="truncate py-1.5 font-semibold">{row.entrant_id}</td>
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
