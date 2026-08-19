import type { components } from '@/types/api'
import { memberName, type ScorePlayers } from './members'
import type { NetballFormat } from './matchFormat'

export type NetballPeriod = components['schemas']['NetballPeriod']
export type NetballGoalEvent = components['schemas']['NetballGoalEvent']
type NetballFoulEvent = components['schemas']['NetballFoulEvent']
export type NetballFoulKind = components['schemas']['NetballFoulKind']
type Score = components['schemas']['Score']
type MatchPlayer = components['schemas']['MatchPlayer']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
/** Anything with an optional `players` list — see `members.ts`'s `MatchLike`
 *  for why `FeedMatch`/`SearchMatch` need their own explicit branches. */
type MatchLike = { players?: MatchPlayer[] } | FeedMatch | SearchMatch

/** A netball `Score` — live or finished, confirmed or not, and regardless of
 *  which of netball's two live-scoring methods produced it (see
 *  `NetballScore`'s backend doc comment). Narrowed via `type`, so this
 *  retains the `type: 'Netball'` tag and is itself a valid `Score` to send
 *  back on a PATCH, same as `FootballScore` in `lib/liveScore.ts`. */
export type NetballScore = Extract<Score, { type: 'Netball' }>

/** A period-marker timestamp map, straight off `NetballScore.period_times` —
 *  the schema widens the key to a bare string, so this is what every helper
 *  below that just needs the timestamps (not the rest of a `NetballScore`)
 *  accepts. */
export type NetballPeriodTimes = Record<string, string>

/** Narrows a match's score to its netball variant, or `null` when there's
 *  none yet or it's for a different sport. */
export function netballScoreFrom(score: Score | null | undefined): NetballScore | null {
  if (!score || score.type !== 'Netball') return null
  return score
}

/** What `eventsFromDetail` reads off a `NetballScore` — narrowed so a
 *  finished match's `Score.Netball` and a live one can both feed the same
 *  timeline (same pattern as `FootballEventSource`). `null` for a
 *  quarter-only-scored or manually-entered result, which carries no
 *  goal-by-goal detail at all. `period_times` is optional — a manually
 *  entered result (see `NetballScoreFields`) has no live clock, so there's
 *  nothing to bracket an event's quarter-relative time against; those events
 *  fall back to their own free-text `minute` instead (see
 *  `eventClockLabel`). */
export type NetballEventSource = {
  goals: NetballGoalEvent[]
  fouls: NetballFoulEvent[]
  players: ScorePlayers
  period_times?: NetballPeriodTimes
}

export function netballEventSourceFromScore(score: Score | null | undefined): NetballEventSource | null {
  if (!score || score.type !== 'Netball') return null
  if (!score.goals && !score.fouls) return null
  return {
    goals: score.goals ?? [],
    fouls: score.fouls ?? [],
    players: score.players,
    period_times: score.period_times,
  }
}

/** A client-side view of one netball event, merging `NetballScore`'s
 *  separately-typed `goals`/`fouls` lists into one timeline — same role as
 *  `FootballEventView`. */
export type NetballEventKind = 'goal' | 'two_point_goal' | 'foul'
export interface NetballEventView {
  kind: NetballEventKind
  side_id: string
  /** Free-text minutes-into-the-match — only ever set on a manually logged
   *  historical event (no live clock). A live-scored event's time comes from
   *  `occurred_at` instead; see `eventClockLabel`. */
  minute?: number
  /** Wall-clock time, set on every live-scored event (and unset on a
   *  manually logged one) — the source of truth for both display (via
   *  `eventClockLabel`) and chronological order (via `eventsFromDetail`'s
   *  sort), unlike `minute`, which resets every quarter. */
  occurred_at?: string
  player_id?: string
  /** Present for `kind: 'foul'` only. */
  foul_kind?: NetballFoulKind
}

function goalKind(g: NetballGoalEvent): NetballEventKind {
  return g.two_points ? 'two_point_goal' : 'goal'
}

/** Maps a bare goals list — `NetballScore.goals`, live or finished — to
 *  event views. Unsorted; callers order as needed. Empty in quarter-only
 *  mode, where there's no goal-by-goal detail behind the tally at all. */
export function goalEventsToViews(goals: NetballGoalEvent[]): NetballEventView[] {
  return goals.map((g): NetballEventView => ({
    kind: goalKind(g),
    side_id: g.side_id,
    minute: g.minute,
    occurred_at: g.occurred_at,
    player_id: g.scorer_player_id,
  }))
}

/** Sort key for ordering events chronologically: `occurred_at` (a live-scored
 *  event's wall-clock time) when present, else a coarse fallback off the
 *  free-text `minute` a manually logged event carries instead, else last.
 *  `minute` alone is *not* usable for a live-scored match — it resets every
 *  quarter, so two events from different quarters can carry the same (or an
 *  inverted) `minute` even though one strictly happened after the other; see
 *  `NetballGoalEvent.occurred_at`'s backend doc comment. In practice a given
 *  match's events come from one source or the other, never a mix, so this
 *  fallback never has to reconcile the two within the same list. */
function eventSortKey(view: { occurred_at?: string; minute?: number }): number {
  if (view.occurred_at) return new Date(view.occurred_at).getTime()
  if (view.minute !== undefined) return view.minute * 60_000
  return Infinity
}

/** All of a netball score's goals/fouls merged into one timeline, in true
 *  chronological order (undated events last). Takes just `NetballEventSource`
 *  (not the full `NetballScore`) so a finished match's `Score.Netball` can
 *  feed it too — see `netballEventSourceFromScore`. */
export function eventsFromDetail(detail: NetballEventSource): NetballEventView[] {
  const events: NetballEventView[] = [
    ...goalEventsToViews(detail.goals),
    ...detail.fouls.map((f): NetballEventView => ({
      kind: 'foul',
      side_id: f.side_id,
      minute: f.minute,
      occurred_at: f.occurred_at,
      player_id: f.player_id,
      foul_kind: f.foul_kind,
    })),
  ]
  return events.sort((a, b) => eventSortKey(a) - eventSortKey(b))
}

const EVENT_LABEL: Record<NetballEventKind, string> = {
  goal: 'Goal',
  two_point_goal: '2pt goal',
  foul: 'Foul',
}

const EVENT_EMOJI: Record<NetballEventKind, string> = {
  goal: '🏐',
  two_point_goal: '🏐',
  foul: '🚫',
}

export function eventLabel(kind: NetballEventKind): string {
  return EVENT_LABEL[kind]
}

export function eventEmoji(kind: NetballEventKind): string {
  return EVENT_EMOJI[kind]
}

const FOUL_KIND_LABEL: Record<NetballFoulKind, string> = {
  contact: 'Contact',
  obstruction: 'Obstruction',
  footwork: 'Footwork',
  offside: 'Offside',
  held_ball: 'Held ball',
  other: 'Other',
}

/** Every foul kind, in the order offered on `RecordNetballEventDialog`'s
 *  picker. */
export const FOUL_KINDS: NetballFoulKind[] = [
  'contact',
  'obstruction',
  'footwork',
  'offside',
  'held_ball',
  'other',
]

export function foulKindLabel(kind: NetballFoulKind): string {
  return FOUL_KIND_LABEL[kind]
}

/** "12'" for a recorded free-text minute (minutes into the *current
 *  quarter*, manual entry only — see `NetballGoalEvent.minute`'s backend doc
 *  comment), else a blank string. A live-scored event should use
 *  `eventClockLabel` instead, which prefers `occurred_at`. `null` (a
 *  live-scored goal's unset `minute`, sent as JSON `null` rather than
 *  omitted — see that field's backend doc comment) is blank too, same as
 *  `undefined`. */
export function minuteLabel(minute: number | undefined | null): string {
  return minute == null ? '' : `${minute}'`
}

/** "3:45" for a whole number of seconds — the live clock's display format,
 *  seconds included (see `NetballGoalEvent.occurred_at`'s backend doc
 *  comment for why the app moved off minute-only granularity). */
export function formatClock(totalSeconds: number): string {
  const m = Math.floor(totalSeconds / 60)
  const s = totalSeconds % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

/** The quarter/segment-start markers, in play order — what an arbitrary
 *  timestamp gets bracketed against to find which quarter it fell in (see
 *  `elapsedInQuarterAt`) and what the live clock counts minutes from (see
 *  `currentQuarterStartedAt`). `start` covers the 1st quarter; every quarter
 *  after that (and extra time) has its own explicit start marker — see
 *  `ClockPhase`'s doc comment. */
const QUARTER_START_MARKERS: NetballPeriod[] = [
  'start',
  'quarter_two_start',
  'quarter_three_start',
  'quarter_four_start',
  'extra_time_start',
]

/** Seconds elapsed within whichever quarter/segment was running when
 *  `occurredAt` happened, found by bracketing it against the recorded
 *  quarter-start markers — the live equivalent of `currentQuarterElapsedSeconds`
 *  but for an arbitrary past instant instead of "now". `null` if `occurredAt`
 *  predates every recorded quarter start (shouldn't happen for a real event,
 *  but keeps this total instead of guessing). */
export function elapsedInQuarterAt(
  periodTimes: NetballPeriodTimes | undefined,
  occurredAt: string,
): number | null {
  const t = new Date(occurredAt).getTime()
  let bracketStart: number | null = null
  for (const marker of QUARTER_START_MARKERS) {
    const markerTime = periodTimes?.[marker]
    if (!markerTime) continue
    const ms = new Date(markerTime).getTime()
    if (ms <= t && (bracketStart === null || ms > bracketStart)) bracketStart = ms
  }
  if (bracketStart === null) return null
  return Math.max(0, Math.floor((t - bracketStart) / 1000))
}

/** Display label for one event's time — "3:45" (mm:ss within its quarter)
 *  for a live-scored event, derived from `occurred_at` plus the match's
 *  recorded quarter-start markers; the legacy bare-minute label
 *  (`minuteLabel`) for a manually logged one. Blank if neither is known
 *  (e.g. `periodTimes` wasn't available to bracket a live event against). */
export function eventClockLabel(event: NetballEventView, periodTimes?: NetballPeriodTimes): string {
  if (event.occurred_at) {
    const elapsed = elapsedInQuarterAt(periodTimes, event.occurred_at)
    if (elapsed !== null) return formatClock(elapsed)
  }
  return minuteLabel(event.minute)
}

const PERIOD_LABEL: Record<NetballPeriod, string> = {
  start: 'Start',
  quarter_one_end: 'End of Q1',
  quarter_two_start: 'Start of Q2',
  quarter_two_end: 'Half-time',
  quarter_three_start: 'Start of 2nd half',
  quarter_three_end: 'End of Q3',
  quarter_four_start: 'Start of Q4',
  full_time: 'Full-time',
  extra_time_start: 'Extra time',
  extra_time_end: 'Extra time complete',
}

export function periodLabel(period: NetballPeriod): string {
  return PERIOD_LABEL[period]
}

/** The most recent events first, capped to `limit` — for a mini-ticker. */
export function recentEvents(detail: NetballScore, limit: number): NetballEventView[] {
  return eventsFromDetail({
    goals: detail.goals ?? [],
    fouls: detail.fouls ?? [],
    players: detail.players,
    period_times: detail.period_times,
  })
    .reverse()
    .slice(0, limit)
}

/** Player display name for a match-scoped player id — same contract as
 *  football's `playerNameFor` in `lib/liveScore.ts`. */
function playerNameFor(
  match: MatchLike,
  playerId: string | undefined,
  scorePlayers?: ScorePlayers,
): string | null {
  if (!playerId) return null
  const resolved = scorePlayers?.[playerId]
  if (resolved) return resolved.name
  const players = ('players' in match && match.players) || []
  const player = players.find((p) => p.member.id === playerId)
  return player ? memberName(player.member) : null
}

/** One-line human description of a netball event, e.g. "Goal — J. Elgar" or
 *  "Foul (Contact) — A. Silva". A foul always names its kind (falling back to
 *  the bare "Foul" only if somehow unset) — which isn't repeated in the text
 *  is which side it belongs to; callers convey that by aligning/positioning
 *  the row using `event.side_id` instead (see football's `describeEvent`). */
export function describeEvent(
  event: NetballEventView,
  match: MatchLike,
  scorePlayers?: ScorePlayers,
): string {
  const scorer = playerNameFor(match, event.player_id, scorePlayers)
  const label =
    event.kind === 'foul' && event.foul_kind
      ? `Foul (${foulKindLabel(event.foul_kind)})`
      : eventLabel(event.kind)
  return scorer ? `${label} — ${scorer}` : label
}

/** One goalscorer's line for a side's scorer list — same shape/role as
 *  football's `FootballScorerLine`. `times` is already display-formatted
 *  (mm:ss or the legacy bare-minute label — see `eventClockLabel`), one per
 *  goal, in chronological order. */
export interface NetballScorerLine {
  key: string
  name: string
  times: string[]
}

/** All of a netball match's goals, grouped by crediting side and then by
 *  scorer — same construction as football's `scorersBySide`, minus the
 *  own-goal handling netball has no equivalent of. `periodTimes` lets a
 *  live-scored goal's time show as mm:ss-within-its-quarter instead of
 *  falling back to its (usually unset) free-text `minute` — see
 *  `eventClockLabel`. */
export function scorersBySide(
  goals: NetballGoalEvent[],
  match: MatchLike,
  scorePlayers?: ScorePlayers,
  periodTimes?: NetballPeriodTimes,
): Record<string, NetballScorerLine[]> {
  const bySide: Record<string, Map<string, { name: string; entries: { key: number; label: string }[] }>> = {}
  for (const g of goals) {
    const lines = (bySide[g.side_id] ??= new Map())
    const key = g.scorer_player_id ?? 'unknown'
    const name = playerNameFor(match, g.scorer_player_id, scorePlayers) ?? 'Unknown'
    const line = lines.get(key) ?? { name, entries: [] }
    const view: NetballEventView = {
      kind: goalKind(g),
      side_id: g.side_id,
      minute: g.minute,
      occurred_at: g.occurred_at,
      player_id: g.scorer_player_id,
    }
    line.entries.push({ key: eventSortKey(view), label: eventClockLabel(view, periodTimes) })
    lines.set(key, line)
  }
  const out: Record<string, NetballScorerLine[]> = {}
  for (const [sideId, lines] of Object.entries(bySide)) {
    out[sideId] = [...lines.entries()]
      .map(([key, line]) => {
        const sorted = [...line.entries].sort((a, b) => a.key - b.key)
        return {
          key,
          name: line.name,
          times: sorted.filter((e) => e.label).map((e) => e.label),
          firstKey: sorted[0]?.key ?? Infinity,
        }
      })
      .sort((a, b) => a.firstKey - b.firstKey)
      .map(({ key, name, times }) => ({ key, name, times }))
  }
  return out
}

/**
 * Which of netball's two live-scoring methods a match's scorer is using.
 * Not persisted anywhere — the backend doesn't need to know (both methods
 * write the exact same `NetballLiveEvent` vocabulary; see
 * `NetballLiveEvent`'s backend doc comment) and neither does the client
 * beyond the current page: once the match's live event log has anything
 * recorded, the method is unambiguous from the log itself (see
 * `NetballLiveScoringPage`'s doc comment), so there's nothing worth
 * remembering across visits — before that point, asking again is cheap and
 * correct.
 */
export type NetballScoringMethod = 'event_by_event' | 'quarter_only'

// ---------------------------------------------------------------------------
// Match clock — derived from `NetballScore.period_times`, the same "shared
// server data, not a per-device stopwatch" approach as football's clock (see
// `lib/liveScore.ts`'s module doc comment). Unlike football, a goal's
// on-quarter time resets each quarter, so the clock here doesn't need to
// accumulate across periods the way football's does — it's just time since
// whichever marker started the current quarter.
// ---------------------------------------------------------------------------

export type ClockPhase =
  | 'not_started'
  | 'quarter_1'
  | 'break_1'
  | 'quarter_2'
  | 'break_2'
  | 'quarter_3'
  | 'break_3'
  | 'quarter_4'
  | 'full_time'
  | 'extra_time'
  | 'finished'

/** Which phase the match is in right now, purely from recorded markers.
 *  Every quarter after the first (and extra time) needs its own explicit
 *  *start* marker before play resumes — an *end* marker alone drops the
 *  match into a `break_*` phase, not straight into the next quarter, so the
 *  scorer has to tap "start" once the break is actually over (of whatever
 *  length — nothing here assumes how long it runs). See `NetballPeriod`'s
 *  backend doc comment for why this mirrors the pattern `full_time` →
 *  `extra_time_start` already used. */
export function phaseFromState(state: NetballScore): ClockPhase {
  if (periodTime(state, 'extra_time_end')) return 'finished'
  if (periodTime(state, 'extra_time_start')) return 'extra_time'
  if (periodTime(state, 'full_time')) return 'full_time'
  if (periodTime(state, 'quarter_four_start')) return 'quarter_4'
  if (periodTime(state, 'quarter_three_end')) return 'break_3'
  if (periodTime(state, 'quarter_three_start')) return 'quarter_3'
  if (periodTime(state, 'quarter_two_end')) return 'break_2'
  if (periodTime(state, 'quarter_two_start')) return 'quarter_2'
  if (periodTime(state, 'quarter_one_end')) return 'break_1'
  if (periodTime(state, 'start')) return 'quarter_1'
  return 'not_started'
}

/** The score as of a given quarter marker, straight off
 *  `NetballScore.period_scores` — the same map both of netball's
 *  live-scoring methods populate (see its backend doc comment), so this
 *  works identically whichever produced the score. */
export function periodScore(
  detail: NetballScore,
  period: NetballPeriod,
): Record<string, number> | undefined {
  return detail.period_scores?.[period]
}

/** When a given period marker was recorded, if at all. */
export function periodTime(detail: NetballScore, period: NetballPeriod): string | undefined {
  return detail.period_times?.[period]
}

/** The marker that starts the *current* quarter — what the live clock counts
 *  from (`currentQuarterElapsedSeconds`) — `undefined` for a phase with no
 *  clock running (`not_started`, any `break_*`, `full_time`, `finished`): the
 *  ball isn't live during a break, so there's nothing to time. */
function currentQuarterStartedAt(state: NetballScore, phase: ClockPhase): string | undefined {
  switch (phase) {
    case 'quarter_1':
      return periodTime(state, 'start')
    case 'quarter_2':
      return periodTime(state, 'quarter_two_start')
    case 'quarter_3':
      return periodTime(state, 'quarter_three_start')
    case 'quarter_4':
      return periodTime(state, 'quarter_four_start')
    case 'extra_time':
      return periodTime(state, 'extra_time_start')
    case 'not_started':
    case 'break_1':
    case 'break_2':
    case 'break_3':
    case 'full_time':
    case 'finished':
      return undefined
  }
}

/** Seconds elapsed in the current quarter — the live clock's tick, with
 *  second precision (see `NetballGoalEvent.occurred_at`'s backend doc
 *  comment). `null` when there's no quarter currently running (including
 *  during a break — see `currentQuarterStartedAt`). */
export function currentQuarterElapsedSeconds(state: NetballScore, now: Date = new Date()): number | null {
  const phase = phaseFromState(state)
  const startedAt = currentQuarterStartedAt(state, phase)
  if (!startedAt) return null
  return Math.max(0, Math.floor((now.getTime() - new Date(startedAt).getTime()) / 1000))
}

/** Minutes elapsed in the current quarter — matches the scope of the legacy
 *  `NetballGoalEvent.minute`. `null` when there's no quarter currently
 *  running. Prefer `currentQuarterElapsedSeconds` for anything that should
 *  tick with second precision (e.g. the live scoring page's own clock). */
export function currentMinute(state: NetballScore, now: Date = new Date()): number | null {
  const seconds = currentQuarterElapsedSeconds(state, now)
  return seconds === null ? null : Math.floor(seconds / 60)
}

const QUARTER_PLAY_PHASES: ClockPhase[] = ['quarter_1', 'quarter_2', 'quarter_3', 'quarter_4']

/** Whether the current quarter has run past the format's quarter length —
 *  the live scoring page's cue to turn the clock red so it's obvious time's
 *  up, even though nothing stops the clock (or scoring) automatically; the
 *  scorer still decides when to actually end the quarter. Extra time has no
 *  configured length (`NetballFormat` doesn't carry one), so it never flags
 *  as over time here. */
export function isQuarterOvertime(
  state: NetballScore,
  format: NetballFormat,
  now: Date = new Date(),
): boolean {
  const phase = phaseFromState(state)
  if (!QUARTER_PLAY_PHASES.includes(phase)) return false
  const seconds = currentQuarterElapsedSeconds(state, now)
  return seconds !== null && seconds >= format.quarter_length_minutes * 60
}

/** Whether the ball is live right now — the phases a goal/foul can
 *  meaningfully be recorded in. A `break_*` phase is deliberately excluded:
 *  nothing happens on court between the whistle ending a quarter and the
 *  scorer tapping "start" on the next one, so there's no event to record
 *  there either. Used to gate the live scoring screen's quick actions on the
 *  current phase, same role as football's `isLivePlayPhase`. */
export function isLivePlayPhase(phase: ClockPhase): boolean {
  return phase === 'quarter_1' || phase === 'quarter_2' || phase === 'quarter_3' || phase === 'quarter_4' || phase === 'extra_time'
}

/** Human label for the current phase, e.g. "3rd quarter" / "Half-time". */
export function phaseLabel(phase: ClockPhase): string {
  switch (phase) {
    case 'not_started':
      return 'Not started'
    case 'quarter_1':
      return '1st quarter'
    case 'break_1':
      return 'Break'
    case 'quarter_2':
      return '2nd quarter'
    case 'break_2':
      return 'Half-time'
    case 'quarter_3':
      return '3rd quarter'
    case 'break_3':
      return 'Break'
    case 'quarter_4':
      return '4th quarter'
    case 'full_time':
      return 'Full-time'
    case 'extra_time':
      return 'Extra time'
    case 'finished':
      return 'Finished'
  }
}

/** A compact clock label for a score box, e.g. "12'", "HT", "FT". */
export function liveClockLabel(state: NetballScore, now: Date = new Date()): string {
  const phase = phaseFromState(state)
  if (phase === 'full_time' || phase === 'finished') return 'FT'
  if (phase === 'break_1' || phase === 'break_2' || phase === 'break_3') return phaseLabel(phase).toUpperCase()
  const minute = currentMinute(state, now)
  return minute === null ? 'LIVE' : `${minute}'`
}

/** Whether extra time can extend the match beyond full time, and whether
 *  it's currently needed (level on goals) — same role as football's
 *  `FootballProgressionContext`. */
export interface NetballProgressionContext {
  isDraw: boolean
  extraTime: boolean
}

/** The `NetballPeriod` marker to record for "the next thing that happens"
 *  from the current phase — what the live scoring screen's single advance
 *  action should send, whether that's ending the current quarter or
 *  starting the next one after a break. `null` once the match is done, or
 *  before it's started (the first event any live-scoring method records is
 *  `Start` itself, so there's no "next marker" to prefill before that). */
export function nextPeriodForPhase(
  phase: ClockPhase,
  ctx: NetballProgressionContext,
): NetballPeriod | null {
  switch (phase) {
    case 'not_started':
      return 'start'
    case 'quarter_1':
      return 'quarter_one_end'
    case 'break_1':
      return 'quarter_two_start'
    case 'quarter_2':
      return 'quarter_two_end'
    case 'break_2':
      return 'quarter_three_start'
    case 'quarter_3':
      return 'quarter_three_end'
    case 'break_3':
      return 'quarter_four_start'
    case 'quarter_4':
      return 'full_time'
    case 'full_time':
      return ctx.isDraw && ctx.extraTime ? 'extra_time_start' : null
    case 'extra_time':
      return 'extra_time_end'
    case 'finished':
      return null
  }
}

/** Label for the clock's quick-action button, contextual to the current phase. */
export function nextPhaseActionLabel(
  phase: ClockPhase,
  ctx: NetballProgressionContext,
): string {
  switch (phase) {
    case 'not_started':
      return 'Start match'
    case 'quarter_1':
      return 'End 1st quarter'
    case 'break_1':
      return 'Start 2nd quarter'
    case 'quarter_2':
      return 'End 2nd quarter (half-time)'
    case 'break_2':
      return 'Start 2nd half'
    case 'quarter_3':
      return 'End 3rd quarter'
    case 'break_3':
      return 'Start 4th quarter'
    case 'quarter_4':
      return ctx.isDraw && ctx.extraTime ? 'Full-time — start extra time' : 'Full-time'
    case 'extra_time':
      return 'End extra time'
    case 'full_time':
    case 'finished':
      return 'Full-time'
  }
}
