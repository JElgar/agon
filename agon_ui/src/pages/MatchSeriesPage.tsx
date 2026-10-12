import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import { Button } from '@/components/ui/button'
import { sportIcon } from '@/lib/sports'
import { scheduledDateTime } from '@/lib/datetime'
import { seriesCadenceLabel, seriesEndLabel, seriesStatusLabel } from '@/lib/matchSeries'

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
      const { data, error } = await fetchClient.GET('/match-series/{series_id}/occurrences', {
        params: { path: { series_id: seriesId! } },
      })
      if (error || !data) throw new Error('Failed to load this series’ matches')
      return data
    },
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
                {series.status === 'generating'
                  ? `Generating ${series.occurrences_generated}…`
                  : seriesStatusLabel(series.status)}
              </span>
            </div>
          </div>

          <section className="flex flex-col gap-2.5">
            <h2 className="px-1 font-display text-[17px] font-bold">Matches in this series</h2>
            <div className="flex flex-col gap-2">
              {(occurrencesQuery.data ?? []).map((occurrence) => (
                <button
                  key={occurrence.match_id}
                  type="button"
                  onClick={() => navigate(`/matches/${occurrence.match_id}`)}
                  className="flex items-center gap-3 rounded-2xl border bg-card p-3.5 text-left transition-colors hover:bg-accent/50"
                >
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-bold text-muted-foreground">
                    {occurrence.index + 1}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm font-semibold">
                    {scheduledDateTime(occurrence.starts_at)}
                  </span>
                  <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
                </button>
              ))}
              {occurrencesQuery.isLoading && (
                <div className="h-14 animate-pulse rounded-2xl border bg-card" aria-hidden />
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

