import type { components } from '@/types/api'
import { memberName } from './members'

export type FootballPeriod = components['schemas']['FootballPeriod']
export type FootballDetail = components['schemas']['FootballDetail']
export type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type FootballCardEvent = components['schemas']['FootballCardEvent']
type FootballSubstitutionEvent = components['schemas']['FootballSubstitutionEvent']
type DetailedScore = components['schemas']['DetailedScore']
type Score = components['schemas']['Score']
type Match = components['schemas']['Match']

/** Narrows a match's detailed score to its football detail, or `null` when
 *  there's none yet or it's for a different sport. */
export function footballDetailFrom(detail: DetailedScore | null | undefined): FootballDetail | null {
  if (!detail || detail.type !== 'Football') return null
  return detail
}

/** What `eventsFromDetail` actually reads off a `FootballDetail` — narrowed
 *  so a finished match's `Score.Football` (whose `goals`/`cards`/
 *  `substitutions` are optional, unlike `FootballDetail`'s) can feed the same
 *  timeline via `footballEventSourceFromScore`, without a live-only fetch. */
export type FootballEventSource = {
  goals: FootballGoalEvent[]
  cards: FootballCardEvent[]
  substitutions: FootballSubstitutionEvent[]
}

/** A finished football match's event timeline straight off its confirmed/
 *  pending score — `null` if there's no score yet, it's a different sport, or
 *  it's a football score with no detail attached (a manual entry with just
 *  the final tally). Lets the match detail page's `FootballScorecard` avoid
 *  fetching `DetailedScore` once the match is over (see `footballDetail`,
 *  which still covers the live, in-progress view). */
export function footballEventSourceFromScore(score: Score | null | undefined): FootballEventSource | null {
  if (!score || score.type !== 'Football') return null
  if (!score.goals && !score.cards && !score.substitutions) return null
  return {
    goals: score.goals ?? [],
    cards: score.cards ?? [],
    substitutions: score.substitutions ?? [],
  }
}

/** A client-side view of one football event, merging `FootballDetail`'s
 *  separately-typed `goals`/`cards`/`substitutions` lists back into a single
 *  timeline for the event log/mini-ticker (see `eventsFromDetail`) — the
 *  backend keeps them apart so a goal's scorer/assist/own-goal/penalty
 *  fields mean the same thing they did when recorded, not a shared field
 *  reinterpreted per kind. */
export type FootballEventKind = 'goal' | 'own_goal' | 'penalty' | 'yellow_card' | 'red_card' | 'substitution'
export interface FootballEventView {
  kind: FootballEventKind
  side_id: string
  minute?: number
  player_id?: string
  assist_player_id?: string
  substituted_player_id?: string
}

function goalKind(g: FootballGoalEvent): FootballEventKind {
  if (g.own_goal) return 'own_goal'
  if (g.penalty) return 'penalty'
  return 'goal'
}

function cardKind(c: FootballCardEvent): FootballEventKind {
  return c.color === 'yellow' ? 'yellow_card' : 'red_card'
}

/** Maps a bare goals list — either `FootballDetail.goals` or a finished
 *  match's `Score.Football.goals` (see `footballGoalsFromScore`) — to event
 *  views. Unsorted; callers order as needed (`eventsFromDetail` merges and
 *  sorts alongside cards/subs, `recentGoalEvents` sorts standalone). */
export function goalEventsToViews(goals: FootballGoalEvent[]): FootballEventView[] {
  return goals.map((g): FootballEventView => ({
    kind: goalKind(g),
    side_id: g.side_id,
    minute: g.minute,
    player_id: g.scorer_player_id,
    assist_player_id: g.assist_player_id,
  }))
}

/** All of a football detail's goals/cards/substitutions merged into one
 *  timeline, ordered by minute (undated events last — the scorer always
 *  fills in a minute in practice, so this only matters for edge cases).
 *  Takes just `FootballEventSource` (not the full `FootballDetail`) so a
 *  finished match's `Score.Football` can feed it too — see
 *  `footballEventSourceFromScore`. */
export function eventsFromDetail(detail: FootballEventSource): FootballEventView[] {
  const events: FootballEventView[] = [
    ...goalEventsToViews(detail.goals),
    ...detail.cards.map((c): FootballEventView => ({
      kind: cardKind(c),
      side_id: c.side_id,
      minute: c.minute,
      player_id: c.player_id,
    })),
    ...detail.substitutions.map((s): FootballEventView => ({
      kind: 'substitution',
      side_id: s.side_id,
      minute: s.minute,
      player_id: s.player_in_id,
      substituted_player_id: s.player_out_id,
    })),
  ]
  return events.sort((a, b) => (a.minute ?? Infinity) - (b.minute ?? Infinity))
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
  extra_time_kick_off: 'Extra time: kick-off',
  extra_time_half_time: 'Extra time: half-time',
  extra_time_second_half_kick_off: 'Extra time: second-half kick-off',
  extra_time_full_time: 'Extra time: full-time',
  penalties_complete: 'Penalties complete',
}

export function periodLabel(period: FootballPeriod): string {
  return PERIOD_LABEL[period]
}

/** The most recent events first, capped to `limit` — for a mini-ticker. */
export function recentEvents(detail: FootballDetail, limit: number): FootballEventView[] {
  return eventsFromDetail(detail).reverse().slice(0, limit)
}

/** The most recent goals first, capped to `limit` — for a finished match's
 *  feed-card ticker, built straight from the goals embedded in its
 *  `Score.Football` (see `footballGoalsFromScore`) rather than a full
 *  `FootballDetail` fetch, which a completed match no longer needs just to
 *  show who scored. */
export function recentGoalEvents(goals: FootballGoalEvent[], limit: number): FootballEventView[] {
  return goalEventsToViews(goals)
    .sort((a, b) => (a.minute ?? Infinity) - (b.minute ?? Infinity))
    .reverse()
    .slice(0, limit)
}

/** Player display name for a member id, if it's on the roster. */
function playerNameFor(match: Pick<Match, 'players'>, playerId: string | undefined): string | null {
  if (!playerId) return null
  const player = match.players.find((p) => p.member.id === playerId)
  return player ? memberName(player.member) : null
}

/** One-line human description of a football event, e.g. "Goal — J. Alvarez
 *  (A. Silva)" or "Sub — Moreno on for Khan" — used by the event log and
 *  mini-ticker. Which side it belongs to isn't repeated in the text; callers
 *  convey that by aligning/positioning the row using `event.side_id`
 *  instead (see `LiveScoringPage`/`LiveMatchBlock`). */
export function describeEvent(
  event: FootballEventView,
  match: Pick<Match, 'players'>,
): string {
  const scorer = playerNameFor(match, event.player_id)

  switch (event.kind) {
    case 'goal':
    case 'penalty': {
      const assist = playerNameFor(match, event.assist_player_id)
      const base = scorer ? `${eventLabel(event.kind)} — ${scorer}` : eventLabel(event.kind)
      return assist ? `${base} (${assist})` : base
    }
    case 'own_goal':
      return eventLabel(event.kind)
    case 'yellow_card':
    case 'red_card':
      return scorer ? `${eventLabel(event.kind)} — ${scorer}` : eventLabel(event.kind)
    case 'substitution': {
      const out = playerNameFor(match, event.substituted_player_id)
      return scorer && out ? `Sub — ${scorer} on for ${out}` : 'Substitution'
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
// Match clock — derived entirely from `FootballDetail.period_times`, each the
// `occurred_at` of the period marker that recorded it server-side (see
// `agon_service::live_score::football::derive_detail`). Because it's
// computed from shared server data rather than a per-device stopwatch, the
// scorer's own screen and every other viewer (feed card, match detail) tick
// the exact same clock.
// ---------------------------------------------------------------------------

/** When a given period marker was recorded, if at all — a typed lookup into
 *  `FootballDetail.period_times`'s string-keyed map. */
export function periodTime(detail: FootballDetail, period: FootballPeriod): string | undefined {
  return detail.period_times[period]
}

export type ClockPhase =
  | 'not_started'
  | 'first_half'
  | 'half_time'
  | 'second_half'
  | 'full_time'
  | 'extra_time_first_half'
  | 'extra_time_half_time'
  | 'extra_time_second_half'
  | 'extra_time_full_time'
  /** Shootout under way or finished (`penalties_complete` recorded) — see
   *  `penaltiesComplete`. Not individually clocked; kicks are just recorded
   *  in order taken. */
  | 'penalties'
  /** An unrecognized marker was recorded (forward-compat only — every
   *  `FootballPeriod` variant maps to a phase above today). */
  | 'other'

/** Which phase the match is in right now, purely from recorded timestamps
 *  (plus, for `penalties`, whether any shootout kicks exist yet — there's no
 *  dedicated "shootout started" marker, the first kick doubles as one). */
export function phaseFromState(state: FootballDetail): ClockPhase {
  if (periodTime(state, 'penalties_complete') || state.penalty_shootout.length > 0) {
    return 'penalties'
  }
  if (periodTime(state, 'extra_time_full_time')) return 'extra_time_full_time'
  if (periodTime(state, 'extra_time_second_half_kick_off')) return 'extra_time_second_half'
  if (periodTime(state, 'extra_time_half_time')) return 'extra_time_half_time'
  if (periodTime(state, 'extra_time_kick_off')) return 'extra_time_first_half'
  if (periodTime(state, 'full_time')) return 'full_time'
  if (periodTime(state, 'second_half_kick_off')) return 'second_half'
  if (periodTime(state, 'half_time')) return 'half_time'
  if (periodTime(state, 'kick_off')) return 'first_half'
  if (state.period) return 'other'
  return 'not_started'
}

/** Whether the penalty shootout has been marked done. `phaseFromState`
 *  returns `'penalties'` for both an in-progress and a finished shootout —
 *  callers that need to tell them apart (e.g. to show "Finish match" instead
 *  of the kick-recording panel) check this too. */
export function penaltiesComplete(state: FootballDetail): boolean {
  return !!periodTime(state, 'penalties_complete')
}

/** A side's penalty-shootout tally (kicks scored, not taken) — the shootout
 *  equivalent of reading `FootballDetail.score` for goals. */
export function shootoutScoreFor(state: FootballDetail, sideId: string | undefined): number {
  return (sideId ? state.penalty_shootout_score[sideId] : undefined) ?? 0
}

function minutesBetween(from: string, to: Date | string): number {
  return Math.max(0, Math.floor((new Date(to).getTime() - new Date(from).getTime()) / 60_000))
}

/**
 * The current match minute, e.g. for the live clock display or to prefill an
 * event's minute field (still overridable — see `RecordEventDialog`). `null`
 * before kickoff or during an unclocked phase (penalties). Added time in a
 * half carries over automatically into the next one — first half into
 * second half, and normal time into extra time — so the clock counts up
 * continuously (e.g. 94', then 106' once extra time starts) rather than
 * resetting to a fixed 45'/90' each time.
 */
export function currentMinute(state: FootballDetail, now: Date = new Date()): number | null {
  const phase = phaseFromState(state)
  const kickoffAt = periodTime(state, 'kick_off')
  const halfTimeAt = periodTime(state, 'half_time')
  const secondHalfKickoffAt = periodTime(state, 'second_half_kick_off')
  const fullTimeAt = periodTime(state, 'full_time')
  const etKickoffAt = periodTime(state, 'extra_time_kick_off')
  const etHalfTimeAt = periodTime(state, 'extra_time_half_time')
  const etSecondHalfKickoffAt = periodTime(state, 'extra_time_second_half_kick_off')
  const etFullTimeAt = periodTime(state, 'extra_time_full_time')

  const firstHalfMinutes = kickoffAt && halfTimeAt ? minutesBetween(kickoffAt, halfTimeAt) : 0
  const normalTimeMinutes = fullTimeAt
    ? secondHalfKickoffAt
      ? firstHalfMinutes + minutesBetween(secondHalfKickoffAt, fullTimeAt)
      : kickoffAt
        ? minutesBetween(kickoffAt, fullTimeAt)
        : 0
    : 0
  const etFirstHalfMinutes = etKickoffAt && etHalfTimeAt ? minutesBetween(etKickoffAt, etHalfTimeAt) : 0

  switch (phase) {
    case 'first_half':
      return kickoffAt ? minutesBetween(kickoffAt, now) : null
    case 'half_time':
      return kickoffAt && halfTimeAt ? firstHalfMinutes : null
    case 'second_half':
      return secondHalfKickoffAt
        ? firstHalfMinutes + minutesBetween(secondHalfKickoffAt, now)
        : null
    case 'full_time':
      if (!fullTimeAt) return null
      return normalTimeMinutes
    case 'extra_time_first_half':
      return etKickoffAt ? normalTimeMinutes + minutesBetween(etKickoffAt, now) : null
    case 'extra_time_half_time':
      return etKickoffAt && etHalfTimeAt ? normalTimeMinutes + etFirstHalfMinutes : null
    case 'extra_time_second_half':
      return etSecondHalfKickoffAt
        ? normalTimeMinutes + etFirstHalfMinutes + minutesBetween(etSecondHalfKickoffAt, now)
        : null
    case 'extra_time_full_time':
      if (!etFullTimeAt) return null
      return etSecondHalfKickoffAt
        ? normalTimeMinutes + etFirstHalfMinutes + minutesBetween(etSecondHalfKickoffAt, etFullTimeAt)
        : etKickoffAt
          ? normalTimeMinutes + minutesBetween(etKickoffAt, etFullTimeAt)
          : null
    case 'not_started':
    case 'penalties':
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
    case 'extra_time_first_half':
      return 'Extra time: 1st half'
    case 'extra_time_half_time':
      return 'Extra time: half-time'
    case 'extra_time_second_half':
      return 'Extra time: 2nd half'
    case 'extra_time_full_time':
      return 'Extra time: full-time'
    case 'penalties':
      return 'Penalties'
    case 'other':
      return 'Live'
  }
}

/** A compact clock label for a score box, e.g. "63'", "HT", "FT", "AET", "PENS". */
export function liveClockLabel(state: FootballDetail, now: Date = new Date()): string {
  const phase = phaseFromState(state)
  if (phase === 'half_time') return 'HT'
  if (phase === 'full_time') return 'FT'
  if (phase === 'extra_time_half_time') return 'ET HT'
  if (phase === 'extra_time_full_time') return 'AET'
  if (phase === 'penalties') return 'PENS'
  if (phase === 'other') return state.period ? periodLabel(state.period) : 'LIVE'
  const minute = currentMinute(state, now)
  return minute === null ? 'LIVE' : `${minute}'`
}

/** Whether extra time/penalties can extend the match beyond normal time, and
 *  whether it's currently needed (level on goals) — what `nextPeriodForPhase`
 *  /`nextPhaseActionLabel` need to decide whether `full_time`/
 *  `extra_time_full_time` lead further or the match is simply done. Entering
 *  the penalty shootout isn't a period marker (see `phaseFromState`), so it's
 *  not represented here — callers offer it separately once
 *  `extra_time_full_time` is reached still level with `penalties` enabled. */
export interface FootballProgressionContext {
  isDraw: boolean
  extraTime: boolean
}

/** The `FootballPeriod` marker to record for "the next thing that happens"
 *  from the current phase (kickoff, end of a half, full/extra time) — what
 *  the live scoring screen's Half/FT-style quick action should send. `null`
 *  once the match is done (or waiting on a non-period-marker decision like
 *  starting penalties — see `FootballProgressionContext`), or during an
 *  unclocked phase. */
export function nextPeriodForPhase(
  phase: ClockPhase,
  ctx: FootballProgressionContext,
): FootballPeriod | null {
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
      return ctx.isDraw && ctx.extraTime ? 'extra_time_kick_off' : null
    case 'extra_time_first_half':
      return 'extra_time_half_time'
    case 'extra_time_half_time':
      return 'extra_time_second_half_kick_off'
    case 'extra_time_second_half':
      return 'extra_time_full_time'
    case 'extra_time_full_time':
    case 'penalties':
    case 'other':
      return null
  }
}

/** Label for the clock's quick-action button, contextual to the current phase. */
export function nextPhaseActionLabel(
  phase: ClockPhase,
  ctx: FootballProgressionContext,
): string {
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
      return ctx.isDraw && ctx.extraTime ? 'Start extra time' : 'Full-time'
    case 'extra_time_first_half':
      return 'End ET 1st half'
    case 'extra_time_half_time':
      return 'Start ET 2nd half'
    case 'extra_time_second_half':
      return 'End extra time'
    case 'extra_time_full_time':
    case 'penalties':
    case 'other':
      return 'Full-time'
  }
}
