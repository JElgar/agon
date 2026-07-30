import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type LiveScoreSnapshot = components['schemas']['LiveScoreSnapshot']
type LiveEvent = components['schemas']['LiveEvent']
type NewLiveEventInput = components['schemas']['NewLiveEventInput']
type FootballLiveEvent = components['schemas']['FootballLiveEvent']
type CricketLiveEvent = components['schemas']['CricketLiveEvent']

export function liveScoreQueryKey(matchId: string | undefined) {
  return ['live', matchId] as const
}

/**
 * A match's derived live-scoring snapshot. `null` (not `undefined`) means the
 * match has no live events recorded yet — a normal state (scoring hasn't
 * started), distinct from "still loading" or "failed to load".
 */
export function useLiveScore(
  matchId: string | undefined,
  options?: { enabled?: boolean; refetchInterval?: number | false },
) {
  return useQuery({
    queryKey: liveScoreQueryKey(matchId),
    enabled: !!matchId && (options?.enabled ?? true),
    refetchInterval: options?.refetchInterval,
    queryFn: async (): Promise<LiveScoreSnapshot | null> => {
      const { data, response } = await fetchClient.GET('/matches/{match_id}/live', {
        params: { path: { match_id: matchId! } },
      })
      if (response.status === 404) return null
      if (!data) throw new Error('Failed to load live score')
      return data
    },
  })
}

/**
 * A match's raw live-scoring event log, in append order — unlike
 * `useLiveScore`'s derived snapshot, this has no size ceiling (one DynamoDB
 * item per event) and stays fully readable regardless of match length or
 * status, so a completed match's full run-progression graph reads deliveries
 * from here (see `inningsDeliveriesFromEvents`) rather than the snapshot,
 * which only keeps the currently (or most recently) open innings' ball log.
 */
export function useLiveEvents(matchId: string | undefined, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['live-events', matchId],
    enabled: !!matchId && (options?.enabled ?? true),
    queryFn: async (): Promise<LiveEvent[]> => {
      const { data, response } = await fetchClient.GET('/matches/{match_id}/live/events', {
        params: { path: { match_id: matchId! } },
      })
      if (response.status === 404) return []
      if (!data) throw new Error('Failed to load live events')
      return data
    },
  })
}

/**
 * Appends one sport-tagged live event to a match's log. Reads
 * `expected_last_seq` off the current cached snapshot (0 if scoring hasn't
 * started yet) so the server can detect a lost update; on success the
 * returned snapshot replaces the cache directly (cheaper and more current
 * than invalidating + refetching). Shared by the per-sport wrappers below —
 * each just tags its event with the right `sport` discriminator.
 *
 * The server flips a still-`scheduled` match to `in_progress` the first time
 * any live event is recorded, so every append also invalidates the match and
 * feed queries — that's the only signal the scorer's own client has that the
 * status (and therefore other viewers' "Live" gate) may have just changed.
 */
function useAppendLiveEvent<T extends { kind: string }>(
  matchId: string,
  sport: NewLiveEventInput['event']['sport'],
) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (event: T) => {
      const key = liveScoreQueryKey(matchId)
      const current = queryClient.getQueryData<LiveScoreSnapshot | null>(key)
      const input: NewLiveEventInput = {
        occurred_at: new Date().toISOString(),
        event: { sport, ...event } as NewLiveEventInput['event'],
      }
      const { data, error } = await fetchClient.POST('/matches/{match_id}/live/events', {
        params: { path: { match_id: matchId } },
        body: {
          expected_last_seq: current?.last_seq ?? 0,
          events: [input],
        },
      })
      if (error || !data) throw new Error('Failed to record event')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(liveScoreQueryKey(matchId), data)
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      queryClient.invalidateQueries({ queryKey: ['feed'] })
    },
  })
}

export function useAppendFootballEvent(matchId: string) {
  return useAppendLiveEvent<FootballLiveEvent>(matchId, 'Football')
}

export function useAppendCricketEvent(matchId: string) {
  return useAppendLiveEvent<CricketLiveEvent>(matchId, 'Cricket')
}
