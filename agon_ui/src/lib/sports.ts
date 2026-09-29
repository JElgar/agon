import {
  Volleyball,
  Dumbbell,
  Target,
  CircleDot,
  Circle,
  type LucideIcon,
} from 'lucide-react'
import type { components } from '@/types/api'

export type MatchType = components['schemas']['MatchType']

/** Human label for a sport, for pills and headings. */
const SPORT_LABELS: Record<MatchType, string> = {
  tennis: 'Tennis',
  badminton: 'Badminton',
  squash: 'Squash',
  table_tennis: 'Table Tennis',
  football: 'Football',
  cricket: 'Cricket',
  netball: 'Netball',
  other: 'Other',
}

/**
 * Icon for a sport. lucide has no sport-specific icons for most of these, so we
 * map to the nearest sensible glyph; swap these out if a better icon library is
 * adopted (the mockups used Tabler's sport icons). Centralised here so every
 * component renders the same icon per sport.
 */
const SPORT_ICONS: Record<MatchType, LucideIcon> = {
  tennis: CircleDot,
  badminton: CircleDot,
  squash: CircleDot,
  table_tennis: CircleDot,
  football: Volleyball,
  cricket: Target,
  netball: Target,
  other: Dumbbell,
}

export function sportLabel(type: MatchType): string {
  return SPORT_LABELS[type] ?? SPORT_LABELS.other
}

export function sportIcon(type: MatchType): LucideIcon {
  return SPORT_ICONS[type] ?? Circle
}

/** Emoji for a sport, used by the compact sport pill (feed cards, match detail). */
const SPORT_EMOJI: Record<MatchType, string> = {
  tennis: '🎾',
  badminton: '🏸',
  squash: '🎾',
  table_tennis: '🏓',
  football: '⚽',
  cricket: '🏏',
  netball: '🏐',
  other: '🏅',
}

export function sportEmoji(type: MatchType): string {
  return SPORT_EMOJI[type] ?? SPORT_EMOJI.other
}

/**
 * Pastel icon-badge tint per sport, for the feed card's 40×40 sport-icon
 * badge — card-local accents from the "Agon redesign" canvas
 * (`Tiles.dc.html`), not theme colors, so they live here as a plain map
 * rather than new CSS custom properties. Only the sports with a dedicated
 * icon badge in the redesigned feed card (currently cricket) need an entry;
 * football uses a team-initials avatar instead (see `MatchCard`).
 */
export const SPORT_ICON_TINT: Partial<Record<MatchType, { bg: string; stroke: string }>> = {
  cricket: { bg: '#D3E3F0', stroke: '#123E5B' },
  tennis: { bg: '#E4F0B8', stroke: '#3B5B12' },
  squash: { bg: '#F7D9C6', stroke: '#6B2F12' },
  // No mock covers these two — same lavender/pink pastel treatment as the
  // above, just a fresh pair of tones so all four racket sports read as a
  // family without colliding with tennis/squash's.
  badminton: { bg: '#EDE3F5', stroke: '#4B2E68' },
  table_tennis: { bg: '#FBE0EA', stroke: '#7A1F3D' },
}

/** Racket sports are scored by sets; everything else by a single points total. */
export function isSetsSport(sport: MatchType): boolean {
  return (
    sport === 'tennis' ||
    sport === 'badminton' ||
    sport === 'squash' ||
    sport === 'table_tennis'
  )
}
