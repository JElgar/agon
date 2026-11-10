import { useState } from 'react'
import { useQueries, useQuery, useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, Plus } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { DateTimePicker } from '@/components/ui/date-time-picker'
import { CARD, FormSection } from '@/components/agon/logmatch/parts'
import { Avatar } from '@/components/agon/Avatar'
import { MatchTabBar } from '@/components/agon/football/FootballMatchView'
import { sideDisplayName } from '@/lib/members'
import { sportLabel } from '@/lib/sports'
import { defaultSquadLabel, resolveSideColour, teamCrestColor } from '@/lib/sideColours'
import {
  footballEventSourceFromScore,
  goalContributions,
  sortGoalContributions,
  type GoalContributionEntry,
} from '@/lib/liveScore'
import {
  groupMatchesByRound,
  listAllTournamentEntries,
  listAllTournamentMatches,
  tournamentEntryAudienceLabel,
  tournamentFormatLabel,
  tournamentStatusLabel,
  type TournamentEntry,
} from '@/lib/tournaments'
import { useEntrantDisplays, type EntrantDisplay } from '@/pages/TournamentEntriesPage'

type Tournament = components['schemas']['Tournament']
type TournamentMatchSummary = components['schemas']['TournamentMatchSummary']
type TournamentStandingsRow = components['schemas']['TournamentStandingsRow']
type CreateMatchInput = components['schemas']['CreateMatchInput']
type Team = components['schemas']['Team']
type TeamMember = components['schemas']['TeamMember']
type Match = components['schemas']['Match']
// openapi-typescript emits `Tournament['generation']` as
// `Omit<TournamentGenerationInput, "type"> & unknown` (a codegen quirk on
// this optional discriminated-union field — see `CreateMatchSeriesInput['ends']`
// for the same shape), which drops `type` from the visible fields. Read
// through the actual union type instead of that mangled one.
type TournamentGenerationInput = components['schemas']['TournamentGenerationInput']

type TournamentTab = 'joined' | 'squads' | 'matches' | 'table' | 'stats'

/**
 * Tournament hub, laid out the same way as a match's detail page
 * (`MatchDetailPage`): a tab bar instead of one long scroll. Joined (who's
 * entered), Squads (each squad/team's roster, with its name and colour),
 * Matches (every generated fixture, grouped by round) and Table — a
 * league's standings, or a knockout's bracket.
 */
export function TournamentPage() {
  const { tournamentId } = useParams<{ tournamentId: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [tab, setTab] = useState<TournamentTab>('joined')

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

  // Always fetched, not just pre-start: the Squads tab and a manual
  // league's "Add match" picker (below) both need the accepted
  // entrants/squads even once the tournament is under way.
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
      // page (a drag-and-drop squad board) with the reason, so they can
      // fix it on the spot instead of bouncing back and forth.
      if (tournament?.entry_audience === 'players') {
        navigate(`/tournaments/${tournamentId}/entries`, {
          state: { startBlockedReason: err.message },
        })
      }
    },
  })

  const entries = entriesQuery.data ?? []
  const entrantDisplays = useEntrantDisplays(
    entries.map((e) => e.entrant_id),
    tournament?.entry_audience,
  )

  if (!tournament) {
    return (
      <div className="mx-auto w-full max-w-xl md:max-w-2xl">
        <div className="h-28 animate-pulse rounded-2xl border bg-card" aria-hidden />
      </div>
    )
  }

  const matches = matchesQuery.data ?? []
  const entryCount = started ? tournament.entry_count : entries.length
  const canStart = !started && entryCount >= tournament.min_teams
  const isTeamless = tournament.entry_audience === 'players'

  // Goal/assist leaders, aggregated across the tournament's matches — only
  // meaningful for football today (the one sport with a per-match
  // "Goals & assists" breakdown to aggregate — see `TournamentStatsTab`'s
  // doc comment for cricket/netball).
  const supportsStats = tournament.match_type === 'football'

  const tabs: { id: TournamentTab; label: string }[] = [
    { id: 'joined', label: 'Joined' },
    { id: 'squads', label: isTeamless ? 'Squads' : 'Teams' },
    { id: 'matches', label: 'Matches' },
    { id: 'table', label: tournament.format === 'knockout' ? 'Bracket' : 'Table' },
    ...(supportsStats ? [{ id: 'stats' as const, label: 'Stats' }] : []),
  ]

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 md:max-w-2xl">
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

      <MatchTabBar tabs={tabs} value={tab} onChange={setTab} />

      {tab === 'joined' && (
        <JoinedTab
          tournament={tournament}
          entries={entries}
          entrantDisplays={entrantDisplays}
          started={started}
          entryCount={entryCount}
          canStart={canStart}
          starting={startMutation.isPending}
          startError={startMutation.isError ? startMutation.error.message : undefined}
          onStart={() => startMutation.mutate()}
        />
      )}

      {tab === 'squads' && (
        <SquadsTab tournament={tournament} entries={entries} entrantDisplays={entrantDisplays} />
      )}

      {tab === 'matches' && (
        <MatchesTab tournament={tournament} matches={matches} started={started} entries={entries} />
      )}

      {tab === 'table' && (
        <TableTab
          tournament={tournament}
          matches={matches}
          standings={standingsQuery.data ?? []}
          started={started}
        />
      )}

      {tab === 'stats' && <StatsTab matches={matches} started={started} />}
    </div>
  )
}

/** Who's entered, plus the Start action pre-tournament — the same
 *  information `TournamentEntriesPage` manages, read-only here with a link
 *  across to manage it. */
function JoinedTab({
  tournament,
  entries,
  entrantDisplays,
  started,
  entryCount,
  canStart,
  starting,
  startError,
  onStart,
}: {
  tournament: Tournament
  entries: TournamentEntry[]
  entrantDisplays: Map<string, EntrantDisplay>
  started: boolean
  entryCount: number
  canStart: boolean
  starting: boolean
  startError?: string
  onStart: () => void
}) {
  return (
    <>
      <FormSection
        title={`${tournamentEntryAudienceLabel(tournament.entry_audience)} (${entryCount}${tournament.max_teams ? ` / ${tournament.max_teams}` : ''})`}
        aside={
          <Link to={`/tournaments/${tournament.id}/entries`} className="underline">
            Manage
          </Link>
        }
      >
        <div className={CARD}>
          {entries.length === 0 && (
            <p className="py-2 text-center text-sm text-muted-foreground">No one's joined yet.</p>
          )}
          {entries.map((entry) => {
            const display = entrantDisplays.get(entry.entrant_id)
            const name = display?.name ?? entry.entrant_id
            return (
              <div key={entry.entrant_id} className="flex items-center gap-2.5 border-b py-2.5 last:border-b-0">
                <Avatar name={name} imageUrl={display?.imageUrl} size="md" />
                <span className="flex-1 truncate text-sm font-semibold">{name}</span>
                {tournament.entry_audience === 'players' && entry.squad != null && (
                  <span className="text-xs text-muted-foreground">
                    {tournament.squad_labels[String(entry.squad)]?.name ?? defaultSquadLabel(entry.squad).name}
                  </span>
                )}
              </div>
            )
          })}
        </div>
        {!started && (
          <p className="px-1 text-xs text-muted-foreground">
            Needs at least {tournament.min_teams} to start.
          </p>
        )}
      </FormSection>

      {!started && (
        <>
          {startError && <p className="text-sm text-destructive">{startError}</p>}
          <Button size="lg" disabled={!canStart || starting} onClick={onStart}>
            {starting ? 'Starting…' : 'Start tournament'}
          </Button>
        </>
      )}
    </>
  )
}

/**
 * Each squad's (or, for a team tournament, each team's) roster — same
 * information the match page's Players tab shows per side, but for every
 * entrant in the tournament instead of just the two sides of one match.
 */
function SquadsTab({
  tournament,
  entries,
  entrantDisplays,
}: {
  tournament: Tournament
  entries: TournamentEntry[]
  entrantDisplays: Map<string, EntrantDisplay>
}) {
  if (tournament.entry_audience === 'players') {
    const highestSquad = entries.reduce((max, e) => (e.squad != null ? Math.max(max, e.squad) : max), -1)
    const squadCount = highestSquad + 1
    const unassigned = entries.filter((e) => e.squad == null)

    if (squadCount === 0) {
      return (
        <div className={CARD}>
          <p className="py-2 text-center text-sm text-muted-foreground">No squads yet.</p>
        </div>
      )
    }

    return (
      <div className="flex flex-col gap-3.5">
        {Array.from({ length: squadCount }, (_, i) => {
          const label = tournament.squad_labels[String(i)] ?? defaultSquadLabel(i)
          const members = entries.filter((e) => e.squad === i)
          return (
            <SquadCard
              key={i}
              name={label.name}
              colour={label.colour}
              entries={members}
              entrantDisplays={entrantDisplays}
            />
          )
        })}
        {unassigned.length > 0 && (
          <SquadCard
            name="Unassigned"
            entries={unassigned}
            entrantDisplays={entrantDisplays}
          />
        )}
      </div>
    )
  }

  // Team audience: each entry's `entrant_id` is a team id — show its crest
  // colour and its actual roster, fetched via `GET /teams/{id}/members`.
  if (entries.length === 0) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">No teams have joined yet.</p>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3.5">
      {entries.map((entry) => (
        <TeamSquadCard key={entry.entrant_id} teamId={entry.entrant_id} display={entrantDisplays.get(entry.entrant_id)} />
      ))}
    </div>
  )
}

function SquadCard({
  name,
  colour,
  entries,
  entrantDisplays,
}: {
  name: string
  colour?: string
  entries: TournamentEntry[]
  entrantDisplays: Map<string, EntrantDisplay>
}) {
  return (
    <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
      <div className="flex items-center gap-2.5 border-b px-4 py-3">
        <span
          className="size-3 shrink-0 rounded-sm"
          style={{ background: colour ?? '#94A3B8' }}
          aria-hidden
        />
        <span className="flex-1 truncate font-display text-base font-bold">{name}</span>
        <span className="text-xs text-muted-foreground">
          {entries.length} {entries.length === 1 ? 'player' : 'players'}
        </span>
      </div>
      {entries.length === 0 && <p className="px-4 py-4 text-sm text-muted-foreground">No players yet.</p>}
      {entries.map((entry) => {
        const display = entrantDisplays.get(entry.entrant_id)
        const playerName = display?.name ?? entry.entrant_id
        return (
          <div key={entry.entrant_id} className="flex items-center gap-2.5 px-4 py-2.5">
            <Avatar name={playerName} imageUrl={display?.imageUrl} size="md" />
            <span className="truncate text-sm font-medium">{playerName}</span>
          </div>
        )
      })}
    </section>
  )
}

/** A team entrant's card: fetches the team itself (for its crest) and the
 *  first page of its members (same pagination posture as `TeamPage`'s own
 *  members list — a squad-sized team fits in one page). */
function TeamSquadCard({ teamId, display }: { teamId: string; display: EntrantDisplay | undefined }) {
  const teamQuery = useQuery({
    queryKey: ['team', teamId],
    queryFn: async (): Promise<Team> => {
      const { data, error } = await fetchClient.GET('/teams/{team_id}', {
        params: { path: { team_id: teamId } },
      })
      if (error || !data) throw new Error('Failed to load team')
      return data
    },
  })

  const membersQuery = useInfiniteQuery({
    queryKey: ['team-members', teamId],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) => {
      const { data, error } = await fetchClient.GET('/teams/{team_id}/members', {
        params: { path: { team_id: teamId }, query: { cursor: pageParam, limit: 50 } },
      })
      if (error || !data) throw new Error('Failed to load members')
      return data
    },
    getNextPageParam: (lastPage) => lastPage.next_cursor ?? undefined,
  })

  const team = teamQuery.data
  const name = team?.name ?? display?.name ?? teamId
  const members: TeamMember[] = membersQuery.data?.pages.flatMap((p) => p.items) ?? []

  return (
    <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
      <div className="flex items-center gap-2.5 border-b px-4 py-3">
        <span
          className="size-3 shrink-0 rounded-sm"
          style={{ background: resolveSideColour({ team_id: teamId }) ?? teamCrestColor(teamId) }}
          aria-hidden
        />
        <Avatar name={name} imageUrl={team?.logo?.image_url ?? display?.imageUrl} size="md" />
        <span className="flex-1 truncate font-display text-base font-bold">{name}</span>
        <span className="text-xs text-muted-foreground">
          {members.length} {members.length === 1 ? 'player' : 'players'}
        </span>
      </div>
      {membersQuery.isLoading && <p className="px-4 py-4 text-sm text-muted-foreground">Loading…</p>}
      {!membersQuery.isLoading && members.length === 0 && (
        <p className="px-4 py-4 text-sm text-muted-foreground">No players yet.</p>
      )}
      {members.map((member) => (
        <div key={member.member.id} className="flex items-center gap-2.5 px-4 py-2.5">
          <Avatar
            name={member.member.type === 'External' ? member.member.display_name : member.member.name}
            imageUrl={member.member.type === 'User' ? member.member.avatar_url : undefined}
            size="md"
          />
          <span className="truncate text-sm font-medium">
            {member.member.type === 'External' ? member.member.display_name : member.member.name}
          </span>
        </div>
      ))}
      {membersQuery.hasNextPage && (
        <button
          type="button"
          className="px-4 py-2.5 text-left text-sm font-semibold text-muted-foreground hover:text-foreground"
          onClick={() => membersQuery.fetchNextPage()}
        >
          Load more
        </button>
      )}
    </section>
  )
}

/** Every generated fixture, grouped by round — a manual-generation league
 *  also gets its "Add match" form here. */
function MatchesTab({
  tournament,
  matches,
  started,
  entries,
}: {
  tournament: Tournament
  matches: TournamentMatchSummary[]
  started: boolean
  entries: TournamentEntry[]
}) {
  if (!started) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">
          Matches will appear here once the tournament starts.
        </p>
      </div>
    )
  }

  return (
    <>
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

      {tournament.format === 'league' &&
        (tournament.generation as TournamentGenerationInput | undefined)?.type === 'Manual' && (
          <AddTournamentMatchSection tournament={tournament} entries={entries} />
        )}
    </>
  )
}

/** A league's standings, or a knockout's bracket — the tournament's actual
 *  shape of progress, as opposed to the Matches tab's flat fixture list. */
function TableTab({
  tournament,
  matches,
  standings,
  started,
}: {
  tournament: Tournament
  matches: TournamentMatchSummary[]
  standings: TournamentStandingsRow[]
  started: boolean
}) {
  if (!started) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">
          {tournament.format === 'knockout'
            ? 'The bracket will appear here once the tournament starts.'
            : 'The table will appear here once the tournament starts.'}
        </p>
      </div>
    )
  }

  if (tournament.format === 'league') {
    return <StandingsTable rows={standings} names={entrantNames(matches)} />
  }

  const rounds = groupMatchesByRound(matches)
  return (
    <div className="-mx-4 overflow-x-auto px-4 md:mx-0 md:px-0">
      <div className="flex min-w-max gap-3 pb-2">
        {rounds.map(([round, roundMatches]) => (
          <div key={round} className="flex w-56 shrink-0 flex-col gap-2">
            <h3 className="px-1 text-xs font-bold uppercase text-muted-foreground">
              {roundLabel(round, matches.length)}
            </h3>
            <div className="flex flex-1 flex-col justify-around gap-3">
              {roundMatches.map((match) => (
                <div key={match.id} className={CARD}>
                  <TournamentMatchRow match={match} />
                </div>
              ))}
            </div>
          </div>
        ))}
        {rounds.length === 0 && (
          <p className="py-2 text-center text-sm text-muted-foreground">No matches yet.</p>
        )}
      </div>
    </div>
  )
}

/**
 * Goal/assist leaders aggregated across every one of the tournament's
 * matches — the tournament-wide equivalent of a single match's own "Goals &
 * assists" card (`GoalsAssistsCard`). Fetches each match's full detail (the
 * summary list doesn't carry events) and reuses that same card's
 * `goalContributions` helper per match, then merges the per-match entries
 * (keyed by a match-scoped player id, so they can't be summed directly) onto
 * a cross-match identity: the player's Agon user id when they have one, else
 * their display name — same identity `memberName`/`memberAvatarUrl` already
 * treat as "this person" everywhere else a player is shown outside one
 * match's own roster.
 *
 * Football only for now — the one sport with a ready-made per-match
 * contributions helper to reuse. Cricket's and netball's equivalents
 * (batting/bowling averages, top-scorer tallies) are computed from each
 * match's innings/quarter detail rather than one flat event list, so
 * aggregating them cleanly across a whole tournament is a bigger job — left
 * for a follow-up.
 */
function StatsTab({ matches, started }: { matches: TournamentMatchSummary[]; started: boolean }) {
  const matchQueries = useQueries({
    queries: matches.map((m) => ({
      queryKey: ['match', m.id],
      queryFn: async (): Promise<Match> => {
        const { data, error } = await fetchClient.GET('/matches/{match_id}', {
          params: { path: { match_id: m.id } },
        })
        if (error || !data) throw new Error('Failed to load match')
        return data
      },
    })),
  })

  if (!started) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">
          Player stats will appear here once the tournament starts.
        </p>
      </div>
    )
  }

  const loadedMatches = matchQueries.map((q) => q.data).filter((m): m is Match => !!m)
  if (matchQueries.some((q) => q.isLoading)) {
    return <div className="h-40 animate-pulse rounded-2xl border bg-card" aria-hidden />
  }

  const byIdentity = new Map<string, GoalContributionEntry>()
  for (const match of loadedMatches) {
    const detail = footballEventSourceFromScore(match.confirmed_score?.score ?? match.pending_score?.score)
    if (!detail) continue
    for (const entry of goalContributions(detail.goals, match, detail.players)) {
      const identity = entry.userId ?? entry.name
      const existing = byIdentity.get(identity)
      if (existing) {
        existing.goals += entry.goals
        existing.assists += entry.assists
      } else {
        byIdentity.set(identity, { ...entry })
      }
    }
  }

  const rows = sortGoalContributions([...byIdentity.values()], 'goals')

  if (rows.length === 0) {
    return (
      <div className={CARD}>
        <p className="py-2 text-center text-sm text-muted-foreground">
          No goals or assists recorded yet.
        </p>
      </div>
    )
  }

  const max = Math.max(...rows.map((r) => r.goals + r.assists)) + 1

  return (
    <section className="flex flex-col overflow-hidden rounded-[20px] border bg-card">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <span className="font-display text-base font-bold">Goals &amp; assists</span>
        <div className="flex gap-3 text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-full bg-primary" />
            Goals
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2 rounded-full bg-accent-foreground/60" />
            Assists
          </span>
        </div>
      </div>
      {rows.map((r, i) => (
        <div key={r.key} className="flex items-center gap-3 px-4 py-2.5">
          <span className="w-4 shrink-0 text-xs font-bold text-muted-foreground">{i + 1}</span>
          <Avatar name={r.name} imageUrl={r.avatarUrl} size="md" />
          {r.userId ? (
            <Link to={`/users/${r.userId}`} className="min-w-0 flex-1 truncate text-sm font-semibold hover:underline">
              {r.name}
            </Link>
          ) : (
            <span className="min-w-0 flex-1 truncate text-sm font-semibold">{r.name}</span>
          )}
          <span aria-label={`${r.goals} goals, ${r.assists} assists`} className="flex h-2 w-20 gap-0.5">
            {r.goals > 0 && <span className="rounded bg-primary" style={{ width: `${(r.goals / max) * 100}%` }} />}
            {r.assists > 0 && (
              <span className="rounded bg-accent-foreground/60" style={{ width: `${(r.assists / max) * 100}%` }} />
            )}
          </span>
          <span className="w-5 text-right font-display text-sm font-extrabold">{r.goals}</span>
          <span className="w-5 text-right font-display text-sm font-extrabold text-muted-foreground">{r.assists}</span>
        </div>
      ))}
    </section>
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

  const canSave = name.trim() !== '' && !!startsAt && !!sideA && !!sideB && sideA !== sideB

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
