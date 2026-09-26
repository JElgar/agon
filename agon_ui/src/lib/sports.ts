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

/**
 * Icon-badge tint per sport, for the profile "Your sports" rows — matches the
 * "Agon redesign" canvas's Profile board, which only designs football/cricket;
 * every other sport falls back to the shared accent tint rather than
 * inventing an un-designed color.
 */
const SPORT_TINTS: Partial<Record<MatchType, { bg: string; fg: string }>> = {
  football: { bg: '#DDE5FB', fg: '#1E3FA8' },
  cricket: { bg: '#D3E3F0', fg: '#123E5B' },
}

export function sportTint(type: MatchType): { bg: string; fg: string } {
  return SPORT_TINTS[type] ?? { bg: 'var(--accent)', fg: 'var(--accent-foreground)' }
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

/** Racket sports are scored by sets; everything else by a single points total. */
export function isSetsSport(sport: MatchType): boolean {
  return (
    sport === 'tennis' ||
    sport === 'badminton' ||
    sport === 'squash' ||
    sport === 'table_tennis'
  )
}
