import type { components } from '@/types/api'

export type MatchFormat = components['schemas']['MatchFormat']
export type FootballFormat = components['schemas']['FootballFormat']
export type CricketFormat = components['schemas']['CricketFormat']

/**
 * App-side defaults for an unconfigured match — the backend stores `format`
 * as `None` until someone sets one (see `agon_service::match_format`), so a
 * default change here reaches every match that never customized it, rather
 * than being baked in permanently at creation time.
 */
export const DEFAULT_FOOTBALL_FORMAT: FootballFormat = {
  half_length_minutes: 45,
  num_halves: 2,
  extra_time: false,
  extra_time_half_length_minutes: undefined,
  penalties: false,
}

/** T20-style defaults — the most common casual/club format. */
export const DEFAULT_CRICKET_FORMAT: CricketFormat = {
  overs_per_innings: 20,
  innings_per_side: 1,
  balls_per_over: 6,
  no_ball_penalty_runs: 1,
  wide_penalty_runs: 1,
  wide_is_extra_ball: true,
  no_ball_is_extra_ball: true,
  free_hit_after_no_ball: true,
}

// `Match.format`'s generated type erases the union discriminant (same quirk
// as `Invitation.kind` in `lib/members.ts` — openapi-typescript widens a
// union nested inside an object to `Omit<Union, "sport"> & unknown`, so
// `.sport` doesn't narrow). Accept the loose field shape and cast back to
// the real union, same fix as there.

/** The football format to use — the match's own if set for football, else
 *  the app default. Never returns a cricket format by mistake. */
export function footballFormat(format: unknown): FootballFormat {
  const fmt = format as MatchFormat | null | undefined
  if (fmt && fmt.sport === 'Football') return fmt
  return DEFAULT_FOOTBALL_FORMAT
}

/** The cricket format to use — the match's own if set for cricket, else the
 *  app default. */
export function cricketFormat(format: unknown): CricketFormat {
  const fmt = format as MatchFormat | null | undefined
  if (fmt && fmt.sport === 'Cricket') return fmt
  return DEFAULT_CRICKET_FORMAT
}

/** "20 overs" / "Unlimited overs" for a cricket format summary. */
export function oversLimitLabel(fmt: CricketFormat): string {
  return fmt.overs_per_innings ? `${fmt.overs_per_innings} overs` : 'Unlimited overs'
}
