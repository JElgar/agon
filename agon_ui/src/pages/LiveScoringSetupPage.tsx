import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useAppendFootballEvent } from '@/hooks/useLiveScore'
import { useMatchDetailedScore } from '@/hooks/useMatchDetailedScore'
import {
  footballDetailFrom,
  loadTrackPrefs,
  periodTime,
  saveTrackPrefs,
  type TrackPrefs,
} from '@/lib/liveScore'

type Match = components['schemas']['Match']

function sideName(match: Match, index: number, fallback: string): string {
  return match.sides[index]?.name?.trim() || fallback
}

/** One row in the "track during the match" list — either a real toggle backed
 *  by a track preference, or a disabled "coming soon" row for event kinds the
 *  backend doesn't record yet (shots/saves, corners/fouls). */
function TrackRow({
  title,
  subtitle,
  checked,
  disabled,
  onChange,
}: {
  title: string
  subtitle: string
  checked: boolean
  disabled?: boolean
  onChange?: (checked: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-b p-4 last:border-b-0">
      <div className="min-w-0">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-xs text-muted-foreground">{subtitle}</p>
      </div>
      <Switch
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
        aria-label={title}
      />
    </div>
  )
}

/**
 * "Set up scoring" — what to track during a live football match. Goals are
 * always recorded; cards and substitutions are real toggles that decide which
 * quick-action buttons `LiveScoringPage` shows, persisted per-device
 * (`localStorage`, see `lib/liveScore`) since they're just a display
 * preference, not something the server needs to know. Shots/saves and
 * corners/fouls are shown but disabled: the live-scoring API
 * (`agon_service::live_score::football`) only models goals, cards, subs and
 * period markers today, so there's nowhere for those events to go yet.
 */
export function LiveScoringSetupPage() {
  const { matchId } = useParams()
  const navigate = useNavigate()

  const query = useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  const [prefs, setPrefs] = useState<TrackPrefs>(() => loadTrackPrefs(matchId ?? ''))

  const setPref = (key: keyof TrackPrefs, value: boolean) => {
    setPrefs((prev) => {
      const next = { ...prev, [key]: value }
      if (matchId) saveTrackPrefs(matchId, next)
      return next
    })
  }

  // Whether the match's live clock has already started (a KickOff period
  // marker already recorded) — determines whether "Start scoring" needs to
  // record one, and whether the button reads "Start" vs "Continue".
  const detailedScore = useMatchDetailedScore(matchId)
  const liveState = footballDetailFrom(detailedScore.data)
  const alreadyKickedOff = !!liveState && !!periodTime(liveState, 'kick_off')
  const appendEvent = useAppendFootballEvent(matchId ?? '')

  const start = useMutation({
    mutationFn: async () => {
      if (!matchId) return
      // The live clock starts now, once — recorded as a real event so every
      // viewer (not just this device) can compute the same running minute.
      // Appending it is also what flips the match from `scheduled` to
      // `in_progress` server-side (see `append_live_events`), which is what
      // makes the live score visible on other viewers' feed/detail screens —
      // no separate status PATCH needed here.
      if (!alreadyKickedOff) {
        await appendEvent.mutateAsync({ kind: 'Period', period: 'kick_off' })
      }
    },
    onSuccess: () => {
      navigate(`/matches/${matchId}/live`, { replace: true })
    },
  })

  if (query.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  if (query.isError || !query.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this match.</p>
        <Button variant="outline" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const match = query.data
  const nameA = sideName(match, 0, 'Side A')
  const nameB = sideName(match, 1, 'Side B')

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center gap-3">
        <Button variant="ghost" size="sm" onClick={() => navigate(`/matches/${matchId}`)}>
          <ChevronLeft className="size-4" /> Back
        </Button>
      </div>

      <div>
        <h1 className="text-lg font-semibold">Set up scoring</h1>
        <p className="text-sm text-muted-foreground">
          {nameA} vs {nameB} · Football
        </p>
      </div>

      <div>
        <p className="mb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Track during the match
        </p>
        <div className="overflow-hidden rounded-xl border bg-card">
          <TrackRow title="Goals" subtitle="Always on" checked disabled />
          <TrackRow
            title="Cards"
            subtitle="Yellow / red"
            checked={prefs.cards}
            onChange={(v) => setPref('cards', v)}
          />
          <TrackRow
            title="Substitutions"
            subtitle=""
            checked={prefs.substitutions}
            onChange={(v) => setPref('substitutions', v)}
          />
          <TrackRow
            title="Shots & saves"
            subtitle="Coming soon"
            checked={false}
            disabled
          />
          <TrackRow title="Corners & fouls" subtitle="Coming soon" checked={false} disabled />
        </div>
      </div>

      <p className="text-center text-xs text-muted-foreground">
        Turn more on anytime — even mid-match
      </p>

      {start.isError && (
        <p className="text-center text-xs text-destructive">
          {(start.error as Error).message}
        </p>
      )}

      <Button size="lg" disabled={start.isPending} onClick={() => start.mutate()}>
        {start.isPending
          ? 'Starting…'
          : alreadyKickedOff
            ? 'Continue scoring'
            : 'Start scoring'}
      </Button>
    </div>
  )
}
