import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Users } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { CARD, FormSection, Segmented } from '@/components/agon/logmatch/parts'
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
type TournamentEntry = components['schemas']['TournamentEntry']
type TournamentMatchSummary = components['schemas']['TournamentMatchSummary']
type TournamentStandingsRow = components['schemas']['TournamentStandingsRow']
type RotatingRoundMatchInput = { side_a: string[]; side_b: string[] }

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

  // Entrants for the manual-round-assignment form below — unlike
  // `entriesQuery` above, this stays enabled once the tournament's started,
  // since manual assignment needs the full accepted-entrant list on every
  // round, not just before the tournament starts.
  const [roundMode, setRoundMode] = useState<'auto' | 'manual'>('auto')
  const rotatingEntriesQuery = useQuery({
    queryKey: ['tournament', tournamentId, 'entries', 'all'],
    enabled: !!tournamentId && tournament?.format === 'rotating_players' && roundMode === 'manual',
    queryFn: () => listAllTournamentEntries(tournamentId!),
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

  // Omitting `matches` auto-forms the round (ranked by current standings);
  // supplying it assigns sides by hand instead.
  const nextRoundMutation = useMutation({
    mutationFn: async (matches?: RotatingRoundMatchInput[]) => {
      const { data, error } = await fetchClient.POST('/tournaments/{tournament_id}/rounds/next', {
        params: { path: { tournament_id: tournamentId! } },
        body: matches ? { matches } : {},
      })
      if (error || !data) {
        throw new Error(
          matches
            ? "Couldn't generate that round — check every side has the right number of players."
            : "Couldn't generate the next round — make sure every match in the current round has a confirmed score.",
        )
      }
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
              <>
                <Segmented
                  label="How to form the next round"
                  options={[
                    { value: 'auto' as const, label: 'Auto-form' },
                    { value: 'manual' as const, label: 'Assign manually' },
                  ]}
                  value={roundMode}
                  onChange={setRoundMode}
                />
                {roundMode === 'auto' ? (
                  <Button
                    disabled={nextRoundMutation.isPending}
                    onClick={() => nextRoundMutation.mutate(undefined)}
                  >
                    {nextRoundMutation.isPending ? 'Generating…' : 'Generate next round'}
                  </Button>
                ) : (
                  <ManualRoundForm
                    entries={rotatingEntriesQuery.data ?? []}
                    sideSize={tournament.squad_size ?? 1}
                    isPending={nextRoundMutation.isPending}
                    onSubmit={(matches) => nextRoundMutation.mutate(matches)}
                  />
                )}
              </>
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

/**
 * Hand-assign the next rotating-sides round: each accepted entrant gets an
 * optional match number plus side A/B; everyone left with no match number
 * sits this round out (same as an auto-formed round's byes). Submits once
 * every match number in use has exactly `sideSize` entrants on each side.
 */
function ManualRoundForm({
  entries,
  sideSize,
  isPending,
  onSubmit,
}: {
  entries: TournamentEntry[]
  sideSize: number
  isPending: boolean
  onSubmit: (matches: RotatingRoundMatchInput[]) => void
}) {
  const [assignments, setAssignments] = useState<Record<string, { match: string; side: 'A' | 'B' }>>(
    {},
  )

  const byMatch = new Map<string, { a: string[]; b: string[] }>()
  for (const entry of entries) {
    const a = assignments[entry.entrant_id]
    if (!a || a.match.trim() === '') continue
    const group = byMatch.get(a.match) ?? { a: [], b: [] }
    group[a.side === 'A' ? 'a' : 'b'].push(entry.entrant_id)
    byMatch.set(a.match, group)
  }
  const matchNumbers = Array.from(byMatch.keys()).sort((x, y) => Number(x) - Number(y))
  const matches: RotatingRoundMatchInput[] = matchNumbers.map((n) => {
    const { a, b } = byMatch.get(n)!
    return { side_a: a, side_b: b }
  })
  const allValid =
    matchNumbers.length > 0 &&
    matchNumbers.every((n) => {
      const { a, b } = byMatch.get(n)!
      return a.length === sideSize && b.length === sideSize
    })

  const setAssignment = (entrantId: string, patch: Partial<{ match: string; side: 'A' | 'B' }>) =>
    setAssignments((prev) => ({
      ...prev,
      [entrantId]: { match: '', side: 'A', ...prev[entrantId], ...patch },
    }))

  return (
    <div className="flex flex-col gap-3">
      <div className={CARD}>
        {entries.length === 0 && (
          <p className="py-2 text-center text-sm text-muted-foreground">No entrants to assign.</p>
        )}
        {entries.map((entry) => {
          const a = assignments[entry.entrant_id]
          return (
            <div key={entry.entrant_id} className="flex items-center gap-2 border-b py-2 last:border-b-0">
              <span className="flex-1 truncate text-sm font-semibold">{entry.entrant_id}</span>
              <Input
                className="h-8 w-16 text-center"
                type="number"
                min={1}
                placeholder="Match"
                value={a?.match ?? ''}
                onChange={(e) => setAssignment(entry.entrant_id, { match: e.target.value })}
              />
              <Segmented
                label={`${entry.entrant_id}'s side`}
                className="w-24"
                options={[
                  { value: 'A' as const, label: 'A' },
                  { value: 'B' as const, label: 'B' },
                ]}
                value={a?.side ?? 'A'}
                onChange={(side) => setAssignment(entry.entrant_id, { side })}
              />
            </div>
          )
        })}
      </div>
      <p className="text-xs text-muted-foreground">
        Give entrants the same match number to pair them, split between side A and B. Leave an
        entrant's match blank to sit them out this round. Every side needs exactly {sideSize}{' '}
        player{sideSize === 1 ? '' : 's'}.
      </p>
      <Button disabled={!allValid || isPending} onClick={() => onSubmit(matches)}>
        {isPending ? 'Submitting…' : 'Submit round'}
      </Button>
    </div>
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
