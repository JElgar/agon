import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { Trophy } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { sportIcon, sportLabel } from '@/lib/sports'
import { tournamentFormatLabel, tournamentStatusLabel } from '@/lib/tournaments'

type Tournament = components['schemas']['Tournament']

/**
 * "My tournaments" (`GET /tournaments/mine`): every tournament this user
 * created, entered directly, or entered via one of their teams. Unlike
 * matches, a tournament isn't fed into the feed or search, so this is the
 * only place to find one again without a direct link — reached from the
 * account sheet (`ProfilePage`), next to Teams.
 */
export function MyTournamentsPage() {
  const list = useQuery({
    queryKey: ['my-tournaments'],
    queryFn: async (): Promise<Tournament[]> => {
      const { data, error } = await fetchClient.GET('/tournaments/mine', {})
      if (error || !data) throw new Error('Failed to load tournaments')
      return data.items
    },
  })

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-3 md:max-w-3xl md:gap-5">
      <div className="flex items-center justify-between gap-2">
        <h1 className="font-display text-2xl font-extrabold md:text-3xl">My tournaments</h1>
        <Button shape="pill" asChild>
          <Link to="/tournaments/new">New tournament</Link>
        </Button>
      </div>

      <ListBody list={list} />
    </div>
  )
}

function ListBody({ list }: { list: ReturnType<typeof useQuery<Tournament[]>> }) {
  if (list.isLoading) {
    return (
      <div className="flex flex-col gap-2.5">
        {Array.from({ length: 3 }).map((_, i) => (
          <div key={i} className="h-20 animate-pulse rounded-2xl border bg-card" />
        ))}
      </div>
    )
  }

  if (list.isError) {
    return (
      <div className="py-12 text-center">
        <p className="mb-3 text-sm text-muted-foreground">Couldn't load your tournaments.</p>
        <Button variant="outline" shape="pill" size="sm" onClick={() => list.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const items = list.data ?? []

  if (items.length === 0) {
    return (
      <Card className="flex flex-col items-center gap-2 py-16 text-center">
        <Trophy className="size-8 text-muted-foreground" />
        <p className="text-sm text-muted-foreground">
          You're not in a tournament yet.
        </p>
      </Card>
    )
  }

  return (
    <div className="flex flex-col divide-y overflow-hidden rounded-2xl border bg-card">
      {items.map((tournament) => (
        <TournamentRow key={tournament.id} tournament={tournament} />
      ))}
    </div>
  )
}

function TournamentRow({ tournament }: { tournament: Tournament }) {
  const Icon = sportIcon(tournament.match_type)
  return (
    <Link
      to={`/tournaments/${tournament.id}`}
      className="flex items-center gap-3.5 px-4 py-3.5 hover:bg-muted/50"
    >
      <div className="flex size-11 shrink-0 items-center justify-center rounded-2xl bg-muted">
        <Icon className="size-5 text-foreground/70" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[15px] font-bold">{tournament.name}</span>
        <span className="text-[13px] text-muted-foreground">
          {sportLabel(tournament.match_type)} · {tournamentFormatLabel(tournament.format)}
        </span>
      </div>
      <span className="shrink-0 rounded-full bg-muted px-2.5 py-1 text-xs font-semibold text-foreground/70">
        {tournamentStatusLabel(tournament.status)}
      </span>
    </Link>
  )
}
