import type { components } from '@/types/api'
import { memberAvatarUrl, memberName, type ScorePlayers } from './members'

export type FootballPeriod = components['schemas']['FootballPeriod']
export type FootballGoalEvent = components['schemas']['FootballGoalEvent']
type FootballCardEvent = components['schemas']['FootballCardEvent']
type FootballSubstitutionEvent = components['schemas']['FootballSubstitutionEvent']
type Score = components['schemas']['Score']
/** A period-marker timestamp map, straight off `FootballScore.period_times`
 *  — the schema widens the key to a bare string, so this is what every
 *  helper below that just needs the timestamps (not the rest of a
 *  `FootballScore`) accepts. Same role as netball's `NetballPeriodTimes`. */
export type FootballPeriodTimes = Record<string, string>
type MatchPlayer = components['schemas']['MatchPlayer']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
/** Anything with an optional `players` list — see `members.ts`'s `MatchLike`
 *  for why `FeedMatch`/`SearchMatch` need their own explicit branches. */
type MatchLike = { players?: MatchPlayer[] } | FeedMatch | SearchMatch

/** A football `Score` — live or finished, confirmed or not, it's the same
 *  shape either way (see `Score`'s doc comment on the backend). Narrowed via
 *  `type`, so this retains the `type: 'Football'` tag and is itself a valid
 *  `Score` to send back on a PATCH (see `LiveScoringPage`'s `finishMatch`) as
 *  well as a source for the clock/ticker helpers below. */
export type FootballScore = Extract<Score, { type: 'Football' }>

/** Narrows a match's score to its football variant, or `null` when there's
 *  none yet or it's for a different sport. */
export function footballScoreFrom(score: Score | null | undefined): FootballScore | null {
  if (!score || score.type !== 'Football') return null
  return score
}

/** What `eventsFromDetail` actually reads off a `FootballScore` — narrowed
 *  so a finished match's `Score.Football` (whose `goals`/`cards`/
 *  `substitutions` are optional) and a live one (same optionality now) can
 *  both feed the same timeline via `footballEventSourceFromScore`. Carries
 *  `players` (the score's resolved-name map) alongside so `describeEvent`
 *  can name scorers/assists/subs on a feed/search card too — see
 *  `playerNameFor`. */
export type FootballEventSource = {
  goals: FootballGoalEvent[]
  cards: FootballCardEvent[]
  substitutions: FootballSubstitutionEvent[]
  players: ScorePlayers
  period_times?: FootballPeriodTimes
}

/** A football match's event timeline straight off its score — `null` if
 *  there's no score yet, it's a different sport, or it's a football score
 *  with no detail attached (a manual entry with just the final tally). Lets
 *  `FootballScorecard` render nothing rather than an empty "Match events"
 *  section for a bare scoreline. */
export function footballEventSourceFromScore(score: Score | null | undefined): FootballEventSource | null {
  if (!score || score.type !== 'Football') return null
  if (!score.goals && !score.cards && !score.substitutions) return null
  return {
    goals: score.goals ?? [],
    cards: score.cards ?? [],
    substitutions: score.substitutions ?? [],
    players: score.players,
    period_times: score.period_times,
  }
}

/** A client-side view of one football event, merging `FootballScore`'s
 *  separately-typed `goals`/`cards`/`substitutions` lists back into a single
 *  timeline for the event log/mini-ticker (see `eventsFromDetail`) — the
 *  backend keeps them apart so a goal's scorer/assist/own-goal/penalty
 *  fields mean the same thing they did when recorded, not a shared field
 *  reinterpreted per kind. */
export type FootballEventKind = 'goal' | 'own_goal' | 'penalty' | 'yellow_card' | 'red_card' | 'substitution'
export interface FootballEventView {
  kind: FootballEventKind
  side_id: string
  /** Free-text minutes-into-the-match — only ever set on a manually logged
   *  historical event (no live clock). A live-scored event's minute is
   *  derived from `occurred_at` instead; see `eventClockLabel`. */
  minute?: number
  /** Wall-clock time, set on every live-scored event (and unset on a
   *  manually logged one) — the source of truth for both display (via
   *  `eventClockLabel`) and chronological order (via `eventsFromDetail`'s
   *  sort). */
  occurred_at?: string
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

/** Maps a bare goals list — `FootballScore.goals`, live or finished — to
 *  event views. Unsorted; callers order as needed (`eventsFromDetail` merges
 *  and sorts alongside cards/subs). */
export function goalEventsToViews(goals: FootballGoalEvent[]): FootballEventView[] {
  return goals.map((g): FootballEventView => ({
    kind: goalKind(g),
    side_id: g.side_id,
    minute: g.minute,
    occurred_at: g.occurred_at,
    player_id: g.scorer_player_id,
    assist_player_id: g.assist_player_id,
  }))
}

/** Sort key for ordering events chronologically: `occurred_at` (a
 *  live-scored event's wall-clock time) when present, else a coarse
 *  fallback off the free-text `minute` a manually logged event carries
 *  instead, else last. Same reasoning as netball's `eventSortKey`. */
function eventSortKey(view: { occurred_at?: string; minute?: number }): number {
  if (view.occurred_at) return new Date(view.occurred_at).getTime()
  if (view.minute !== undefined) return view.minute * 60_000
  return Infinity
}

/** All of a football score's goals/cards/substitutions merged into one
 *  timeline, in true chronological order (undated events last). Takes just
 *  `FootballEventSource` (not the full `FootballScore`) so a finished
 *  match's `Score.Football` can feed it too — see
 *  `footballEventSourceFromScore`. */
export function eventsFromDetail(detail: FootballEventSource): FootballEventView[] {
  const events: FootballEventView[] = [
    ...goalEventsToViews(detail.goals),
    ...detail.cards.map((c): FootballEventView => ({
      kind: cardKind(c),
      side_id: c.side_id,
      minute: c.minute,
      occurred_at: c.occurred_at,
      player_id: c.player_id,
    })),
    ...detail.substitutions.map((s): FootballEventView => ({
      kind: 'substitution',
      side_id: s.side_id,
      minute: s.minute,
      occurred_at: s.occurred_at,
      player_id: s.player_in_id,
      substituted_player_id: s.player_out_id,
    })),
  ]
  return events.sort((a, b) => eventSortKey(a) - eventSortKey(b))
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

/** "63'" for a recorded free-text minute (manual entry only — see
 *  `FootballGoalEvent.minute`'s backend doc comment), else a blank string. A
 *  live-scored event should use `eventClockLabel` instead, which prefers
 *  `occurred_at`. */
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
export function recentEvents(detail: FootballScore, limit: number): FootballEventView[] {
  return eventsFromDetail({
    goals: detail.goals ?? [],
    cards: detail.cards ?? [],
    substitutions: detail.substitutions ?? [],
    players: detail.players,
    period_times: detail.period_times,
  })
    .reverse()
    .slice(0, limit)
}

/** One goalscorer's line for a side's scorer list — every minute they scored
 *  merged onto one row, e.g. "James Elgar 5', 53'" or "Own goal (J. Smith)
 *  90'". `key` is stable for React lists (a player id, an
 *  `own_goal:<player id>`, or `own_goal:unknown` for an unrecorded own-goal
 *  scorer — see `scorersBySide`). */
export interface FootballScorerLine {
  key: string
  name: string
  /** Already display-formatted (mm' or the legacy bare-minute label — see
   *  `eventClockLabel`), one per goal, in chronological order. */
  times: string[]
}

/** All of a football match's goals, grouped by crediting side and then by
 *  scorer — a side-by-side scorer list for a finished match's feed card,
 *  built straight from the goals embedded in its confirmed/pending
 *  `Score.Football` rather than a full live fetch, which a completed match
 *  no longer needs just to show who scored. Multiple goals by the same
 *  player become one line with every minute, ascending; lines are ordered by
 *  that player's first goal. An own goal still lists under the *benefiting*
 *  side's column, matching where it counts on the scoreboard (`side_id` is
 *  the crediting side, not the own-goal scorer's own team — see
 *  `FootballGoalEvent`'s doc comment), but is labeled "Own goal (Scorer)"
 *  using the scorer's own name, resolved the same way a regular goal's is;
 *  falls back to a bare "Own goal" line when no scorer was recorded. Own
 *  goals by different players are never merged onto the same line, even
 *  though they'd count for the same side. */
export function scorersBySide(
  goals: FootballGoalEvent[],
  match: MatchLike,
  scorePlayers?: ScorePlayers,
  periodTimes?: FootballPeriodTimes,
): Record<string, FootballScorerLine[]> {
  const bySide: Record<string, Map<string, { name: string; entries: { key: number; label: string }[] }>> = {}
  for (const g of goals) {
    const lines = (bySide[g.side_id] ??= new Map())
    const scorer = playerNameFor(match, g.scorer_player_id, scorePlayers)
    const key = g.own_goal ? `own_goal:${g.scorer_player_id ?? 'unknown'}` : (g.scorer_player_id ?? 'unknown')
    const name = g.own_goal ? (scorer ? `Own goal (${scorer})` : 'Own goal') : (scorer ?? 'Unknown')
    const line = lines.get(key) ?? { name, entries: [] }
    const view: FootballEventView = { kind: goalKind(g), side_id: g.side_id, minute: g.minute, occurred_at: g.occurred_at }
    line.entries.push({ key: eventSortKey(view), label: eventClockLabel(view, periodTimes) })
    lines.set(key, line)
  }
  const out: Record<string, FootballScorerLine[]> = {}
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

/** One player's tally for the goal-contributions table: how many goals they
 *  scored plus how many they assisted, across the whole match (both sides
 *  merged into one list — `side_id` is kept per row for a caller that wants
 *  to badge/group it, but the table itself doesn't split by side the way
 *  `scorersBySide` does). */
export interface GoalContributionEntry {
  /** A scorer/assister player id — stable for React lists. */
  key: string
  name: string
  side_id: string
  goals: number
  assists: number
  /** Linked Agon account id, for a profile avatar/link — same as the match's
   *  own player-list rows (`SideRoster`). Undefined for an external player,
   *  or a scorer/assister id that resolves to neither `scorePlayers` nor the
   *  match's roster. */
  userId?: string
  avatarUrl?: string
}

/** Every player who scored or assisted at least one goal, tallied across the
 *  whole match. Mirrors the backend's own "goal contributions" accounting
 *  (`agon_worker`'s per-match stat contribution, and
 *  `FootballStatsRecord::best_goal_contributions`): an own goal counts for
 *  the scoring side on the board but is never credited to the player who
 *  put it in their own net, so it's excluded here too — only
 *  `assist_player_id`, which an own goal never carries anyway, would still
 *  count. Unsorted; pass through `sortGoalContributions` for a stable
 *  goals-first or assists-first ordering. */
export function goalContributions(
  goals: FootballGoalEvent[],
  match: MatchLike,
  scorePlayers?: ScorePlayers,
): GoalContributionEntry[] {
  const byPlayer = new Map<string, GoalContributionEntry>()
  const entryFor = (playerId: string, sideId: string) => {
    let entry = byPlayer.get(playerId)
    if (!entry) {
      const info = playerInfoFor(match, playerId, scorePlayers)
      entry = {
        key: playerId,
        name: info?.name ?? 'Unknown',
        side_id: sideId,
        goals: 0,
        assists: 0,
        userId: info?.userId,
        avatarUrl: info?.avatarUrl,
      }
      byPlayer.set(playerId, entry)
    }
    return entry
  }

  for (const g of goals) {
    if (!g.own_goal && g.scorer_player_id) {
      entryFor(g.scorer_player_id, g.side_id).goals += 1
    }
    if (g.assist_player_id) {
      entryFor(g.assist_player_id, g.side_id).assists += 1
    }
  }
  return [...byPlayer.values()]
}

/** Which column leads the goal-contributions table's sort. */
export type GoalContributionSort = 'goals' | 'assists'

/** Sorts goal-contribution rows by the chosen column descending, using the
 *  other column as the tie-breaker (also descending) — sorting by goals
 *  breaks ties on assists and vice versa, per the table's brief — then name
 *  for a fully stable order once both counts match. */
export function sortGoalContributions(
  entries: GoalContributionEntry[],
  sortBy: GoalContributionSort,
): GoalContributionEntry[] {
  const other: GoalContributionSort = sortBy === 'goals' ? 'assists' : 'goals'
  return [...entries].sort(
    (a, b) => b[sortBy] - a[sortBy] || b[other] - a[other] || a.name.localeCompare(b.name),
  )
}

/** Player identity for a match-scoped player id: display name plus, when the
 *  player is a linked Agon account, their `userId`/`avatarUrl` for a profile
 *  link and avatar — same fields `SideRoster` uses for the match's player
 *  list. Checks `scorePlayers` first — `FootballScore.players`, resolved
 *  server-side for exactly the ids a score references (see its backend doc
 *  comment) — since that's the only source a feed/search card's trimmed
 *  match type has at all (it never carries a `players` list). Falls back to
 *  scanning the match's own roster, always present on the full `Match` type
 *  a detail view uses. An external player (or an id neither source
 *  resolves) has no `userId`/`avatarUrl`. */
export function playerInfoFor(
  match: MatchLike,
  playerId: string | undefined,
  scorePlayers?: ScorePlayers,
): { name: string; userId?: string; avatarUrl?: string } | null {
  if (!playerId) return null
  const resolved = scorePlayers?.[playerId]
  if (resolved) {
    return {
      name: resolved.name,
      userId: resolved.user_id ?? undefined,
      avatarUrl: resolved.avatar_url ?? undefined,
    }
  }
  const players = ('players' in match && match.players) || []
  const player = players.find((p) => p.member.id === playerId)
  if (!player) return null
  return {
    name: memberName(player.member),
    userId: player.member.type === 'User' ? player.member.user_id : undefined,
    avatarUrl: memberAvatarUrl(player.member),
  }
}

/** Player display name for a match-scoped player id. Same resolution order
 *  as `playerInfoFor` — see its doc comment. Same contract as cricket's
 *  `playerNameFor` in `lib/cricketScore.ts`. */
export function playerNameFor(
  match: MatchLike,
  playerId: string | undefined,
  scorePlayers?: ScorePlayers,
): string | null {
  return playerInfoFor(match, playerId, scorePlayers)?.name ?? null
}

/** One-line human description of a football event, e.g. "Goal — J. Alvarez
 *  (A. Silva)" or "Sub — Moreno on for Khan" — used by the event log and
 *  mini-ticker. Which side it belongs to isn't repeated in the text; callers
 *  convey that by aligning/positioning the row using `event.side_id`
 *  instead (see `LiveScoringPage`/`LiveMatchBlock`). `scorePlayers` is the
 *  owning score's resolved-name map — see `playerNameFor`. */
export function describeEvent(
  event: FootballEventView,
  match: MatchLike,
  scorePlayers?: ScorePlayers,
): string {
  const scorer = playerNameFor(match, event.player_id, scorePlayers)

  switch (event.kind) {
    case 'goal':
    case 'penalty': {
      const assist = playerNameFor(match, event.assist_player_id, scorePlayers)
      const base = scorer ? `${eventLabel(event.kind)} — ${scorer}` : eventLabel(event.kind)
      return assist ? `${base} (${assist})` : base
    }
    case 'own_goal':
      return eventLabel(event.kind)
    case 'yellow_card':
    case 'red_card':
      return scorer ? `${eventLabel(event.kind)} — ${scorer}` : eventLabel(event.kind)
    case 'substitution': {
      const out = playerNameFor(match, event.substituted_player_id, scorePlayers)
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
// Match clock — derived entirely from `FootballScore.period_times`, each the
// `occurred_at` of the period marker that recorded it server-side (see
// `agon_service::live_score::football`). Because it's computed from shared
// server data rather than a per-device stopwatch, the scorer's own screen
// and every other viewer (feed card, match detail) tick the exact same
// clock. `period_times` is `undefined` for a score with no live detail
// behind it at all — every lookup below already treats that the same as "no
// marker recorded yet".
// ---------------------------------------------------------------------------

/** When a given period marker was recorded, if at all — a typed lookup into
 *  `FootballScore.period_times`'s string-keyed map. */
export function periodTime(detail: FootballScore, period: FootballPeriod): string | undefined {
  return detail.period_times?.[period]
}

/** Same lookup as `periodTime`, but off a bare `FootballPeriodTimes` map
 *  instead of a full `FootballScore` — for helpers (`minuteAt`,
 *  `eventClockLabel`) that need to work from just the timestamps, e.g. a
 *  finished match's `FootballEventSource`, which doesn't carry a full score. */
function periodTimeFrom(periodTimes: FootballPeriodTimes | undefined, period: FootballPeriod): string | undefined {
  return periodTimes?.[period]
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
export function phaseFromState(state: FootballScore): ClockPhase {
  if (periodTime(state, 'penalties_complete') || (state.penalty_shootout?.length ?? 0) > 0) {
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
export function penaltiesComplete(state: FootballScore): boolean {
  return !!periodTime(state, 'penalties_complete')
}

/** A side's penalty-shootout tally (kicks scored, not taken) — the shootout
 *  equivalent of reading `FootballScore.score` for goals. */
export function shootoutScoreFor(state: FootballScore, sideId: string | undefined): number {
  return (sideId ? state.penalty_shootout_score?.[sideId] : undefined) ?? 0
}

function minutesBetween(from: string, to: Date | string): number {
  return Math.max(0, Math.floor((new Date(to).getTime() - new Date(from).getTime()) / 60_000))
}

/** The cumulative-minute baselines each live-play bracket starts from —
 *  shared by `currentMinute` (evaluated at "now") and `minuteAt` (evaluated
 *  at an arbitrary past instant, e.g. a recorded event's `occurred_at`) so
 *  the two can never disagree about where the clock resumes after a break.
 *  Added time in a half carries over automatically into the next one —
 *  first half into second half, and normal time into extra time — so the
 *  clock counts up continuously (e.g. 94', then 106' once extra time
 *  starts) rather than resetting to a fixed 45'/90' each time. */
function clockBaselines(periodTimes: FootballPeriodTimes | undefined) {
  const kickoffAt = periodTimeFrom(periodTimes, 'kick_off')
  const halfTimeAt = periodTimeFrom(periodTimes, 'half_time')
  const secondHalfKickoffAt = periodTimeFrom(periodTimes, 'second_half_kick_off')
  const fullTimeAt = periodTimeFrom(periodTimes, 'full_time')
  const etKickoffAt = periodTimeFrom(periodTimes, 'extra_time_kick_off')
  const etHalfTimeAt = periodTimeFrom(periodTimes, 'extra_time_half_time')
  const etSecondHalfKickoffAt = periodTimeFrom(periodTimes, 'extra_time_second_half_kick_off')
  const etFullTimeAt = periodTimeFrom(periodTimes, 'extra_time_full_time')

  const firstHalfMinutes = kickoffAt && halfTimeAt ? minutesBetween(kickoffAt, halfTimeAt) : 0
  const normalTimeMinutes = fullTimeAt
    ? secondHalfKickoffAt
      ? firstHalfMinutes + minutesBetween(secondHalfKickoffAt, fullTimeAt)
      : kickoffAt
        ? minutesBetween(kickoffAt, fullTimeAt)
        : 0
    : 0
  const etFirstHalfMinutes = etKickoffAt && etHalfTimeAt ? minutesBetween(etKickoffAt, etHalfTimeAt) : 0

  return {
    kickoffAt,
    halfTimeAt,
    secondHalfKickoffAt,
    fullTimeAt,
    etKickoffAt,
    etHalfTimeAt,
    etSecondHalfKickoffAt,
    etFullTimeAt,
    firstHalfMinutes,
    normalTimeMinutes,
    etFirstHalfMinutes,
  }
}

/**
 * The current match minute, e.g. for the live clock display. `null` before
 * kickoff or during an unclocked phase (penalties). See `clockBaselines` for
 * how added time/extra time carry over.
 */
export function currentMinute(state: FootballScore, now: Date = new Date()): number | null {
  const phase = phaseFromState(state)
  const {
    kickoffAt,
    halfTimeAt,
    secondHalfKickoffAt,
    fullTimeAt,
    etKickoffAt,
    etHalfTimeAt,
    etSecondHalfKickoffAt,
    etFullTimeAt,
    firstHalfMinutes,
    normalTimeMinutes,
    etFirstHalfMinutes,
  } = clockBaselines(state.period_times)

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

/** The match minute a past instant (`occurredAt`) fell in — the same
 *  bracket-and-carry-over math as `currentMinute`, but bracketing an
 *  arbitrary timestamp against the recorded markers instead of dispatching
 *  on the match's *current* phase. This is what a live-scored event's
 *  displayed minute is derived from (see `eventClockLabel`), rather than
 *  trusting a stored/typed number: extra time layers on top of normal time
 *  automatically, and a break's duration is never counted, exactly like the
 *  live clock itself. `null` if `occurredAt` predates kickoff (shouldn't
 *  happen for a real event, but keeps this total rather than guessing). */
export function minuteAt(periodTimes: FootballPeriodTimes | undefined, occurredAt: string): number | null {
  const b = clockBaselines(periodTimes)
  const t = new Date(occurredAt).getTime()
  const since = (marker: string) => minutesBetween(marker, occurredAt)

  // Checked from the latest bracket backward — an event's `occurred_at`
  // should never predate the marker that started its own bracket, so the
  // first (latest) marker it's at-or-after is the one it belongs to.
  if (b.etSecondHalfKickoffAt && t >= new Date(b.etSecondHalfKickoffAt).getTime()) {
    return b.normalTimeMinutes + b.etFirstHalfMinutes + since(b.etSecondHalfKickoffAt)
  }
  if (b.etKickoffAt && t >= new Date(b.etKickoffAt).getTime()) {
    return b.normalTimeMinutes + since(b.etKickoffAt)
  }
  if (b.secondHalfKickoffAt && t >= new Date(b.secondHalfKickoffAt).getTime()) {
    return b.firstHalfMinutes + since(b.secondHalfKickoffAt)
  }
  if (b.kickoffAt && t >= new Date(b.kickoffAt).getTime()) {
    return since(b.kickoffAt)
  }
  return null
}

/** Display label for one event's time — "63'" for a live-scored event,
 *  derived from `occurred_at` plus the match's recorded period markers
 *  (`minuteAt`); the legacy free-text minute label (`minuteLabel`) for a
 *  manually logged one. Blank if neither is known (e.g. `periodTimes` wasn't
 *  available to bracket a live event against). */
export function eventClockLabel(event: FootballEventView, periodTimes?: FootballPeriodTimes): string {
  if (event.occurred_at) {
    const minute = minuteAt(periodTimes, event.occurred_at)
    if (minute !== null) return `${minute}'`
  }
  return minuteLabel(event.minute)
}

/** Whether the ball is actually in play right now — the phases where a goal
 *  (or card/sub) can meaningfully happen. Excludes not just `penalties`
 *  (which has its own dedicated recording UI) but every clock-stopped phase —
 *  `not_started`, `half_time`, `full_time`, `extra_time_half_time`,
 *  `extra_time_full_time` — since nothing should be recorded as happening
 *  during a break that hasn't been played yet. Used to gate the live
 *  scoring screen's Goal/Card/Sub quick actions on the current phase, not
 *  just `!== 'penalties'` (see `LiveScoringPage`). */
export function isLivePlayPhase(phase: ClockPhase): boolean {
  return (
    phase === 'first_half' ||
    phase === 'second_half' ||
    phase === 'extra_time_first_half' ||
    phase === 'extra_time_second_half'
  )
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
export function liveClockLabel(state: FootballScore, now: Date = new Date()): string {
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
