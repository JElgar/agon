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
 * comment timestamps, where a compact marker reads better than a full date.
 */
export function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  if (Number.isNaN(then)) return ''
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000))
  if (seconds < 60) return 'now'
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return new Date(iso).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  })
}
