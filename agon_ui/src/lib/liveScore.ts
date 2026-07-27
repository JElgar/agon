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
  half_time: 'Half-time',
  full_time: 'Full-time',
  extra_time_half_time: 'Extra time: half-time',
  extra_time_full_time: 'Extra time: full-time',
  penalties_complete: 'Penalties complete',
}

export function periodLabel(period: FootballPeriod): string {
  return PERIOD_LABEL[period]
}

/**
 * A short clock label for read-only viewers (feed card, match detail) who
 * have no local clock of their own — just the scorer's device does (see
 * below). Derived from what's actually on the log: the last period marker if
 * the match is between/after halves, else the latest recorded event minute,
 * else a plain "LIVE".
 */
export function latestMinuteLabel(state: FootballLiveState): string {
  if (state.period === 'half_time') return 'HT'
  if (state.period === 'full_time') return 'FT'
  if (state.period === 'extra_time_half_time') return 'ET · HT'
  if (state.period === 'extra_time_full_time') return 'ET · FT'
  if (state.period === 'penalties_complete') return 'Pens'
  const minutes = state.events.map((e) => e.minute).filter((m): m is number => m !== undefined)
  if (minutes.length === 0) return 'LIVE'
  return `${Math.max(...minutes)}'`
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
// Local match clock — the backend's live-event log has no notion of kickoff
// time or a running clock, only discrete events (optionally minute-stamped)
// plus a "last period marker seen" (`FootballLiveState.period`). The running
// 63'-style clock shown while scoring is purely a client-side stopwatch, kept
// in localStorage so it survives a refresh on the scorer's device; other
// viewers just see the discrete event log/derived score, not this clock.
// ---------------------------------------------------------------------------

export type ClockPhase = 'first_half' | 'half_time' | 'second_half' | 'full_time'

export interface ClockState {
  phase: ClockPhase
  /** ISO timestamp the current running phase started, null while paused. */
  runningSince: string | null
  /** Match minutes already elapsed before `runningSince` (frozen carry-over). */
  baseMinutes: number
}

function clockKey(matchId: string): string {
  return `agon:live-clock:${matchId}`
}

function freshClock(): ClockState {
  return { phase: 'first_half', runningSince: new Date().toISOString(), baseMinutes: 0 }
}

export function loadClock(matchId: string): ClockState {
  try {
    const raw = localStorage.getItem(clockKey(matchId))
    if (!raw) return freshClock()
    const parsed = JSON.parse(raw)
    if (
      typeof parsed.phase === 'string' &&
      typeof parsed.baseMinutes === 'number' &&
      (parsed.runningSince === null || typeof parsed.runningSince === 'string')
    ) {
      return parsed as ClockState
    }
    return freshClock()
  } catch {
    return freshClock()
  }
}

function saveClock(matchId: string, clock: ClockState): void {
  try {
    localStorage.setItem(clockKey(matchId), JSON.stringify(clock))
  } catch {
    // Best-effort, as above.
  }
}

/** Minutes elapsed in the running phase (0 while paused). */
function runningMinutes(clock: ClockState, now: Date): number {
  if (!clock.runningSince) return 0
  const ms = now.getTime() - new Date(clock.runningSince).getTime()
  return Math.max(0, Math.floor(ms / 60_000))
}

/** The current display minute, e.g. for prefilling an event's minute field. */
export function currentMinute(clock: ClockState, now: Date = new Date()): number {
  return clock.baseMinutes + runningMinutes(clock, now)
}

/** Human label for the current phase, e.g. "2nd half" / "Half-time". */
export function phaseLabel(phase: ClockPhase): string {
  switch (phase) {
    case 'first_half':
      return '1st half'
    case 'half_time':
      return 'Half-time'
    case 'second_half':
      return '2nd half'
    case 'full_time':
      return 'Full-time'
  }
}

/**
 * Advances the clock to its next phase, returning the new state and — when
 * the transition corresponds to a real period marker (end of a half) — the
 * `FootballPeriod` to record on the server. The half-time → second-half
 * transition has no backend marker (kickoff isn't modelled), so it advances
 * the local clock only.
 */
export function advanceClock(
  matchId: string,
  clock: ClockState,
  now: Date = new Date(),
): { clock: ClockState; period: FootballPeriod | null } {
  let next: ClockState
  let period: FootballPeriod | null = null

  switch (clock.phase) {
    case 'first_half':
      next = {
        phase: 'half_time',
        runningSince: null,
        baseMinutes: currentMinute(clock, now),
      }
      period = 'half_time'
      break
    case 'half_time':
      next = { phase: 'second_half', runningSince: now.toISOString(), baseMinutes: 45 }
      break
    case 'second_half':
      next = {
        phase: 'full_time',
        runningSince: null,
        baseMinutes: currentMinute(clock, now),
      }
      period = 'full_time'
      break
    case 'full_time':
      next = clock
      break
  }

  saveClock(matchId, next)
  return { clock: next, period }
}

/** Label for the clock's quick-action button, contextual to the current phase. */
export function nextPhaseActionLabel(phase: ClockPhase): string {
  switch (phase) {
    case 'first_half':
      return 'End 1st half'
    case 'half_time':
      return 'Start 2nd half'
    case 'second_half':
      return 'End match'
    case 'full_time':
      return 'Full-time'
  }
}
