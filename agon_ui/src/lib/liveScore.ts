import type { components } from '@/types/api'
import { memberName } from './members'

export type FootballEventKind = components['schemas']['FootballEventKind']
export type FootballEvent = components['schemas']['FootballEvent']
export type FootballPeriod = components['schemas']['FootballPeriod']
export type FootballLiveState = components['schemas']['FootballLiveState']
type LiveScoreSnapshot = components['schemas']['LiveScoreSnapshot']
type Match = components['schemas']['Match']

/** Narrows a live-score snapshot to its football state, or `null` when there's
 *  no snapshot yet or it's for a different sport. */
export function footballLiveState(
  snapshot: LiveScoreSnapshot | null | undefined,
): FootballLiveState | null {
  if (!snapshot || snapshot.state.sport !== 'Football') return null
  return snapshot.state
}

/** Label + emoji for each football live-event kind, for the event log/ticker. */
const EVENT_LABEL: Record<FootballEventKind, string> = {
  goal: 'Goal',
  own_goal: 'Own goal',
  penalty: 'Penalty',
  yellow_card: 'Yellow card',
  red_card: 'Red card',
  substitution: 'Sub',
}

const EVENT_EMOJI: Record<FootballEventKind, string> = {
  goal: '⚽',
  own_goal: '⚽',
  penalty: '⚽',
  yellow_card: '🟨',
  red_card: '🟥',
  substitution: '🔁',
}

export function eventLabel(kind: FootballEventKind): string {
  return EVENT_LABEL[kind]
}

export function eventEmoji(kind: FootballEventKind): string {
  return EVENT_EMOJI[kind]
}

/** "63'" for a recorded minute, else a blank string. */
export function minuteLabel(minute: number | undefined): string {
  return minute === undefined ? '' : `${minute}'`
}

const PERIOD_LABEL: Record<FootballPeriod, string> = {
  kick_off: 'Kick-off',
  half_time: 'Half-time',
  second_half_kick_off: 'Second-half kick-off',
  full_time: 'Full-time',
  extra_time_half_time: 'Extra time: half-time',
  extra_time_full_time: 'Extra time: full-time',
  penalties_complete: 'Penalties complete',
}

export function periodLabel(period: FootballPeriod): string {
  return PERIOD_LABEL[period]
}

/** The most recent events first, capped to `limit` — for a mini-ticker. */
export function recentEvents(state: FootballLiveState, limit: number): FootballEvent[] {
  return [...state.events].reverse().slice(0, limit)
}

/** Side display name for an event, falling back to a neutral label. */
function sideNameFor(match: Pick<Match, 'sides'>, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
}

/** Player display name for a member id, if it's on the roster. */
function playerNameFor(match: Pick<Match, 'players'>, playerId: string | undefined): string | null {
  if (!playerId) return null
  const player = match.players.find((p) => p.member.id === playerId)
  return player ? memberName(player.member) : null
}

/** One-line human description of a football event, e.g. "Goal — J. Alvarez
 *  (Riverside)" or "Sub — Moreno on for Khan (Oak Park)" — used by the event
 *  log and mini-ticker. */
export function describeEvent(
  event: FootballEvent,
  match: Pick<Match, 'sides' | 'players'>,
): string {
  const side = sideNameFor(match, event.side_id)
  const scorer = playerNameFor(match, event.player_id)

  switch (event.kind) {
    case 'goal':
    case 'penalty':
      return scorer ? `${eventLabel(event.kind)} — ${scorer} (${side})` : `${eventLabel(event.kind)} (${side})`
    case 'own_goal':
      return `Own goal — ${side}`
    case 'yellow_card':
    case 'red_card':
      return scorer
        ? `${eventLabel(event.kind)} — ${scorer} (${side})`
        : `${eventLabel(event.kind)} — ${side}`
    case 'substitution': {
      const out = playerNameFor(match, event.substituted_player_id)
      if (scorer && out) return `Sub — ${scorer} on for ${out} (${side})`
      return `Substitution (${side})`
    }
  }
}

// ---------------------------------------------------------------------------
// Track preferences — which quick actions the scorer wants on the live
// scoring screen. Client-only: the backend's football live-event vocabulary
// only covers goals/cards/subs/period markers (see
// `agon_service::live_score::football`), so "shots & saves" and "corners &
// fouls" from the setup mockup have nowhere to be recorded yet and are shown
// as disabled "coming soon" toggles rather than persisted here.
// ---------------------------------------------------------------------------

export interface TrackPrefs {
  cards: boolean
  substitutions: boolean
}

const DEFAULT_PREFS: TrackPrefs = { cards: true, substitutions: true }

function prefsKey(matchId: string): string {
  return `agon:live-track-prefs:${matchId}`
}

export function loadTrackPrefs(matchId: string): TrackPrefs {
  try {
    const raw = localStorage.getItem(prefsKey(matchId))
    if (!raw) return { ...DEFAULT_PREFS }
    const parsed = JSON.parse(raw)
    return {
      cards: typeof parsed.cards === 'boolean' ? parsed.cards : DEFAULT_PREFS.cards,
      substitutions:
        typeof parsed.substitutions === 'boolean'
          ? parsed.substitutions
          : DEFAULT_PREFS.substitutions,
    }
  } catch {
    return { ...DEFAULT_PREFS }
  }
}

export function saveTrackPrefs(matchId: string, prefs: TrackPrefs): void {
  try {
    localStorage.setItem(prefsKey(matchId), JSON.stringify(prefs))
  } catch {
    // Best-effort — a private-browsing/full-storage failure just means the
    // toggles reset to defaults next visit, not worth surfacing.
  }
}

// ---------------------------------------------------------------------------
// Match clock — derived entirely from `FootballLiveState`'s phase-boundary
// timestamps (`kickoff_at`/`half_time_at`/`second_half_kickoff_at`/
// `full_time_at`), each the `occurred_at` of the period marker that recorded
// it server-side (see `agon_service::live_score::football::derive_state`).
// Because it's computed from shared server data rather than a per-device
// stopwatch, the scorer's own screen and every other viewer (feed card, match
// detail) tick the exact same clock.
// ---------------------------------------------------------------------------

export type ClockPhase =
  | 'not_started'
  | 'first_half'
  | 'half_time'
  | 'second_half'
  | 'full_time'
  /** An extra-time/penalties marker was recorded — those phases aren't
   *  clocked yet (see `FootballPeriod`), so just the marker itself is shown. */
  | 'other'

/** Which phase the match is in right now, purely from recorded timestamps. */
export function phaseFromState(state: FootballLiveState): ClockPhase {
  if (state.full_time_at) return 'full_time'
  if (state.second_half_kickoff_at) return 'second_half'
  if (state.half_time_at) return 'half_time'
  if (state.kickoff_at) return 'first_half'
  if (state.period) return 'other'
  return 'not_started'
}

function minutesBetween(from: string, to: Date | string): number {
  return Math.max(0, Math.floor((new Date(to).getTime() - new Date(from).getTime()) / 60_000))
}

/**
 * The current match minute, e.g. for the live clock display or to prefill an
 * event's minute field (still overridable — see `RecordEventDialog`). `null`
 * before kickoff or during an unclocked phase (extra time/penalties).
 * Added time in the first half carries over automatically: the second half's
 * minute count continues from wherever the first half actually left off,
 * rather than resetting to a fixed 45'.
 */
export function currentMinute(state: FootballLiveState, now: Date = new Date()): number | null {
  const phase = phaseFromState(state)
  const firstHalfMinutes =
    state.kickoff_at && state.half_time_at
      ? minutesBetween(state.kickoff_at, state.half_time_at)
      : 0

  switch (phase) {
    case 'first_half':
      return state.kickoff_at ? minutesBetween(state.kickoff_at, now) : null
    case 'half_time':
      return state.kickoff_at && state.half_time_at ? firstHalfMinutes : null
    case 'second_half':
      return state.second_half_kickoff_at
        ? firstHalfMinutes + minutesBetween(state.second_half_kickoff_at, now)
        : null
    case 'full_time':
      if (!state.full_time_at) return null
      return state.second_half_kickoff_at
        ? firstHalfMinutes + minutesBetween(state.second_half_kickoff_at, state.full_time_at)
        : state.kickoff_at
          ? minutesBetween(state.kickoff_at, state.full_time_at)
          : null
    case 'not_started':
    case 'other':
      return null
  }
}

/** Human label for the current phase, e.g. "2nd half" / "Half-time". */
export function phaseLabel(phase: ClockPhase): string {
  switch (phase) {
    case 'not_started':
      return 'Not started'
    case 'first_half':
      return '1st half'
    case 'half_time':
      return 'Half-time'
    case 'second_half':
      return '2nd half'
    case 'full_time':
      return 'Full-time'
    case 'other':
      return 'Live'
  }
}

/** A compact clock label for a score box, e.g. "63'", "HT", "FT", "LIVE". */
export function liveClockLabel(state: FootballLiveState, now: Date = new Date()): string {
  const phase = phaseFromState(state)
  if (phase === 'half_time') return 'HT'
  if (phase === 'full_time') return 'FT'
  if (phase === 'other') return state.period ? periodLabel(state.period) : 'LIVE'
  const minute = currentMinute(state, now)
  return minute === null ? 'LIVE' : `${minute}'`
}

/** The `FootballPeriod` marker to record for "the next thing that happens"
 *  from the current phase (kickoff, end of a half, full time) — what the
 *  live scoring screen's Half/FT-style quick action should send. `null` once
 *  the match is done, or during an unclocked phase. */
export function nextPeriodForPhase(phase: ClockPhase): FootballPeriod | null {
  switch (phase) {
    case 'not_started':
      return 'kick_off'
    case 'first_half':
      return 'half_time'
    case 'half_time':
      return 'second_half_kick_off'
    case 'second_half':
      return 'full_time'
    case 'full_time':
    case 'other':
      return null
  }
}

/** Label for the clock's quick-action button, contextual to the current phase. */
export function nextPhaseActionLabel(phase: ClockPhase): string {
  switch (phase) {
    case 'not_started':
      return 'Kick off'
    case 'first_half':
      return 'End 1st half'
    case 'half_time':
      return 'Start 2nd half'
    case 'second_half':
      return 'End match'
    case 'full_time':
    case 'other':
      return 'Full-time'
  }
}
