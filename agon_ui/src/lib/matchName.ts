import { sportLabel, type MatchType } from './sports'

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

function partOfDay(hour: number): string {
  if (hour < 12) return 'morning'
  if (hour < 17) return 'afternoon'
  if (hour < 19) return 'evening'
  return 'night'
}

/** A default match name from its sport and local start time, e.g.
 *  "Monday night football". `startsAt` is a datetime-local
 *  "YYYY-MM-DDTHH:mm" string; `undefined` when it doesn't parse. */
export function suggestMatchName(sport: MatchType | null, startsAt: string): string | undefined {
  if (!sport) return undefined
  const d = new Date(startsAt)
  if (Number.isNaN(d.getTime())) return undefined
  const what = sport === 'other' ? 'match' : sportLabel(sport).toLowerCase()
  return `${WEEKDAYS[d.getDay()]} ${partOfDay(d.getHours())} ${what}`
}
