/**
 * Helpers for the native `<input type="datetime-local">` control, which speaks
 * only local wall-clock `YYYY-MM-DDTHH:mm` (no timezone, no seconds). Shared by
 * the create-match and edit-match flows.
 */

/**
 * Format a `Date` as the local wall-clock string the control expects. ISO
 * strings with a `Z` won't populate a datetime-local input, so we build it from
 * the local components.
 */
export function toDateTimeLocal(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}`
  )
}

/** The datetime-local value for an existing UTC ISO instant (e.g. a match's
 *  `starts_at`), rendered in the viewer's local time. */
export function isoToDateTimeLocal(iso: string): string {
  return toDateTimeLocal(new Date(iso))
}

/**
 * A short, human relative time for an ISO instant (e.g. "3m", "2h", "5d"),
 * falling back to a localized date for anything older than a week. Used for
 * comment timestamps (always in the past) and a match's `starts_at` (which
 * can just as easily be ahead of now, for a still-upcoming scheduled match —
 * that case reads as "in 3d" rather than silently clamping to "now").
 */
export function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  if (Number.isNaN(then)) return ''
  const diff = Math.round((Date.now() - then) / 1000) // positive = past, negative = future
  const future = diff < 0
  const seconds = Math.abs(diff)
  if (seconds < 60) return 'now'
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return future ? `in ${minutes}m` : `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return future ? `in ${hours}h` : `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return future ? `in ${days}d` : `${days}d`
  return new Date(iso).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  })
}

/**
 * The full scheduled date + time for a match's `starts_at`, e.g. "Sat, 12
 * Sep · 3:00 PM" (the year is added only when it isn't the current one).
 * Unlike `relativeTime`, this doesn't decay with elapsed time — it's the one
 * place a match's actual kick-off time is spelled out in full, on the detail
 * page and (space permitting) the feed/profile card.
 */
export function scheduledDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  const now = new Date()
  return date.toLocaleString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: date.getFullYear() === now.getFullYear() ? undefined : 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

/** Whether two dates fall on the same local calendar day. */
function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  )
}

/**
 * A day-grouping label for an ISO instant: "Today" / "Yesterday", else a
 * localized date (with the year only when it isn't the current one). Used to
 * group the feed into date sections.
 */
export function dayLabel(iso: string): string {
  const date = new Date(iso)
  const now = new Date()
  if (isSameDay(date, now)) return 'Today'

  const yesterday = new Date(now)
  yesterday.setDate(now.getDate() - 1)
  if (isSameDay(date, yesterday)) return 'Yesterday'

  return date.toLocaleDateString(undefined, {
    month: 'long',
    day: 'numeric',
    year: date.getFullYear() === now.getFullYear() ? undefined : 'numeric',
  })
}
