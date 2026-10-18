import type { components } from '@/types/api'

type MatchSeries = components['schemas']['MatchSeries']

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
