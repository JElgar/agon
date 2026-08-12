import type { components } from '@/types/api'
import { memberName, type ScorePlayers } from './members'

export type NetballPeriod = components['schemas']['NetballPeriod']
export type NetballGoalEvent = components['schemas']['NetballGoalEvent']
type NetballFoulEvent = components['schemas']['NetballFoulEvent']
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
 *  goal-by-goal detail at all. */
export type NetballEventSource = {
  goals: NetballGoalEvent[]
  fouls: NetballFoulEvent[]
  players: ScorePlayers
}

export function netballEventSourceFromScore(score: Score | null | undefined): NetballEventSource | null {
  if (!score || score.type !== 'Netball') return null
  if (!score.goals && !score.fouls) return null
  return { goals: score.goals ?? [], fouls: score.fouls ?? [], players: score.players }
}

/** A client-side view of one netball event, merging `NetballScore`'s
 *  separately-typed `goals`/`fouls` lists into one timeline — same role as
 *  `FootballEventView`. */
export type NetballEventKind = 'goal' | 'two_point_goal' | 'foul'
export interface NetballEventView {
  kind: NetballEventKind
  side_id: string
  minute?: number
  player_id?: string
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
    player_id: g.scorer_player_id,
  }))
}

/** All of a netball score's goals/fouls merged into one timeline, ordered by
 *  minute (undated events last). Takes just `NetballEventSource` (not the
 *  full `NetballScore`) so a finished match's `Score.Netball` can feed it
 *  too — see `netballEventSourceFromScore`. */
export function eventsFromDetail(detail: NetballEventSource): NetballEventView[] {
  const events: NetballEventView[] = [
    ...goalEventsToViews(detail.goals),
    ...detail.fouls.map((f): NetballEventView => ({
      kind: 'foul',
      side_id: f.side_id,
      minute: f.minute,
      player_id: f.player_id,
    })),
  ]
  return events.sort((a, b) => (a.minute ?? Infinity) - (b.minute ?? Infinity))
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

/** "12'" for a recorded minute (minutes into the *current quarter*, not
 *  match-wide — see `NetballGoalEvent.minute`'s backend doc comment), else a
 *  blank string. */
export function minuteLabel(minute: number | undefined): string {
  return minute === undefined ? '' : `${minute}'`
}

const PERIOD_LABEL: Record<NetballPeriod, string> = {
  start: 'Start',
  quarter_one_end: 'End of Q1',
  quarter_two_end: 'Half-time',
  quarter_three_end: 'End of Q3',
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
 *  "2pt goal — A. Silva". Which side it belongs to isn't repeated in the
 *  text; callers convey that by aligning/positioning the row using
 *  `event.side_id` instead (see football's `describeEvent`). */
export function describeEvent(
  event: NetballEventView,
  match: MatchLike,
  scorePlayers?: ScorePlayers,
): string {
  const scorer = playerNameFor(match, event.player_id, scorePlayers)
  return scorer ? `${eventLabel(event.kind)} — ${scorer}` : eventLabel(event.kind)
}

/** One goalscorer's line for a side's scorer list — same shape/role as
 *  football's `FootballScorerLine`. */
export interface NetballScorerLine {
  key: string
  name: string
  minutes: number[]
}

/** All of a netball match's goals, grouped by crediting side and then by
 *  scorer — same construction as football's `scorersBySide`, minus the
 *  own-goal handling netball has no equivalent of. */
export function scorersBySide(
  goals: NetballGoalEvent[],
  match: MatchLike,
  scorePlayers?: ScorePlayers,
): Record<string, NetballScorerLine[]> {
  const bySide: Record<string, Map<string, NetballScorerLine>> = {}
  for (const g of goals) {
    const lines = (bySide[g.side_id] ??= new Map())
    const key = g.scorer_player_id ?? 'unknown'
    const name = playerNameFor(match, g.scorer_player_id, scorePlayers) ?? 'Unknown'
    const line = lines.get(key) ?? { key, name, minutes: [] }
    if (g.minute !== undefined) line.minutes.push(g.minute)
    lines.set(key, line)
  }
  const out: Record<string, NetballScorerLine[]> = {}
  for (const [sideId, lines] of Object.entries(bySide)) {
    out[sideId] = [...lines.values()]
      .map((line) => ({ ...line, minutes: [...line.minutes].sort((a, b) => a - b) }))
      .sort((a, b) => (a.minutes[0] ?? Infinity) - (b.minutes[0] ?? Infinity))
  }
  return out
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

// ---------------------------------------------------------------------------
// Scoring method — which of netball's two live-scoring methods this match's
// scorer is using. Client-only, same as football's `TrackPrefs`: the backend
// doesn't need to know (both methods write the exact same `NetballLiveEvent`
// vocabulary — see `NetballLiveEvent`'s backend doc comment), this just picks
// which screen `NetballLiveScoringPage` shows. Chosen once, before the first
// event, and inferred after that from whether the live state already has
// goal-by-goal detail (see `NetballLiveScoringPage`), so this preference is
// really just a tiebreaker for a fresh match with no events yet.
// ---------------------------------------------------------------------------

export type NetballScoringMethod = 'event_by_event' | 'quarter_only'

function methodKey(matchId: string): string {
  return `agon:netball-scoring-method:${matchId}`
}

export function loadNetballScoringMethod(matchId: string): NetballScoringMethod | null {
  try {
    const raw = localStorage.getItem(methodKey(matchId))
    return raw === 'event_by_event' || raw === 'quarter_only' ? raw : null
  } catch {
    return null
  }
}

export function saveNetballScoringMethod(matchId: string, method: NetballScoringMethod): void {
  try {
    localStorage.setItem(methodKey(matchId), method)
  } catch {
    // Best-effort — a private-browsing/full-storage failure just means the
    // choice is asked again next visit, not worth surfacing.
  }
}

// ---------------------------------------------------------------------------
// Match clock — derived from `NetballScore.period_times`, the same "shared
// server data, not a per-device stopwatch" approach as football's clock (see
// `lib/liveScore.ts`'s module doc comment). Unlike football, a goal's
// `minute` is scoped to the *current quarter* (it resets each quarter), so
// the clock here doesn't need to accumulate across periods the way
// football's does — it's just time since whichever marker started the
// current quarter.
// ---------------------------------------------------------------------------

export type ClockPhase =
  | 'not_started'
  | 'quarter_1'
  | 'quarter_2'
  | 'quarter_3'
  | 'quarter_4'
  | 'full_time'
  | 'extra_time'
  | 'finished'

/** Which phase the match is in right now, purely from recorded markers. Each
 *  marker doubles as the start of the next quarter — netball's breaks are
 *  too short to need a separate "quarter N start" event (see
 *  `NetballPeriod`'s backend doc comment). */
export function phaseFromState(state: NetballScore): ClockPhase {
  if (periodTime(state, 'extra_time_end')) return 'finished'
  if (periodTime(state, 'extra_time_start')) return 'extra_time'
  if (periodTime(state, 'full_time')) return 'full_time'
  if (periodTime(state, 'quarter_three_end')) return 'quarter_4'
  if (periodTime(state, 'quarter_two_end')) return 'quarter_3'
  if (periodTime(state, 'quarter_one_end')) return 'quarter_2'
  if (periodTime(state, 'start')) return 'quarter_1'
  return 'not_started'
}

/** The marker that starts the *current* quarter — what the clock counts
 *  minutes from (`currentMinute`) — `undefined` for a phase with no clock
 *  running (`not_started`, `full_time`, `finished`). */
function currentQuarterStartedAt(state: NetballScore, phase: ClockPhase): string | undefined {
  switch (phase) {
    case 'quarter_1':
      return periodTime(state, 'start')
    case 'quarter_2':
      return periodTime(state, 'quarter_one_end')
    case 'quarter_3':
      return periodTime(state, 'quarter_two_end')
    case 'quarter_4':
      return periodTime(state, 'quarter_three_end')
    case 'extra_time':
      return periodTime(state, 'extra_time_start')
    case 'not_started':
    case 'full_time':
    case 'finished':
      return undefined
  }
}

/** Minutes elapsed in the current quarter — matches the scope of
 *  `NetballGoalEvent.minute`, so this can prefill a new goal's minute field
 *  directly. `null` when there's no quarter currently running. */
export function currentMinute(state: NetballScore, now: Date = new Date()): number | null {
  const phase = phaseFromState(state)
  const startedAt = currentQuarterStartedAt(state, phase)
  if (!startedAt) return null
  return Math.max(0, Math.floor((now.getTime() - new Date(startedAt).getTime()) / 60_000))
}

/** Whether the ball is live right now — the phases a goal/foul can
 *  meaningfully be recorded in. Used to gate the live scoring screen's quick
 *  actions on the current phase, same role as football's `isLivePlayPhase`. */
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
    case 'quarter_2':
      return '2nd quarter'
    case 'quarter_3':
      return '3rd quarter'
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
 *  from the current phase — what the live scoring screen's quarter-end quick
 *  action should send. `null` once the match is done, or before it's
 *  started (the first event any live-scoring method records is `Start`
 *  itself, so there's no "next marker" to prefill before that). */
export function nextPeriodForPhase(
  phase: ClockPhase,
  ctx: NetballProgressionContext,
): NetballPeriod | null {
  switch (phase) {
    case 'not_started':
      return 'start'
    case 'quarter_1':
      return 'quarter_one_end'
    case 'quarter_2':
      return 'quarter_two_end'
    case 'quarter_3':
      return 'quarter_three_end'
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
    case 'quarter_2':
      return 'End 2nd quarter (half-time)'
    case 'quarter_3':
      return 'End 3rd quarter'
    case 'quarter_4':
      return ctx.isDraw && ctx.extraTime ? 'Full-time — start extra time' : 'Full-time'
    case 'extra_time':
      return 'End extra time'
    case 'full_time':
    case 'finished':
      return 'Full-time'
  }
}
