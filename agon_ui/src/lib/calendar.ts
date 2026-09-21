/**
 * Build a downloadable `.ics` calendar entry for a match. There's no stored
 * duration or end time on a match (`Match.starts_at` is the only timestamp),
 * so the event length is estimated from the match's format when one is set,
 * falling back to a plain per-sport default — good enough for a calendar
 * block, not meant to be exact.
 */
import type { components } from '@/types/api'
import { cricketFormat, footballFormat, netballFormat } from '@/lib/matchFormat'

type Match = components['schemas']['Match']

const DEFAULT_DURATION_MINUTES = 120

/** Rough estimated length of a match, in minutes, from its format (or the
 *  app default format for its sport) — used only to pick a calendar event's
 *  end time. */
export function estimatedDurationMinutes(match: Match): number {
  if (match.match_type === 'football') {
    const fmt = footballFormat(match.format)
    return fmt.num_halves * fmt.half_length_minutes + 15
  }
  if (match.match_type === 'netball') {
    const fmt = netballFormat(match.format)
    return fmt.num_quarters * fmt.quarter_length_minutes + 15
  }
  if (match.match_type === 'cricket') {
    const fmt = cricketFormat(match.format)
    // ~4 minutes/over/innings is a common casual-cricket rule of thumb.
    return fmt.overs_per_innings ? fmt.overs_per_innings * fmt.innings_per_side * 4 : 180
  }
  return DEFAULT_DURATION_MINUTES
}

/** Escape text for an ICS content line (RFC 5545 §3.3.11). */
function escapeIcsText(text: string): string {
  return text
    .replace(/\\/g, '\\\\')
    .replace(/;/g, '\\;')
    .replace(/,/g, '\\,')
    .replace(/\n/g, '\\n')
}

/** Format a `Date` as a UTC ICS timestamp, e.g. `20260920T143000Z`. */
function formatIcsDate(date: Date): string {
  return date.toISOString().replace(/[-:]/g, '').split('.')[0] + 'Z'
}

/**
 * The full `.ics` file content for adding a match to a personal calendar.
 * `title`/`description` are the caller's choice (e.g. the match name and
 * "You vs Opposition"), so this stays independent of how the detail page
 * labels sides.
 */
export function buildMatchIcs(
  match: Match,
  { title, description }: { title: string; description: string },
): string {
  const start = new Date(match.starts_at)
  const end = new Date(start.getTime() + estimatedDurationMinutes(match) * 60_000)
  const location = match.location

  const lines = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//Agon//Match Calendar//EN',
    'CALSCALE:GREGORIAN',
    'BEGIN:VEVENT',
    `UID:match-${match.id}@agon`,
    `DTSTAMP:${formatIcsDate(new Date())}`,
    `DTSTART:${formatIcsDate(start)}`,
    `DTEND:${formatIcsDate(end)}`,
    `SUMMARY:${escapeIcsText(title)}`,
    `DESCRIPTION:${escapeIcsText(description)}`,
  ]
  if (location) {
    lines.push(`LOCATION:${escapeIcsText(location.text)}`)
    if (location.latitude != null && location.longitude != null) {
      lines.push(`GEO:${location.latitude};${location.longitude}`)
    }
  }
  lines.push('END:VEVENT', 'END:VCALENDAR')
  return lines.join('\r\n')
}

/** Trigger a browser download of the match's `.ics` file. Most desktop
 *  browsers save it; mobile browsers typically open it straight into the
 *  device's own "add to calendar" flow. */
export function downloadMatchIcs(
  match: Match,
  labels: { title: string; description: string },
): void {
  const ics = buildMatchIcs(match, labels)
  const blob = new Blob([ics], { type: 'text/calendar;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `${match.id}.ics`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  URL.revokeObjectURL(url)
}
