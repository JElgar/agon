import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type MatchSeries = components['schemas']['MatchSeries']
type MatchSeriesOccurrence = components['schemas']['MatchSeriesOccurrence']

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

const FREQUENCY_LABELS: Record<string, string> = {
  weekly: 'Weekly',
  fortnightly: 'Fortnightly',
  monthly: 'Monthly',
  yearly: 'Yearly',
}

/** "Weekly on Mondays" — a series' cadence, for its header and the repeat
 *  indicator on a generated occurrence's own match page. */
export function seriesCadenceLabel(frequency: string, weekday: number): string {
  const freq = FREQUENCY_LABELS[frequency] ?? frequency
  const day = WEEKDAYS[weekday] ?? ''
  return day ? `${freq} on ${day}s` : freq
}

/** How a series' `ends` reads out, e.g. "10 matches", "until 12 Dec 2026" or
 *  "no end date" for an indefinite series. */
export function seriesEndLabel(ends: MatchSeries['ends']): string {
  if (ends.type === 'Never') return 'No end date'
  if (ends.type === 'AfterN') return `${ends.count} ${ends.count === 1 ? 'match' : 'matches'}`
  const d = new Date(ends.date)
  return Number.isNaN(d.getTime())
    ? `until ${ends.date}`
    : `until ${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}`
}

/** The status chip's text while a series is still generating its
 *  occurrences, e.g. "Generating 3 of 10" ("Generating 3…" when the series
 *  ends on a date, so the total isn't known client-side). */
export function seriesGeneratingLabel(series: MatchSeries): string {
  const so_far = series.occurrences_generated
  return series.ends.type === 'AfterN' ? `Generating ${so_far} of ${series.ends.count}` : `Generating ${so_far}…`
}

export function seriesStatusLabel(status: MatchSeries['status']): string {
  switch (status) {
    case 'generating':
      return 'Generating'
    case 'active':
      return 'Active'
    case 'paused':
      return 'Paused'
    case 'ended':
      return 'Ended'
  }
}

/** Every occurrence a series has generated so far, oldest first — walks
 *  `GET /match-series/{series_id}/occurrences` to the end. Used by the "edit
 *  this and future matches" flow to find which other matches to apply the
 *  same edit to. */
export async function listAllSeriesOccurrences(seriesId: string): Promise<MatchSeriesOccurrence[]> {
  const items: MatchSeriesOccurrence[] = []
  let cursor: string | undefined
  do {
    const { data, error } = await fetchClient.GET('/match-series/{series_id}/occurrences', {
      params: { path: { series_id: seriesId }, query: { cursor, limit: 50 } },
    })
    if (error || !data) throw new Error("Failed to load this series' matches")
    items.push(...data.items)
    cursor = data.next_cursor
  } while (cursor)
  return items
}

/** This series' occurrences strictly after `afterIso` (an occurrence's own
 *  `starts_at`), oldest first — the "future matches" an "apply to the whole
 *  series" edit reaches, starting from (but not including) the one being
 *  edited. ISO instants from the same API compare correctly as plain
 *  strings. */
export async function listFutureSeriesOccurrences(
  seriesId: string,
  afterIso: string,
): Promise<MatchSeriesOccurrence[]> {
  const all = await listAllSeriesOccurrences(seriesId)
  return all.filter((o) => o.starts_at > afterIso)
}
