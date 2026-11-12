/**
 * Fixture data for the team-page design prototype (`/design/team`, dev only).
 *
 * Nothing here comes from the API. The shapes are deliberately written as the
 * *proposed* responses the real page would need, so the prototype doubles as
 * an API sketch — see `docs/teams-page-scope.md`:
 *
 * - `TeamSummary`     → `GET /teams/{id}/stats?sport=`  (team record, form, streaks)
 * - `LeaderboardRow`  → `GET /teams/{id}/leaderboard?sport=&scope=`
 *                        (per-player stats *for this team*, not lifetime)
 * - `Rival`           → derivable from the same per-team match aggregation
 *
 * Only `football` and `cricket` are filled in: they're the two sports with
 * rich per-player stats today (goals/assists; runs/wickets), which is what
 * makes a leaderboard interesting rather than just W/L.
 */

export type DesignSport = 'football' | 'cricket'
export type Result = 'W' | 'D' | 'L'

export interface DesignTeam {
  name: string
  founded: string
  followers: number
  members: number
  sports: DesignSport[]
  myRole: 'owner' | 'admin' | 'member' | null
}

export interface HeadlineStat {
  label: string
  value: string
  /** Optional small line under the value, e.g. "per match". */
  hint?: string
}

export interface FormMatch {
  result: Result
  opponent: string
  score: string
  date: string
}

export interface TeamSummary {
  played: number
  wins: number
  draws: number
  losses: number
  winRatePct: number
  /** Sport-specific headline extras (goals for/against, runs, etc). */
  extras: HeadlineStat[]
  /** Last 10, most recent first. */
  form: FormMatch[]
  currentStreak: { kind: Result; length: number }
  longestWinStreak: number
  biggestWin: { opponent: string; score: string; date: string }
  /** Per-match "for" vs "against" over the last 10, oldest first — for the trend chart. */
  trend: { forValue: number; againstValue: number; result: Result }[]
  trendLabel: { forLabel: string; againstLabel: string }
}

export interface LeaderboardMetric {
  key: string
  label: string
  /** Short header for the table column. */
  short: string
  /** Lower is better (e.g. economy). */
  ascending?: boolean
  format?: (v: number) => string
}

export interface LeaderboardRow {
  userId: string
  name: string
  isMe?: boolean
  role?: 'owner' | 'admin'
  apps: number
  wins: number
  winPct: number
  /** Published rating on the sport's ladder, if any. */
  rating?: number
  /** Places moved since the team's previous match, on the default metric. */
  movement: number
  /** Sport-specific counters keyed by `LeaderboardMetric.key`. */
  stats: Record<string, number>
  /** Badges earned for this team — "Hat-trick", "Ever-present", etc. */
  badges?: string[]
}

export interface Rival {
  name: string
  played: number
  w: number
  d: number
  l: number
}

export interface Fixture {
  opponent: string
  when: string
  where: string
  confirmed: number
  needed: number
}

export const team: DesignTeam = {
  name: 'Hackney Wick FC',
  founded: 'Mar 2025',
  followers: 128,
  members: 18,
  sports: ['football', 'cricket'],
  myRole: 'admin',
}

export const nextFixture: Fixture = {
  opponent: 'Clapton Rovers',
  when: 'Sat 15 Nov · 10:30',
  where: 'Hackney Marshes, Pitch 42',
  confirmed: 9,
  needed: 11,
}

const pct = (v: number) => `${Math.round(v)}%`
const dec1 = (v: number) => v.toFixed(1)
const dec2 = (v: number) => v.toFixed(2)

export const metrics: Record<DesignSport, LeaderboardMetric[]> = {
  football: [
    { key: 'rating', label: 'Rating', short: 'Rtg', format: String },
    { key: 'goals', label: 'Goals', short: 'G' },
    { key: 'assists', label: 'Assists', short: 'A' },
    { key: 'ga', label: 'Goal contributions', short: 'G+A' },
    { key: 'apps', label: 'Appearances', short: 'Apps' },
    { key: 'winPct', label: 'Win rate', short: 'Win%', format: pct },
    { key: 'gpm', label: 'Goals per match', short: 'G/M', format: dec2 },
  ],
  cricket: [
    { key: 'rating', label: 'Rating', short: 'Rtg', format: String },
    { key: 'runs', label: 'Runs', short: 'R' },
    { key: 'wickets', label: 'Wickets', short: 'W' },
    { key: 'avg', label: 'Batting avg', short: 'Avg', format: dec1 },
    { key: 'econ', label: 'Economy', short: 'Econ', ascending: true, format: dec2 },
    { key: 'catches', label: 'Catches', short: 'Ct' },
    { key: 'apps', label: 'Appearances', short: 'Apps' },
  ],
}

export const summaries: Record<DesignSport, TeamSummary> = {
  football: {
    played: 24,
    wins: 14,
    draws: 4,
    losses: 6,
    winRatePct: 58.3,
    extras: [
      { label: 'Goals for', value: '71', hint: '2.96 / match' },
      { label: 'Against', value: '43', hint: '1.79 / match' },
      { label: 'Goal diff', value: '+28' },
      { label: 'Clean sheets', value: '5' },
    ],
    form: [
      { result: 'W', opponent: 'Clapton Rovers', score: '4–2', date: '8 Nov' },
      { result: 'W', opponent: 'Victoria Park Utd', score: '3–1', date: '1 Nov' },
      { result: 'W', opponent: 'Lea Bridge Lions', score: '5–0', date: '25 Oct' },
      { result: 'L', opponent: 'Dalston Athletic', score: '1–3', date: '18 Oct' },
      { result: 'D', opponent: 'Homerton Harriers', score: '2–2', date: '11 Oct' },
      { result: 'W', opponent: 'Stoke Newington', score: '3–2', date: '4 Oct' },
      { result: 'L', opponent: 'Dalston Athletic', score: '0–2', date: '27 Sep' },
      { result: 'W', opponent: 'Bow Town', score: '4–1', date: '20 Sep' },
      { result: 'W', opponent: 'Clapton Rovers', score: '2–1', date: '13 Sep' },
      { result: 'D', opponent: 'Mile End Albion', score: '1–1', date: '6 Sep' },
    ],
    currentStreak: { kind: 'W', length: 3 },
    longestWinStreak: 5,
    biggestWin: { opponent: 'Lea Bridge Lions', score: '5–0', date: '25 Oct' },
    trend: [
      { forValue: 1, againstValue: 1, result: 'D' },
      { forValue: 2, againstValue: 1, result: 'W' },
      { forValue: 4, againstValue: 1, result: 'W' },
      { forValue: 0, againstValue: 2, result: 'L' },
      { forValue: 3, againstValue: 2, result: 'W' },
      { forValue: 2, againstValue: 2, result: 'D' },
      { forValue: 1, againstValue: 3, result: 'L' },
      { forValue: 5, againstValue: 0, result: 'W' },
      { forValue: 3, againstValue: 1, result: 'W' },
      { forValue: 4, againstValue: 2, result: 'W' },
    ],
    trendLabel: { forLabel: 'Scored', againstLabel: 'Conceded' },
  },
  cricket: {
    played: 9,
    wins: 5,
    draws: 0,
    losses: 4,
    winRatePct: 55.6,
    extras: [
      { label: 'Runs scored', value: '1,284', hint: '143 / innings' },
      { label: 'Wickets taken', value: '71' },
      { label: 'Highest total', value: '196/4' },
      { label: 'Run rate', value: '7.18' },
    ],
    form: [
      { result: 'W', opponent: 'Victoria Park CC', score: 'won by 34 runs', date: '2 Nov' },
      { result: 'L', opponent: 'Hackney Hawks', score: 'lost by 3 wkts', date: '26 Oct' },
      { result: 'W', opponent: 'Bow Bandits', score: 'won by 6 wkts', date: '19 Oct' },
      { result: 'W', opponent: 'Lea Valley XI', score: 'won by 12 runs', date: '12 Oct' },
      { result: 'L', opponent: 'Hackney Hawks', score: 'lost by 41 runs', date: '5 Oct' },
    ],
    currentStreak: { kind: 'W', length: 1 },
    longestWinStreak: 3,
    biggestWin: { opponent: 'Bow Bandits', score: 'won by 78 runs', date: '7 Sep' },
    trend: [
      { forValue: 121, againstValue: 140, result: 'L' },
      { forValue: 196, againstValue: 118, result: 'W' },
      { forValue: 154, againstValue: 142, result: 'W' },
      { forValue: 102, againstValue: 143, result: 'L' },
      { forValue: 162, againstValue: 150, result: 'W' },
      { forValue: 133, againstValue: 136, result: 'L' },
      { forValue: 147, againstValue: 113, result: 'W' },
    ],
    trendLabel: { forLabel: 'Runs for', againstLabel: 'Runs against' },
  },
}

export const leaderboards: Record<DesignSport, LeaderboardRow[]> = {
  football: [
    { userId: 'u1', name: 'Marcus Okafor', role: 'owner', apps: 23, wins: 14, winPct: 60.9, rating: 1712, movement: 0, stats: { goals: 19, assists: 7, ga: 26, gpm: 0.83 }, badges: ['Top scorer', 'Hat-trick ×2'] },
    { userId: 'u2', name: 'Priya Shah', apps: 21, wins: 13, winPct: 61.9, rating: 1688, movement: 1, stats: { goals: 11, assists: 12, ga: 23, gpm: 0.52 }, badges: ['Playmaker'] },
    { userId: 'u3', name: 'Tom Hughes', isMe: true, role: 'admin', apps: 24, wins: 14, winPct: 58.3, rating: 1641, movement: -1, stats: { goals: 9, assists: 8, ga: 17, gpm: 0.38 }, badges: ['Ever-present'] },
    { userId: 'u4', name: 'Dan Mensah', apps: 18, wins: 11, winPct: 61.1, rating: 1603, movement: 2, stats: { goals: 8, assists: 3, ga: 11, gpm: 0.44 } },
    { userId: 'u5', name: 'Sofia Rossi', apps: 15, wins: 10, winPct: 66.7, rating: 1590, movement: 0, stats: { goals: 6, assists: 5, ga: 11, gpm: 0.4 } },
    { userId: 'u6', name: 'Jamal Ahmed', apps: 20, wins: 10, winPct: 50, movement: -2, stats: { goals: 5, assists: 6, ga: 11, gpm: 0.25 } },
    { userId: 'u7', name: 'Ellie Turner', apps: 12, wins: 7, winPct: 58.3, rating: 1544, movement: 0, stats: { goals: 4, assists: 2, ga: 6, gpm: 0.33 } },
    { userId: 'u8', name: 'Kofi Boateng', apps: 9, wins: 4, winPct: 44.4, movement: 1, stats: { goals: 2, assists: 3, ga: 5, gpm: 0.22 } },
    { userId: 'u9', name: 'Ben Carter', apps: 16, wins: 8, winPct: 50, rating: 1498, movement: -1, stats: { goals: 1, assists: 1, ga: 2, gpm: 0.06 }, badges: ['Clean sheets ×5'] },
  ],
  cricket: [
    { userId: 'u3', name: 'Tom Hughes', isMe: true, role: 'admin', apps: 9, wins: 5, winPct: 55.6, rating: 1602, movement: 0, stats: { runs: 312, wickets: 4, avg: 39.0, econ: 8.1, catches: 3 }, badges: ['Half-century ×3'] },
    { userId: 'u10', name: 'Arjun Patel', apps: 9, wins: 5, winPct: 55.6, rating: 1655, movement: 1, stats: { runs: 188, wickets: 17, avg: 23.5, econ: 5.9, catches: 5 }, badges: ['5-for'] },
    { userId: 'u2', name: 'Priya Shah', apps: 8, wins: 5, winPct: 62.5, rating: 1580, movement: -1, stats: { runs: 241, wickets: 9, avg: 34.4, econ: 7.2, catches: 6 } },
    { userId: 'u11', name: 'Will Fraser', apps: 7, wins: 3, winPct: 42.9, movement: 0, stats: { runs: 96, wickets: 14, avg: 13.7, econ: 6.4, catches: 2 } },
    { userId: 'u1', name: 'Marcus Okafor', role: 'owner', apps: 6, wins: 4, winPct: 66.7, movement: 2, stats: { runs: 154, wickets: 2, avg: 25.7, econ: 9.3, catches: 4 } },
    { userId: 'u12', name: 'Hannah Lee', apps: 8, wins: 4, winPct: 50, rating: 1512, movement: -2, stats: { runs: 72, wickets: 11, avg: 12.0, econ: 6.8, catches: 7 }, badges: ['Safe hands'] },
  ],
}

// `apps` / `winPct` / `rating` live on the row itself; fold them into `stats`
// so every metric sorts the same way.
for (const rows of Object.values(leaderboards)) {
  for (const row of rows) {
    row.stats.apps = row.apps
    row.stats.winPct = row.winPct
    if (row.rating !== undefined) row.stats.rating = row.rating
  }
}

export const rivals: Record<DesignSport, Rival[]> = {
  football: [
    { name: 'Clapton Rovers', played: 5, w: 4, d: 0, l: 1 },
    { name: 'Dalston Athletic', played: 4, w: 1, d: 0, l: 3 },
    { name: 'Homerton Harriers', played: 3, w: 1, d: 2, l: 0 },
  ],
  cricket: [
    { name: 'Hackney Hawks', played: 3, w: 0, d: 0, l: 3 },
    { name: 'Bow Bandits', played: 2, w: 2, d: 0, l: 0 },
  ],
}
