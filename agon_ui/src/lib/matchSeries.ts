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

/** How a series' `ends` reads out, e.g. "10 matches" or "until 12 Dec 2026". */
export function seriesEndLabel(ends: MatchSeries['ends']): string {
  if (ends.type === 'AfterN') return `${ends.count} ${ends.count === 1 ? 'match' : 'matches'}`
  const d = new Date(ends.date)
  return Number.isNaN(d.getTime())
    ? `until ${ends.date}`
    : `until ${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}`
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
