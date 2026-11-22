import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft, ChevronRight, MoreHorizontal, Play, Square } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { sportIcon } from '@/lib/sports'
import { scheduledDateTime } from '@/lib/datetime'
import { seriesCadenceLabel, seriesEndLabel, seriesGeneratingLabel, seriesStatusLabel } from '@/lib/matchSeries'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'

type SeriesAction = 'stop' | 'resume' | 'end'

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

  const series = seriesQuery.data
  const currentUserId = useCurrentUserId()
  const canManageSeries =
    !!series &&
    !!currentUserId &&
    (series.created_by_user_id === currentUserId || series.admin_user_ids.includes(currentUserId))

  const queryClient = useQueryClient()
  const [pendingAction, setPendingAction] = useState<SeriesAction | null>(null)
  const updateStatus = useMutation({
    mutationFn: async (status: components['schemas']['MatchSeriesStatus']) => {
      const { error } = await fetchClient.PATCH('/match-series/{series_id}', {
        params: { path: { series_id: seriesId! } },
        body: { status },
      })
      if (error) throw new Error('Failed to update this series')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['match-series', seriesId] })
      queryClient.invalidateQueries({ queryKey: ['match-series', seriesId, 'occurrences'] })
      setPendingAction(null)
    },
  })

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="icon" className="-ml-2" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-2xl">Recurring match</h1>
        {series && canManageSeries && series.status !== 'ended' && (
          <SeriesMenu
            isStopped={series.status === 'paused'}
            onSelect={(action) => setPendingAction(action)}
          />
        )}
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

      <SeriesActionDialog
        action={pendingAction}
        pending={updateStatus.isPending}
        isError={updateStatus.isError}
        onOpenChange={(open) => !open && setPendingAction(null)}
        onConfirm={() => {
          if (pendingAction === 'stop') updateStatus.mutate('paused')
          else if (pendingAction === 'resume') updateStatus.mutate('active')
          else if (pendingAction === 'end') updateStatus.mutate('ended')
        }}
      />
    </div>
  )
}

/**
 * The series' header overflow menu — shown to the series' owner or an admin
 * (`series.admin_user_ids`), always in the same top-right spot regardless of
 * status. Each item just requests a `SeriesAction`; `SeriesActionDialog`
 * owns the actual confirm + `PATCH /match-series/:id` call.
 */
function SeriesMenu({
  isStopped,
  onSelect,
}: {
  isStopped: boolean
  onSelect: (action: SeriesAction) => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="ml-auto" aria-label="Series options">
          <MoreHorizontal className="size-5" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        {isStopped ? (
          <DropdownMenuItem onSelect={() => onSelect('resume')}>
            <Play className="size-4" /> Resume series
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem onSelect={() => onSelect('stop')}>
            <Square className="size-4" /> Stop series
          </DropdownMenuItem>
        )}
        <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => onSelect('end')}>
          End series
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

const ACTION_COPY: Record<SeriesAction, { title: string; description: string; confirmLabel: string; pendingLabel: string; destructive: boolean }> = {
  stop: {
    title: 'Stop this series?',
    description: "No new matches will be created until you resume it. Matches already scheduled aren't affected, and you can resume at any time.",
    confirmLabel: 'Stop series',
    pendingLabel: 'Stopping…',
    destructive: false,
  },
  resume: {
    title: 'Resume this series?',
    description: 'New matches will start being created again on the usual schedule.',
    confirmLabel: 'Resume series',
    pendingLabel: 'Resuming…',
    destructive: false,
  },
  end: {
    title: 'End this series?',
    description:
      "No more matches will be created, and any already-scheduled ones that haven't started yet will be cancelled. This can't be undone.",
    confirmLabel: 'Yes, end it',
    pendingLabel: 'Ending…',
    destructive: true,
  },
}

/** One shared confirm dialog for all three series actions — which action is
 *  open (if any) is owned by the caller, so there's only ever one dialog in
 *  the tree regardless of which menu item triggered it. */
function SeriesActionDialog({
  action,
  pending,
  isError,
  onOpenChange,
  onConfirm,
}: {
  action: SeriesAction | null
  pending: boolean
  isError: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
}) {
  const copy = action ? ACTION_COPY[action] : null
  return (
    <Dialog open={!!action} onOpenChange={(open) => !pending && onOpenChange(open)}>
      <DialogContent className="max-w-sm">
        {copy && (
          <>
            <DialogHeader>
              <DialogTitle>{copy.title}</DialogTitle>
              <DialogDescription>{copy.description}</DialogDescription>
            </DialogHeader>
            {isError && <p className="text-sm text-destructive">Something went wrong. Please try again.</p>}
            <DialogFooter>
              <Button variant="outline" disabled={pending} onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button variant={copy.destructive ? 'destructive' : 'default'} disabled={pending} onClick={onConfirm}>
                {pending ? copy.pendingLabel : copy.confirmLabel}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
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
