/**
 * Client-side preview of a recurring series' occurrence dates — mirrors
 * `agon_core::series` (`occurrence_date`/`count_occurrences_through`) so the
 * "Next up" preview in the create flow shows exactly what the server will
 * generate, without a round trip.
 *
 * Every date here is built from local calendar components (`getFullYear`/
 * `getMonth`/`getDate` and the `new Date(y, m, d, h, min)` constructor)
 * rather than by adding milliseconds to a timestamp — the same local
 * wall-clock arithmetic the server now does, so a preview spanning a DST
 * transition still lands on the right local time instead of drifting by the
 * transition's offset.
 */

export type SeriesFrequency = 'weekly' | 'fortnightly' | 'monthly' | 'yearly'

/** `first` with `days` added to its calendar date, local time-of-day kept
 *  fixed. `Date`'s constructor normalizes an out-of-range day (e.g. day 32),
 *  rolling into the next month as needed. */
function addDaysLocal(first: Date, days: number): Date {
  return new Date(
    first.getFullYear(),
    first.getMonth(),
    first.getDate() + days,
    first.getHours(),
    first.getMinutes(),
    first.getSeconds(),
    first.getMilliseconds(),
  )
}

/**
 * `first`'s ordinal weekday-of-month (the Nth `weekday` of its month),
 * advanced by `monthOffset` months — clamped to the last occurrence of
 * `weekday` in the target month when that ordinal doesn't exist there (e.g.
 * the 5th Friday of a month that only has 4). `monthOffset` is always >= 0
 * here (every caller passes `index` or `index * 12`), so no month-count
 * wraparound below zero to handle.
 */
function nthWeekdayOfMonthOffset(first: Date, weekday: number, monthOffset: number): Date {
  const ordinal = Math.floor((first.getDate() - 1) / 7) + 1
  const totalMonths = first.getMonth() + monthOffset
  const targetYear = first.getFullYear() + Math.floor(totalMonths / 12)
  const targetMonth = totalMonths % 12

  const daysWithWeekday: number[] = []
  let d = new Date(targetYear, targetMonth, 1, first.getHours(), first.getMinutes(), first.getSeconds())
  while (d.getMonth() === targetMonth) {
    if (d.getDay() === weekday) daysWithWeekday.push(d.getDate())
    d = addDaysLocal(d, 1)
  }
  const idx = Math.min(ordinal - 1, daysWithWeekday.length - 1)
  return new Date(
    targetYear,
    targetMonth,
    daysWithWeekday[idx],
    first.getHours(),
    first.getMinutes(),
    first.getSeconds(),
  )
}

/** The occurrence at `index` (0-based; index 0 is `first` itself), advancing
 *  from `first` at `frequency`. `weekday` is `0 = Sunday .. 6 = Saturday`
 *  (`Date.prototype.getDay`'s own convention), used only by monthly/yearly. */
export function occurrenceDate(
  first: Date,
  frequency: SeriesFrequency,
  weekday: number,
  index: number,
): Date {
  switch (frequency) {
    case 'weekly':
      return addDaysLocal(first, 7 * index)
    case 'fortnightly':
      return addDaysLocal(first, 14 * index)
    case 'monthly':
      return nthWeekdayOfMonthOffset(first, weekday, index)
    case 'yearly':
      return nthWeekdayOfMonthOffset(first, weekday, index * 12)
  }
}

/** A pure defensive guard, same reasoning as the server's own
 *  `COUNT_THROUGH_ITERATION_CAP`: a pathological `end` (e.g. the year 9999)
 *  shouldn't spin the preview forever. */
const COUNT_THROUGH_ITERATION_CAP = 10_000

/** How many occurrences of `weekday` at `frequency`, starting at `first`
 *  (inclusive, counted as occurrence 1), fall on or before `end` — compared
 *  by calendar date, not instant. */
export function countOccurrencesThrough(
  first: Date,
  frequency: SeriesFrequency,
  weekday: number,
  end: Date,
): number {
  const endDay = new Date(end.getFullYear(), end.getMonth(), end.getDate()).getTime()
  let count = 0
  while (count <= COUNT_THROUGH_ITERATION_CAP) {
    const date = occurrenceDate(first, frequency, weekday, count)
    const day = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
    if (day > endDay) break
    count++
  }
  return count
}
