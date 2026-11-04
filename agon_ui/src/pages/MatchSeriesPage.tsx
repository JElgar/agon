import { useQueries, useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { sportIcon } from '@/lib/sports'
import { seriesCadenceLabel, seriesEndLabel, seriesGeneratingLabel, seriesStatusLabel } from '@/lib/matchSeries'
import { MatchCard } from '@/components/agon/MatchCard'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type Match = components['schemas']['Match']

/** A series' cadence hasn't finished generating occurrences while its
 *  `status` is `generating` — poll until it flips to `active`. */
const GENERATING_POLL_MS = 3000

/**
 * A recurring match series: its cadence plus every occurrence it's
 * generated so far, each linking to its own match page.
 */
export function MatchSeriesPage() {
  const { seriesId } = useParams<{ seriesId: string }>()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const seriesQuery = useQuery({
    queryKey: ['match-series', seriesId],
    enabled: !!seriesId,
    refetchInterval: (query) => (query.state.data?.status === 'generating' ? GENERATING_POLL_MS : false),
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/match-series/{series_id}', {
        params: { path: { series_id: seriesId! } },
      })
      if (error || !data) throw new Error('Failed to load this series')
      return data
    },
  })

  const occurrencesQuery = useQuery({
    queryKey: ['match-series', seriesId, 'occurrences'],
    enabled: !!seriesId,
    refetchInterval: () => (seriesQuery.data?.status === 'generating' ? GENERATING_POLL_MS : false),
    queryFn: async () => {
      const items: components['schemas']['MatchSeriesOccurrence'][] = []
      let cursor: string | undefined
      do {
        const { data, error } = await fetchClient.GET('/match-series/{series_id}/occurrences', {
          params: { path: { series_id: seriesId! }, query: { cursor, limit: 50 } },
        })
        if (error || !data) throw new Error('Failed to load this series’ matches')
        items.push(...data.items)
        cursor = data.next_cursor
      } while (cursor)
      return items
    },
  })

  const occurrences = occurrencesQuery.data ?? []

  // Each occurrence only carries its index/start time/match id — render the
  // same `MatchCard` the recent-matches list uses by fetching each
  // occurrence's full match. Shares a cache entry (and refetch) with
  // `MatchDetailPage`, so opening a match from here then navigating back
  // doesn't refetch it.
  const matchQueries = useQueries({
    queries: occurrences.map((occurrence) => ({
      queryKey: ['match', occurrence.match_id],
      queryFn: async (): Promise<Match> => {
        const { data, error } = await fetchClient.GET('/matches/{match_id}', {
          params: { path: { match_id: occurrence.match_id } },
        })
        if (error || !data) throw new Error('Failed to load match')
        return data
      },
    })),
  })

  const series = seriesQuery.data

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="icon" className="-ml-2" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-2xl">Recurring match</h1>
      </div>

      {seriesQuery.isLoading && (
        <div className="h-28 animate-pulse rounded-2xl border bg-card" aria-hidden />
      )}

      {seriesQuery.isError && (
        <div className="py-12 text-center">
          <p className="mb-3 text-sm text-muted-foreground">Couldn't load this series.</p>
          <Button variant="outline" size="sm" onClick={() => seriesQuery.refetch()}>
            Retry
          </Button>
        </div>
      )}

      {series && (
        <>
          <div className="flex flex-col gap-2 rounded-2xl border bg-card p-4">
            <div className="flex items-center gap-2.5">
              <SeriesSportIcon type={series.match_type} />
              <div className="flex min-w-0 flex-col">
                <span className="truncate font-display text-lg font-extrabold">{series.name}</span>
                <span className="text-[13px] text-muted-foreground">
                  {seriesCadenceLabel(series.frequency, series.weekday)} · {seriesEndLabel(series.ends)}
                </span>
              </div>
              <span
                className="ml-auto inline-flex h-6 shrink-0 items-center rounded-full bg-accent px-2.5 text-[11px] font-bold text-accent-foreground"
                aria-label={`Series status: ${seriesStatusLabel(series.status)}`}
              >
                {series.status === 'generating' ? seriesGeneratingLabel(series) : seriesStatusLabel(series.status)}
              </span>
            </div>
          </div>

          <section className="flex flex-col gap-2.5">
            <h2 className="px-1 font-display text-[17px] font-bold">Matches in this series</h2>
            <div className="flex flex-col gap-3">
              {occurrences.map((occurrence, i) => {
                const matchQuery = matchQueries[i]
                if (matchQuery.isLoading) {
                  return (
                    <div
                      key={occurrence.match_id}
                      className="h-28 animate-pulse rounded-2xl border bg-card"
                      aria-hidden
                    />
                  )
                }
                if (matchQuery.isError || !matchQuery.data) {
                  return (
                    <div
                      key={occurrence.match_id}
                      className="rounded-2xl border bg-card p-3.5 text-sm text-muted-foreground"
                    >
                      Couldn't load match {occurrence.index + 1}.
                    </div>
                  )
                }
                return (
                  <MatchCard
                    key={occurrence.match_id}
                    match={matchQuery.data}
                    currentUserId={currentUserId}
                    onOpen={() => navigate(`/matches/${occurrence.match_id}`)}
                  />
                )
              })}
              {occurrencesQuery.isLoading && (
                <div className="h-28 animate-pulse rounded-2xl border bg-card" aria-hidden />
              )}
            </div>
            {series.status === 'generating' && (
              <p className="px-1 text-xs text-muted-foreground">
                More matches are still being created — this'll update as they're added.
              </p>
            )}
          </section>
        </>
      )}
    </div>
  )
}

function SeriesSportIcon({ type }: { type: string }) {
  const Icon = sportIcon(type as Parameters<typeof sportIcon>[0])
  return (
    <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-accent text-accent-foreground">
      <Icon className="size-5" />
    </span>
  )
}

